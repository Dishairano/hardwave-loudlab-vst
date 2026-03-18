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
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::auth;
use crate::params::{Genre, HardwaveMasterParams};
use crate::protocol::MasterPacket;

const LOUDLAB_URL: &str = "https://loudlab.hardwavestudios.com";
const EDITOR_WIDTH: u32 = 1100;
const EDITOR_HEIGHT: u32 = 700;

/// Wraps a raw window handle value (usize) so wry can use it via rwh 0.6.
struct RwhWrapper(usize);

unsafe impl Send for RwhWrapper {}
unsafe impl Sync for RwhWrapper {}

impl raw_window_handle::HasWindowHandle for RwhWrapper {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::RawWindowHandle;

        #[cfg(target_os = "linux")]
        let raw = {
            let h = raw_window_handle::XlibWindowHandle::new(self.0 as _);
            RawWindowHandle::Xlib(h)
        };

        #[cfg(target_os = "macos")]
        let raw = {
            let ns_view = std::ptr::NonNull::new(self.0 as *mut _).expect("null NSView");
            let h = raw_window_handle::AppKitWindowHandle::new(ns_view);
            RawWindowHandle::AppKit(h)
        };

        #[cfg(target_os = "windows")]
        let raw = {
            let hwnd = std::num::NonZeroIsize::new(self.0 as isize).expect("null HWND");
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

        input_lufs: -120.0,
        output_lufs: -120.0,
        true_peak_db: -120.0,
        spectrum: None,
    }
}

/// Build the init JavaScript that gets injected into the webview on load.
fn ipc_init_script(params: &HardwaveMasterParams) -> String {
    let snapshot = snapshot_params(params);
    let initial_json = serde_json::to_string(&snapshot).unwrap_or_else(|_| "null".into());
    let version = env!("CARGO_PKG_VERSION");

    format!(
        r#"
window.__HARDWAVE_VST = true;
window.__HARDWAVE_VST_VERSION = '{version}';
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
                unsafe {
                    context.raw_begin_set_parameter(*ptr);
                    context.raw_set_parameter_normalized(*ptr, value as f32);
                    context.raw_end_set_parameter(*ptr);
                }
            }
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
    auth_token: Option<String>,
    scale_factor: Mutex<f32>,
}

impl MasterEditor {
    pub fn new(
        params: Arc<HardwaveMasterParams>,
        packet_rx: Arc<Mutex<Receiver<MasterPacket>>>,
        auth_token: Option<String>,
    ) -> Self {
        Self {
            params,
            packet_rx,
            auth_token,
            scale_factor: Mutex::new(1.0),
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
            spawn_windows(raw_handle, url, width, height, packet_rx, context, param_map, init_js)
        }

        #[cfg(not(target_os = "windows"))]
        {
            spawn_unix(raw_handle, url, width, height, packet_rx, context, param_map, init_js)
        }
    }

    fn size(&self) -> (u32, u32) {
        self.scaled_size()
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        *self.scale_factor.lock() = factor;
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

#[cfg(target_os = "windows")]
fn webview2_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("hardwave")
        .join("loudlab-webview2")
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
    base_init_js: String,
) -> Box<dyn std::any::Any + Send> {
    use std::io::{Read as IoRead, Write as IoWrite};
    use std::net::TcpListener;

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    // Start local TCP server for polling.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind TCP");
    let port = listener.local_addr().unwrap().port();
    let latest_json = Arc::new(Mutex::new(String::from("{}")));
    let latest_json_server = Arc::clone(&latest_json);
    let running_server = Arc::clone(&running);

    let server_thread = std::thread::spawn(move || {
        listener.set_nonblocking(true).ok();
        while running_server.load(Ordering::Relaxed) {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = latest_json_server.lock().clone();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
            // Feed latest packet.
            if let Some(rx) = packet_rx.try_lock() {
                while let Ok(pkt) = rx.try_recv() {
                    if let Ok(json) = serde_json::to_string(&pkt) {
                        *latest_json.lock() = json;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    });

    // JS poll script.
    let poll_script = format!(
        r#"
(function() {{
    var _port = {port};
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

    let data_dir = webview2_data_dir();
    let _ = std::fs::create_dir_all(&data_dir);
    let mut web_context = wry::WebContext::new(Some(data_dir));

    let wrapper = RwhWrapper(raw_handle);

    use wry::WebViewBuilderExtWindows;
    let webview = wry::WebViewBuilder::with_web_context(&mut web_context)
        .with_url(&url)
        .with_initialization_script(&init_js)
        .with_ipc_handler(move |msg| {
            handle_ipc(&ctx, &pmap, &msg.body());
        })
        .with_bounds(wry::Rect {
            position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
            size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(width as f64, height as f64)),
        })
        .with_transparent(false)
        .with_devtools(false)
        .with_background_color((10, 10, 11, 255))
        .build(&wrapper)
        .ok();

    Box::new(EditorHandle {
        running: running_clone,
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
    init_js: String,
) -> Box<dyn std::any::Any + Send> {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    let editor_thread = std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            let _ = gtk::init();
        }

        let wrapper = RwhWrapper(raw_handle);
        let ctx = Arc::clone(&context);
        let pmap = Arc::clone(&param_map);

        let webview = match wry::WebViewBuilder::new()
            .with_url(&url)
            .with_initialization_script(&init_js)
            .with_ipc_handler(move |msg| {
                handle_ipc(&ctx, &pmap, &msg.body());
            })
            .with_bounds(wry::Rect {
                position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
                size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(width as f64, height as f64)),
            })
            .build_as_child(&wrapper)
        {
            Ok(wv) => wv,
            Err(e) => {
                eprintln!("[HardwaveLoudLab] failed to create WebView: {}", e);
                return;
            }
        };

        while running.load(Ordering::Relaxed) {
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

            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });

    Box::new(EditorHandle {
        running: running_clone,
        _webview: None,
        _web_context: None,
        _server_thread: None,
        _editor_thread: Some(editor_thread),
    })
}

// ─── Editor handle (dropped when DAW closes editor) ───────────────────────

struct EditorHandle {
    running: Arc<AtomicBool>,
    _webview: Option<wry::WebView>,
    _web_context: Option<wry::WebContext>,
    _server_thread: Option<std::thread::JoinHandle<()>>,
    _editor_thread: Option<std::thread::JoinHandle<()>>,
}

unsafe impl Send for EditorHandle {}

impl Drop for EditorHandle {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
