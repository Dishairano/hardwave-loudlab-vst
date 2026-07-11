//! WebView-based editor for Hardwave LoudLab.
//!
//! Uses the same hwpacket bridge pattern as KickForge:
//! - Linux/macOS: Rust pushes state via `evaluate_script()`.
//! - Windows: Rust starts a local TCP server, JS polls via `fetch()`.
//!
//! On editor open the current DAW-persisted param state is snapshot'd and
//! injected into the webview init script so the UI never shows stale defaults.

use crossbeam_channel::Receiver;
use nih_plug::editor::Editor;
use nih_plug::prelude::{GuiContext, ParentWindowHandle, Param};
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Generate an identifier that is guaranteed unique within this process.
/// Used to give every plug-in instance its own WebView2 user-data folder so
/// two LoudLabs in the same DAW project don't collide on the same lock file
/// (the documented WebView2 "do not share UserDataFolder" anti-pattern).
fn unique_instance_id() -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{}-{}-{}", pid, nanos, n)
}

/// Cooperative shutdown signal used by the editor's worker threads. Setting
/// the flag wakes any thread that is currently parked in `wait()`, so Drop
/// can join its threads in <1ms instead of polling on an 8ms interval.
struct ShutdownSignal {
    flag: Mutex<bool>,
    cv: Condvar,
}

impl ShutdownSignal {
    fn new() -> Self {
        Self { flag: Mutex::new(false), cv: Condvar::new() }
    }

    fn signal(&self) {
        let mut g = self.flag.lock();
        *g = true;
        self.cv.notify_all();
    }

    fn is_shutdown(&self) -> bool {
        *self.flag.lock()
    }

    /// Sleep up to `timeout`, return early if shutdown is signalled.
    /// Returns `true` if shutdown was signalled.
    fn wait(&self, timeout: Duration) -> bool {
        let mut g = self.flag.lock();
        if *g { return true; }
        let _ = self.cv.wait_for(&mut g, timeout);
        *g
    }
}

use crate::auth;
use crate::params::{Genre, HardwaveMasterParams};
use crate::protocol::MasterPacket;

const LOUDLAB_URL: &str = "https://loudlab.hardwavestudios.com/vst/loudlab";
const EDITOR_WIDTH: u32 = 1100;
const EDITOR_HEIGHT: u32 = 700;

/// Wraps a raw window handle value (usize) so wry can use it via rwh 0.6.
struct RwhWrapper(usize);

unsafe impl Send for RwhWrapper {}
unsafe impl Sync for RwhWrapper {}

impl raw_window_handle::HasWindowHandle for RwhWrapper {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::{HandleError, RawWindowHandle};

        #[cfg(target_os = "linux")]
        let raw = {
            let h = raw_window_handle::XlibWindowHandle::new(self.0 as _);
            RawWindowHandle::Xlib(h)
        };

        #[cfg(target_os = "macos")]
        let raw = {
            // A null NSView from the host means we cannot embed; surface
            // that as Unavailable rather than panicking across the FFI
            // boundary into the host (which would crash the DAW).
            let ns_view = std::ptr::NonNull::new(self.0 as *mut _)
                .ok_or(HandleError::Unavailable)?;
            let h = raw_window_handle::AppKitWindowHandle::new(ns_view);
            RawWindowHandle::AppKit(h)
        };

        #[cfg(target_os = "windows")]
        let raw = {
            let hwnd = std::num::NonZeroIsize::new(self.0 as isize)
                .ok_or(HandleError::Unavailable)?;
            let h = raw_window_handle::Win32WindowHandle::new(hwnd);
            RawWindowHandle::Win32(h)
        };

        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

impl raw_window_handle::HasDisplayHandle for RwhWrapper {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::RawDisplayHandle;

        #[cfg(target_os = "linux")]
        let raw = RawDisplayHandle::Xlib(raw_window_handle::XlibDisplayHandle::new(None, 0));

        #[cfg(target_os = "macos")]
        let raw = RawDisplayHandle::AppKit(raw_window_handle::AppKitDisplayHandle::new());

        #[cfg(target_os = "windows")]
        let raw = RawDisplayHandle::Windows(raw_window_handle::WindowsDisplayHandle::new());

        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw) })
    }
}

/// Build a map of param ID strings to ParamPtr for the IPC handler.
fn build_param_map(params: &HardwaveMasterParams) -> HashMap<String, nih_plug::prelude::ParamPtr> {
    let mut map = HashMap::new();

    // Global
    map.insert("genre".into(), params.genre.as_ptr());
    map.insert("intensity".into(), params.intensity.as_ptr());
    map.insert("input_gain".into(), params.input_gain.as_ptr());
    map.insert("output_gain".into(), params.output_gain.as_ptr());
    map.insert("mix".into(), params.mix.as_ptr());
    map.insert("auto_mode".into(), params.auto_mode.as_ptr());
    map.insert("master_enabled".into(), params.master_enabled.as_ptr());

    // EQ
    map.insert("eq_enabled".into(), params.eq_enabled.as_ptr());
    map.insert("eq_low_freq".into(), params.eq_low_freq.as_ptr());
    map.insert("eq_low_gain".into(), params.eq_low_gain.as_ptr());
    map.insert("eq_low_q".into(), params.eq_low_q.as_ptr());
    map.insert("eq_low_mid_freq".into(), params.eq_low_mid_freq.as_ptr());
    map.insert("eq_low_mid_gain".into(), params.eq_low_mid_gain.as_ptr());
    map.insert("eq_low_mid_q".into(), params.eq_low_mid_q.as_ptr());
    map.insert("eq_high_mid_freq".into(), params.eq_high_mid_freq.as_ptr());
    map.insert("eq_high_mid_gain".into(), params.eq_high_mid_gain.as_ptr());
    map.insert("eq_high_mid_q".into(), params.eq_high_mid_q.as_ptr());
    map.insert("eq_high_freq".into(), params.eq_high_freq.as_ptr());
    map.insert("eq_high_gain".into(), params.eq_high_gain.as_ptr());
    map.insert("eq_high_q".into(), params.eq_high_q.as_ptr());

    // Compressor
    map.insert("comp_enabled".into(), params.comp_enabled.as_ptr());
    map.insert("comp_xover_low".into(), params.comp_xover_low.as_ptr());
    map.insert("comp_xover_mid".into(), params.comp_xover_mid.as_ptr());
    map.insert("comp_xover_high".into(), params.comp_xover_high.as_ptr());
    map.insert("comp_sub_thresh".into(), params.comp_sub_thresh.as_ptr());
    map.insert("comp_sub_ratio".into(), params.comp_sub_ratio.as_ptr());
    map.insert("comp_sub_attack".into(), params.comp_sub_attack.as_ptr());
    map.insert("comp_sub_release".into(), params.comp_sub_release.as_ptr());
    map.insert("comp_lm_thresh".into(), params.comp_lm_thresh.as_ptr());
    map.insert("comp_lm_ratio".into(), params.comp_lm_ratio.as_ptr());
    map.insert("comp_lm_attack".into(), params.comp_lm_attack.as_ptr());
    map.insert("comp_lm_release".into(), params.comp_lm_release.as_ptr());
    map.insert("comp_hm_thresh".into(), params.comp_hm_thresh.as_ptr());
    map.insert("comp_hm_ratio".into(), params.comp_hm_ratio.as_ptr());
    map.insert("comp_hm_attack".into(), params.comp_hm_attack.as_ptr());
    map.insert("comp_hm_release".into(), params.comp_hm_release.as_ptr());
    map.insert("comp_hi_thresh".into(), params.comp_hi_thresh.as_ptr());
    map.insert("comp_hi_ratio".into(), params.comp_hi_ratio.as_ptr());
    map.insert("comp_hi_attack".into(), params.comp_hi_attack.as_ptr());
    map.insert("comp_hi_release".into(), params.comp_hi_release.as_ptr());

    // Stereo
    map.insert("stereo_enabled".into(), params.stereo_enabled.as_ptr());
    map.insert("stereo_width".into(), params.stereo_width.as_ptr());
    map.insert("stereo_mono_bass".into(), params.stereo_mono_bass.as_ptr());
    map.insert("stereo_mono_bass_freq".into(), params.stereo_mono_bass_freq.as_ptr());

    // Limiter
    map.insert("limiter_enabled".into(), params.limiter_enabled.as_ptr());
    map.insert("limiter_ceiling".into(), params.limiter_ceiling.as_ptr());

    // Saturation (Advanced mode)
    map.insert("sat_enabled".into(), params.sat_enabled.as_ptr());
    map.insert("sat_drive".into(), params.sat_drive.as_ptr());
    map.insert("sat_mix".into(), params.sat_mix.as_ptr());

    // Per-band makeup gain (Advanced MBC)
    map.insert("comp_sub_makeup".into(), params.comp_sub_makeup.as_ptr());
    map.insert("comp_lm_makeup".into(), params.comp_lm_makeup.as_ptr());
    map.insert("comp_hm_makeup".into(), params.comp_hm_makeup.as_ptr());
    map.insert("comp_hi_makeup".into(), params.comp_hi_makeup.as_ptr());

    map
}

/// Create a snapshot of the current DAW params as a `MasterPacket`.
pub fn snapshot_params(params: &HardwaveMasterParams) -> MasterPacket {
    let genre_str = match params.genre.value() {
        Genre::Hardstyle => "Hardstyle",
        Genre::Rawstyle => "Rawstyle",
        Genre::Hardcore => "Hardcore",
        Genre::Frenchcore => "Frenchcore",
        Genre::Edm => "EDM",
        Genre::HipHop => "Hip-Hop",
        Genre::Flat => "Flat",
    };

    MasterPacket {
        genre: genre_str.to_string(),
        intensity: params.intensity.value(),
        input_gain: params.input_gain.value(),
        output_gain: params.output_gain.value(),
        mix: params.mix.value(),
        auto_mode: params.auto_mode.value(),

        eq_enabled: params.eq_enabled.value(),
        eq_low_freq: params.eq_low_freq.value(),
        eq_low_gain: params.eq_low_gain.value(),
        eq_low_q: params.eq_low_q.value(),
        eq_low_mid_freq: params.eq_low_mid_freq.value(),
        eq_low_mid_gain: params.eq_low_mid_gain.value(),
        eq_low_mid_q: params.eq_low_mid_q.value(),
        eq_high_mid_freq: params.eq_high_mid_freq.value(),
        eq_high_mid_gain: params.eq_high_mid_gain.value(),
        eq_high_mid_q: params.eq_high_mid_q.value(),
        eq_high_freq: params.eq_high_freq.value(),
        eq_high_gain: params.eq_high_gain.value(),
        eq_high_q: params.eq_high_q.value(),

        comp_enabled: params.comp_enabled.value(),
        comp_xover_low: params.comp_xover_low.value(),
        comp_xover_mid: params.comp_xover_mid.value(),
        comp_xover_high: params.comp_xover_high.value(),
        comp_sub_thresh: params.comp_sub_thresh.value(),
        comp_sub_ratio: params.comp_sub_ratio.value(),
        comp_sub_attack: params.comp_sub_attack.value(),
        comp_sub_release: params.comp_sub_release.value(),
        comp_lm_thresh: params.comp_lm_thresh.value(),
        comp_lm_ratio: params.comp_lm_ratio.value(),
        comp_lm_attack: params.comp_lm_attack.value(),
        comp_lm_release: params.comp_lm_release.value(),
        comp_hm_thresh: params.comp_hm_thresh.value(),
        comp_hm_ratio: params.comp_hm_ratio.value(),
        comp_hm_attack: params.comp_hm_attack.value(),
        comp_hm_release: params.comp_hm_release.value(),
        comp_hi_thresh: params.comp_hi_thresh.value(),
        comp_hi_ratio: params.comp_hi_ratio.value(),
        comp_hi_attack: params.comp_hi_attack.value(),
        comp_hi_release: params.comp_hi_release.value(),

        stereo_enabled: params.stereo_enabled.value(),
        stereo_width: params.stereo_width.value(),
        stereo_mono_bass: params.stereo_mono_bass.value(),
        stereo_mono_bass_freq: params.stereo_mono_bass_freq.value(),

        limiter_enabled: params.limiter_enabled.value(),
        limiter_ceiling: params.limiter_ceiling.value(),

        sat_enabled: params.sat_enabled.value(),
        sat_drive: params.sat_drive.value(),
        sat_mix: params.sat_mix.value(),

        comp_sub_makeup: params.comp_sub_makeup.value(),
        comp_lm_makeup: params.comp_lm_makeup.value(),
        comp_hm_makeup: params.comp_hm_makeup.value(),
        comp_hi_makeup: params.comp_hi_makeup.value(),

        input_lufs: -120.0,
        output_lufs: -120.0,
        lufs_short_term: -120.0,
        lufs_integrated: -120.0,
        true_peak_db: -120.0,
        dynamic_range: 0.0,
        correlation: 1.0,
        ms_ratio: 0.5,
        sample_rate: 44_100.0,
        track_duration: 0.0,
        is_playing: false,
        lufs_max_momentary: -120.0,
        spectrum: None,
    }
}

/// Build the init JavaScript that gets injected into the webview on load.
fn ipc_init_script(params: &HardwaveMasterParams) -> String {
    let snapshot = snapshot_params(params);
    let initial_json = serde_json::to_string(&snapshot).unwrap_or_else(|_| "null".into());
    let version = env!("CARGO_PKG_VERSION");
    // Stable per-machine id (64-hex) exposed to the webview so trial/start can
    // bind one free trial per machine. Hex-only, safe to inline unescaped.
    let machine_id = crate::crash_reporter::machine_id();

    format!(
        r#"
window.__HARDWAVE_VST = true;
window.__HARDWAVE_VST_VERSION = '{version}';
window.__HARDWAVE_MACHINE_ID = '{machine_id}';
window.__hardwave = {{
    postMessage: function(msg) {{
        window.ipc.postMessage(JSON.stringify(msg));
    }}
}};

(function() {{
    var _init = {initial_json};
    function pushInit() {{
        if (window.__onMasterPacket) {{
            window.__onMasterPacket(_init);
        }} else {{
            setTimeout(pushInit, 50);
        }}
    }}
    if (document.readyState === 'complete') {{ pushInit(); }}
    else {{ window.addEventListener('load', pushInit); }}
}})();
"#,
    )
}

/// Handle IPC messages from the webview (set_param, set_genre, etc.).
fn handle_ipc(
    context: &Arc<dyn GuiContext>,
    param_map: &HashMap<String, nih_plug::prelude::ParamPtr>,
    reset_capture_flag: &std::sync::atomic::AtomicBool,
    raw_body: &str,
) {
    let msg: serde_json::Value = match serde_json::from_str(raw_body) {
        Ok(v) => v,
        Err(_) => return,
    };

    let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match msg_type {
        "set_param" => {
            let id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let value = msg.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
            if let Some(ptr) = param_map.get(id) {
                // The webview sends *plain* values (e.g. -1.4 dB for eq_low_gain,
                // 28 Hz for eq_low_freq, -18.5 dB for comp_sub_thresh). Convert
                // to the normalized [0.0, 1.0] form expected by
                // raw_set_parameter_normalized via preview_normalized — that's
                // the nih-plug way to apply a param's range mapping. Sending the
                // plain value directly used to clamp every interaction to the
                // [0, 1] cap, which is why every knob jumped to its maximum.
                let normalized = unsafe { ptr.preview_normalized(value as f32) };
                unsafe {
                    context.raw_begin_set_parameter(*ptr);
                    context.raw_set_parameter_normalized(*ptr, normalized);
                    context.raw_end_set_parameter(*ptr);
                }
            }
        }
        "reset_capture" => {
            // Set the atomic; process() picks it up at the top of the next
            // block and resets the meters + max-pos + max-momentary in one
            // shot. Using Release ordering pairs with the Acquire load on the
            // audio thread so the reset is visible there.
            reset_capture_flag.store(true, std::sync::atomic::Ordering::Release);
        }
        "save_token" => {
            if let Some(token) = msg.get("token").and_then(|v| v.as_str()) {
                let _ = auth::save_token(token);
            }
        }
        "clear_token" => {
            let _ = auth::clear_token();
        }
        _ => {}
    }
}

pub struct MasterEditor {
    params: Arc<HardwaveMasterParams>,
    packet_rx: Arc<Mutex<Receiver<MasterPacket>>>,
    /// Shared with the audio thread. When the webview sends a
    /// `reset_capture` IPC message we flip this to `true`; the next
    /// `process()` call drains it and resets the LUFS meters, max
    /// position, and max momentary.
    reset_capture_flag: Arc<std::sync::atomic::AtomicBool>,
    auth_token: Option<String>,
    scale_factor: Mutex<f32>,
    /// Process-unique identifier for this plug-in instance. Used to give
    /// every instance its own WebView2 user-data folder so two LoudLabs in
    /// the same DAW project don't collide on the same lock file.
    instance_id: String,
}

impl MasterEditor {
    pub fn new(
        params: Arc<HardwaveMasterParams>,
        packet_rx: Arc<Mutex<Receiver<MasterPacket>>>,
        reset_capture_flag: Arc<std::sync::atomic::AtomicBool>,
        auth_token: Option<String>,
    ) -> Self {
        Self {
            params,
            packet_rx,
            reset_capture_flag,
            auth_token,
            scale_factor: Mutex::new(1.0),
            instance_id: unique_instance_id(),
        }
    }

    fn scaled_size(&self) -> (u32, u32) {
        let f = *self.scale_factor.lock();
        ((EDITOR_WIDTH as f32 * f) as u32, (EDITOR_HEIGHT as f32 * f) as u32)
    }
}

impl Editor for MasterEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn std::any::Any + Send> {
        let packet_rx = Arc::clone(&self.packet_rx);
        let (width, height) = self.scaled_size();

        let version = env!("CARGO_PKG_VERSION");
        let url = match &self.auth_token {
            Some(t) => format!("{}?token={}&v={}", LOUDLAB_URL, t, version),
            None => format!("{}?v={}", LOUDLAB_URL, version),
        };

        let param_map = Arc::new(build_param_map(&self.params));
        let init_js = ipc_init_script(&self.params);
        let raw_handle = extract_raw_handle(&parent);

        #[cfg(target_os = "windows")]
        {
            spawn_windows(
                raw_handle, url, width, height, packet_rx, context, param_map,
                Arc::clone(&self.reset_capture_flag), init_js,
                self.instance_id.clone(),
            )
        }

        #[cfg(not(target_os = "windows"))]
        {
            spawn_unix(raw_handle, url, width, height, packet_rx, context, param_map,
                Arc::clone(&self.reset_capture_flag), init_js)
        }
    }

    fn size(&self) -> (u32, u32) {
        self.scaled_size()
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        // Clamp the host-supplied DPI scale to a sane range so a misbehaving
        // host can't shrink the editor to zero pixels (which then makes the
        // webview layout glitch on resize).
        let clamped = factor.clamp(0.5, 4.0);
        *self.scale_factor.lock() = clamped;
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}
    fn param_values_changed(&self) {}
}

/// Extract raw handle value from ParentWindowHandle so we can send across threads.
fn extract_raw_handle(parent: &ParentWindowHandle) -> usize {
    match *parent {
        #[cfg(target_os = "linux")]
        ParentWindowHandle::X11Window(id) => id as usize,
        #[cfg(target_os = "macos")]
        ParentWindowHandle::AppKitNsView(ptr) => ptr as usize,
        #[cfg(target_os = "windows")]
        ParentWindowHandle::Win32Hwnd(h) => h as usize,
        _ => 0,
    }
}

// ─── Windows: TCP polling approach ─────────────────────────────────────────

/// WebView2 user-data folder, per plug-in *instance*. Two instances of LoudLab
/// in the same DAW project must not share the same UserDataFolder — that's the
/// documented WebView2 anti-pattern that produces the FL Studio 50/50 crash.
#[cfg(target_os = "windows")]
fn webview2_data_dir(instance_id: &str) -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("hardwave")
        .join("loudlab-webview2")
        .join(instance_id)
}

#[cfg(target_os = "windows")]
fn spawn_windows(
    raw_handle: usize,
    url: String,
    width: u32,
    height: u32,
    packet_rx: Arc<Mutex<Receiver<MasterPacket>>>,
    context: Arc<dyn GuiContext>,
    param_map: Arc<HashMap<String, nih_plug::prelude::ParamPtr>>,
    reset_capture_flag: Arc<std::sync::atomic::AtomicBool>,
    base_init_js: String,
    instance_id: String,
) -> Box<dyn std::any::Any + Send> {
    use std::io::{Read as IoRead, Write as IoWrite};
    use std::net::TcpListener;

    let shutdown = Arc::new(ShutdownSignal::new());
    let shutdown_for_handle = Arc::clone(&shutdown);

    // Bind a TCP listener for the JS poll bridge. Bind failures used to
    // panic across the FFI boundary (host crash). Now they degrade to a
    // working DSP path with a non-functional GUI.
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[HardwaveLoudLab] failed to bind TCP: {}", e);
            return Box::new(EditorHandle {
                shutdown: shutdown_for_handle,
                _webview: None,
                _web_context: None,
                _server_thread: None,
                _editor_thread: None,
            });
        }
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(e) => {
            eprintln!("[HardwaveLoudLab] failed to read local_addr: {}", e);
            return Box::new(EditorHandle {
                shutdown: shutdown_for_handle,
                _webview: None,
                _web_context: None,
                _server_thread: None,
                _editor_thread: None,
            });
        }
    };

    let latest_json = Arc::new(Mutex::new(String::from("{}")));
    let latest_json_server = Arc::clone(&latest_json);
    let shutdown_server = Arc::clone(&shutdown);

    // Server thread needs its own clones of context + param_map so the POST
    // /ipc handler can apply param changes from the webview. The wry IPC
    // bridge is unreliable in some WebView2 configurations (see CHANGELOG
    // v0.6.7) — the HTTP POST path is the canonical route now.
    let server_ctx = Arc::clone(&context);
    let server_pmap = Arc::clone(&param_map);
    let server_reset_flag = Arc::clone(&reset_capture_flag);

    let server_thread = std::thread::spawn(move || {
        listener.set_nonblocking(true).ok();
        while !shutdown_server.is_shutdown() {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = std::str::from_utf8(&buf[..n]).unwrap_or("");

                if req.starts_with("POST /ipc") {
                    // Locate the body — everything past the first blank line.
                    let body = req
                        .find("\r\n\r\n")
                        .map(|i| &req[i + 4..])
                        .unwrap_or("");
                    // Body may include trailing NULs from the uninitialised
                    // buffer tail; trim them so the JSON parse doesn't reject.
                    let trimmed = body.trim_end_matches('\0').trim();
                    if !trimmed.is_empty() {
                        handle_ipc(&server_ctx, &server_pmap, &server_reset_flag, trimmed);
                    }
                    let response = "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 2\r\n\r\n{}";
                    let _ = stream.write_all(response.as_bytes());
                } else if req.starts_with("OPTIONS") {
                    // CORS preflight — answer permissively.
                    let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\n\r\n";
                    let _ = stream.write_all(response.as_bytes());
                } else {
                    // Default: GET — return the most recent packet JSON.
                    let body = latest_json_server.lock().clone();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
            }
            // Drain pending packets into the latest-json slot.
            if let Some(rx) = packet_rx.try_lock() {
                while let Ok(pkt) = rx.try_recv() {
                    if let Ok(json) = serde_json::to_string(&pkt) {
                        *latest_json.lock() = json;
                    }
                }
            }
            // Cooperative wait — wakes immediately when Drop signals shutdown
            // so editor close takes <1ms instead of the previous 8ms cap.
            if shutdown_server.wait(Duration::from_millis(8)) {
                break;
            }
        }
    });

    // JS poll script + port export. window.__loudlab_packet_port is read by
    // the webview's sendParam helper so it can POST to /ipc instead of
    // depending on the wry IPC bridge.
    let poll_script = format!(
        r#"
(function() {{
    var _port = {port};
    window.__loudlab_packet_port = _port;
    function poll() {{
        fetch('http://127.0.0.1:' + _port)
            .then(function(r) {{ return r.json(); }})
            .then(function(data) {{
                if (window.__onMasterPacket) window.__onMasterPacket(data);
            }})
            .catch(function() {{}});
        setTimeout(poll, 16);
    }}
    poll();
}})();
"#,
    );

    let init_js = format!("{}\n{}", base_init_js, poll_script);
    let ctx = Arc::clone(&context);
    let pmap = Arc::clone(&param_map);
    let ipc_reset_flag = Arc::clone(&reset_capture_flag);

    let data_dir = webview2_data_dir(&instance_id);
    let _ = std::fs::create_dir_all(&data_dir);
    let mut web_context = wry::WebContext::new(Some(data_dir));

    let wrapper = RwhWrapper(raw_handle);

    use wry::WebViewBuilderExtWindows;
    let webview = wry::WebViewBuilder::with_web_context(&mut web_context)
        .with_url(&url)
        .with_initialization_script(&init_js)
        .with_ipc_handler(move |msg| {
            handle_ipc(&ctx, &pmap, &ipc_reset_flag, &msg.body());
        })
        .with_bounds(wry::Rect {
            position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
            size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(width as f64, height as f64)),
        })
        .with_transparent(false)
        .with_devtools(false)
        // Disable WebView2 browser accelerator keys (Ctrl+P / Ctrl+S / Ctrl+R /
        // F5 / F12 / Ctrl+Shift+I) at the OS level. Belt-and-braces with the
        // JS keydown blocker — works even before scripts attach.
        .with_browser_accelerator_keys(false)
        .with_background_color((10, 10, 11, 255))
        .build(&wrapper)
        .ok();

    Box::new(EditorHandle {
        shutdown: shutdown_for_handle,
        _webview: webview,
        _web_context: Some(web_context),
        _server_thread: Some(server_thread),
        _editor_thread: None,
    })
}

// ─── Linux / macOS: evaluate_script approach ───────────────────────────────

#[cfg(not(target_os = "windows"))]
fn spawn_unix(
    raw_handle: usize,
    url: String,
    width: u32,
    height: u32,
    packet_rx: Arc<Mutex<Receiver<MasterPacket>>>,
    context: Arc<dyn GuiContext>,
    param_map: Arc<HashMap<String, nih_plug::prelude::ParamPtr>>,
    reset_capture_flag: Arc<std::sync::atomic::AtomicBool>,
    init_js: String,
) -> Box<dyn std::any::Any + Send> {
    let shutdown = Arc::new(ShutdownSignal::new());
    let shutdown_for_handle = Arc::clone(&shutdown);
    let shutdown_thread = Arc::clone(&shutdown);

    let editor_thread = std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            let _ = gtk::init();
        }

        let wrapper = RwhWrapper(raw_handle);
        let ctx = Arc::clone(&context);
        let pmap = Arc::clone(&param_map);
        let ipc_reset_flag = Arc::clone(&reset_capture_flag);

        let webview = match wry::WebViewBuilder::new()
            .with_url(&url)
            .with_initialization_script(&init_js)
            .with_ipc_handler(move |msg| {
                handle_ipc(&ctx, &pmap, &ipc_reset_flag, &msg.body());
            })
            .with_bounds(wry::Rect {
                position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
                size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(width as f64, height as f64)),
            })
            .with_devtools(false)
            .build_as_child(&wrapper)
        {
            Ok(wv) => wv,
            Err(e) => {
                eprintln!("[HardwaveLoudLab] failed to create WebView: {}", e);
                return;
            }
        };

        while !shutdown_thread.is_shutdown() {
            if let Some(rx) = packet_rx.try_lock() {
                while let Ok(pkt) = rx.try_recv() {
                    if let Ok(json) = serde_json::to_string(&pkt) {
                        let js = format!(
                            "window.__onMasterPacket && window.__onMasterPacket({})",
                            json
                        );
                        let _ = webview.evaluate_script(&js);
                    }
                }
            }

            #[cfg(target_os = "linux")]
            {
                while gtk::events_pending() {
                    gtk::main_iteration_do(false);
                }
            }

            // Cooperative wait — wakes immediately on Drop signal.
            if shutdown_thread.wait(Duration::from_millis(16)) {
                break;
            }
        }
    });

    Box::new(EditorHandle {
        shutdown: shutdown_for_handle,
        _webview: None,
        _web_context: None,
        _server_thread: None,
        _editor_thread: Some(editor_thread),
    })
}

// ─── Editor handle (dropped when DAW closes editor) ───────────────────────

struct EditorHandle {
    shutdown: Arc<ShutdownSignal>,
    _webview: Option<wry::WebView>,
    _web_context: Option<wry::WebContext>,
    _server_thread: Option<std::thread::JoinHandle<()>>,
    _editor_thread: Option<std::thread::JoinHandle<()>>,
}

unsafe impl Send for EditorHandle {}

impl Drop for EditorHandle {
    fn drop(&mut self) {
        // Signal shutdown first so worker threads can break out of their
        // wait() immediately. Then explicitly join — JoinHandle::drop
        // detaches, which leaks threads past Drop and lets them keep
        // touching shared state while we tear the WebView down. That's
        // the F3 race the audit flagged. Joining bounds Drop at <1ms on
        // a healthy machine.
        self.shutdown.signal();
        if let Some(h) = self._server_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self._editor_thread.take() {
            let _ = h.join();
        }
    }
}
