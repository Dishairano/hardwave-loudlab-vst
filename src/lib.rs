//! Hardwave LoudLab — AI-assisted mastering VST3/CLAP plugin.
//!
//! Signal chain:
//!   Input Gain → EQ (4-band parametric) → Multiband Compressor (4 bands)
//!   → Stereo Processor (width + mono bass) → Brickwall Limiter → Output Gain
//!
//! When Auto mode is enabled, the AI engine analyses the spectrum and adjusts
//! EQ, compressor, stereo, and limiter settings toward the selected genre target.

use crossbeam_channel::{Sender, Receiver};
use nih_plug::prelude::*;
use parking_lot::Mutex;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod auth;
mod auto;
mod dsp;
mod editor;
mod params;
mod profiles;
mod protocol;

// ─── Crash handler ───────────────────────────────────────────────────────────
//
// Mirrors the Analyser's pattern. A panic crossing the FFI boundary into a VST
// host is undefined behaviour and almost always crashes the host process.
// Installing a panic hook means any panic we miss in the editor / IPC paths
// gets logged to %APPDATA%\hardwave\loudlab-crash.log instead of being lost.

fn hardwave_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("hardwave")
}

fn crash_log_path() -> std::path::PathBuf {
    hardwave_data_dir().join("loudlab-crash.log")
}

fn crash_pending_path() -> std::path::PathBuf {
    hardwave_data_dir().join("loudlab-crash-pending")
}

mod crash_reporter;

fn install_crash_handler() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            use std::io::Write;
            let path = crash_log_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let ts = unix_timestamp();
                let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
                    (*s).to_string()
                } else if let Some(s) = info.payload().downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                let location = info
                    .location()
                    .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                    .unwrap_or_else(|| "unknown location".to_string());
                let bt = std::backtrace::Backtrace::force_capture();
                // Stash for the telemetry hook (runs after this one) so it
                // doesn't capture a second backtrace on the panicking thread.
                if let Ok(mut g) = crash_reporter::LAST_BACKTRACE.lock() {
                    *g = Some(bt.to_string());
                }

                let _ = writeln!(f, "========================================");
                let _ = writeln!(f, "HARDWAVE LOUDLAB CRASH REPORT");
                let _ = writeln!(f, "Time:     {}", ts);
                let _ = writeln!(f, "Version:  {}", env!("CARGO_PKG_VERSION"));
                let _ = writeln!(f, "OS:       {}", std::env::consts::OS);
                let _ = writeln!(f, "Arch:     {}", std::env::consts::ARCH);
                let _ = writeln!(f, "Location: {}", location);
                let _ = writeln!(f, "Message:  {}", payload);
                let _ = writeln!(f, "");
                let _ = writeln!(f, "Backtrace:");
                let _ = writeln!(f, "{}", bt);
                let _ = writeln!(f, "========================================");
                let _ = writeln!(f);
            }
            let pending = crash_pending_path();
            if let Some(parent) = pending.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let crash_ts = unix_timestamp();
            let _ = std::fs::write(
                &pending,
                format!("loudlab\n{}\n{}", env!("CARGO_PKG_VERSION"), crash_ts),
            );
            prev(info);
        }));
    });
}

fn unix_timestamp() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{} (unix)", dur.as_secs())
}

use auto::AutoEngine;
use dsp::eq::EqBandParams;
use dsp::compressor::BandCompParams;
use dsp::{
    BrickwallLimiter, LufsMeter, MultibandCompressor, ParametricEq, SpectrumAnalyzer,
    StereoMeter, StereoProcessor,
};
use params::{Genre, HardwaveMasterParams};
use profiles::GenreProfile;
use protocol::MasterPacket;

struct HardwaveLoudLab {
    params: Arc<HardwaveMasterParams>,

    // DSP modules — EQ is applied independently per channel.
    eq_l: ParametricEq,
    eq_r: ParametricEq,
    compressor: MultibandCompressor,
    saturation: dsp::Saturation,
    stereo: StereoProcessor,
    limiter: BrickwallLimiter,

    // Metering.
    analyzer: SpectrumAnalyzer,
    input_meter: LufsMeter,
    output_meter: LufsMeter,
    // Stereo meter operates on the post-mix output bus so the user sees the
    // correlation / mid-side balance of what they are about to render. A
    // 3-second sliding window matches the LUFS short-term timing.
    output_stereo_meter: StereoMeter,

    // Auto engine.
    auto_engine: AutoEngine,

    // Editor communication.
    editor_packet_tx: Sender<MasterPacket>,
    editor_packet_rx: Arc<Mutex<Receiver<MasterPacket>>>,
    update_counter: u32,

    // State for throttled auto-compute (every N samples).
    samples_since_auto: usize,
    current_profile: GenreProfile,

    sample_rate: f32,

    /// Maximum playback position the host has reported during this DAW
    /// session, in samples. Drives the webview's "Track loaded · MM:SS"
    /// header — pragmatic stand-in for project length without a Transport
    /// loop range.
    max_pos_samples: i64,

    /// Last reported transport.playing state. Emitted in the packet so the
    /// Step 1 "Listening" indicator turns green when the user presses play
    /// and grey when they stop.
    last_is_playing: bool,

    /// Peak momentary LUFS observed since the last `reset_capture`. The
    /// Step 1 "LUFS-M (drop peak)" stat tracks this — when the limiter is
    /// going to work hardest is the loudest 400 ms window the meter has
    /// seen.
    lufs_max_momentary: f32,

    /// Reset-capture flag — set to `true` by the editor's IPC handler when
    /// the user clicks "Reset capture" in Step 1, drained by `process()`
    /// at the top of the next block. Using an atomic avoids needing a
    /// realtime channel for a single-bit signal.
    reset_capture_flag: Arc<AtomicBool>,
}

impl Default for HardwaveLoudLab {
    fn default() -> Self {
        // Install the panic hook before anything else can fault. Idempotent
        // via std::sync::Once, so it's safe to call from every plug-in
        // instance the host creates.
        install_crash_handler();
        crash_reporter::install("loudlab");

        let sr = 44100.0;
        let (pkt_tx, pkt_rx) = crossbeam_channel::bounded(4);
        Self {
            params: Arc::new(HardwaveMasterParams::default()),
            eq_l: ParametricEq::new(sr),
            eq_r: ParametricEq::new(sr),
            compressor: MultibandCompressor::new(sr),
            saturation: dsp::Saturation::new(sr),
            stereo: StereoProcessor::new(sr),
            limiter: BrickwallLimiter::new(sr),
            analyzer: SpectrumAnalyzer::new(sr),
            input_meter: LufsMeter::new(sr),
            output_meter: LufsMeter::new(sr),
            output_stereo_meter: StereoMeter::new(sr),
            auto_engine: AutoEngine::new(),
            editor_packet_tx: pkt_tx,
            editor_packet_rx: Arc::new(Mutex::new(pkt_rx)),
            update_counter: 0,
            samples_since_auto: 0,
            current_profile: GenreProfile::for_genre(Genre::Hardstyle),
            sample_rate: sr,
            max_pos_samples: 0,
            last_is_playing: false,
            lufs_max_momentary: -120.0,
            reset_capture_flag: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Plugin for HardwaveLoudLab {
    const NAME: &'static str = "Hardwave LoudLab";
    const VENDOR: &'static str = "Hardwave Studios";
    const URL: &'static str = "https://hardwavestudios.com";
    const EMAIL: &'static str = "hello@hardwavestudios.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let token = auth::load_token();
        Some(Box::new(editor::MasterEditor::new(
            Arc::clone(&self.params),
            Arc::clone(&self.editor_packet_rx),
            Arc::clone(&self.reset_capture_flag),
            token,
        )))
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        let sr = buffer_config.sample_rate;
        self.sample_rate = sr;

        // Report plugin delay for host PDC: the limiter's lookahead buffer
        // plus the oversampler's linear-phase group delay.
        let latency = (dsp::limiter::LOOKAHEAD_MS * 0.001 * sr) as u32
            + dsp::oversample::LATENCY_SAMPLES as u32;
        context.set_latency_samples(latency);

        self.eq_l.set_sample_rate(sr);
        self.eq_r.set_sample_rate(sr);
        self.compressor.set_sample_rate(sr);
        self.saturation.set_sample_rate(sr);
        self.stereo.set_sample_rate(sr);
        self.limiter.set_sample_rate(sr);
        self.analyzer.set_sample_rate(sr);
        self.input_meter.set_sample_rate(sr);
        self.output_meter.set_sample_rate(sr);
        self.output_stereo_meter.set_sample_rate(sr);

        true
    }

    fn reset(&mut self) {
        self.eq_l.reset();
        self.eq_r.reset();
        self.compressor.reset();
        self.saturation.reset();
        self.stereo.reset();
        self.limiter.reset();
        self.analyzer.reset();
        self.input_meter.reset();
        self.output_meter.reset();
        self.output_stereo_meter.reset();
        self.auto_engine.reset();
        self.samples_since_auto = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Read ALL param values into locals so we can drop the borrow on self.params.
        let p = &self.params;
        let intensity = p.intensity.value();
        let input_gain_db = p.input_gain.value();
        let output_gain_db = p.output_gain.value();
        let mix = p.mix.value();
        let auto_mode = p.auto_mode.value();
        let master_enabled = p.master_enabled.value();
        let eq_enabled = p.eq_enabled.value();
        let comp_enabled = p.comp_enabled.value();
        let stereo_enabled = p.stereo_enabled.value();
        let limiter_enabled = p.limiter_enabled.value();
        let sat_enabled = p.sat_enabled.value();
        let sat_drive_db = p.sat_drive.value();
        let sat_mix = p.sat_mix.value();
        let genre = p.genre.value();

        // Track the maximum playback position ever observed so the webview
        // header can show "Track loaded · MM:SS". The host's transport gives
        // us per-block position; the maximum across blocks is the closest
        // pragmatic stand-in for "track length" without a Transport loop range
        // (some DAWs don't expose project length at all).
        let transport = context.transport();
        if let Some(pos) = transport.pos_samples() {
            if pos > self.max_pos_samples {
                self.max_pos_samples = pos;
            }
        }
        self.last_is_playing = transport.playing;

        // Drain the Step 1 "Reset capture" signal from the editor IPC thread.
        // When set, blow away the integrated/short-term LUFS history, the max
        // observed position, and the max momentary so the next capture starts
        // from a clean slate. Acquire pairs with the Release store in
        // handle_ipc to give us a happens-before edge across threads.
        if self.reset_capture_flag.swap(false, Ordering::Acquire) {
            self.input_meter.reset();
            self.output_meter.reset();
            self.output_stereo_meter.reset();
            self.max_pos_samples = 0;
            self.lufs_max_momentary = -120.0;
        }

        // Snapshot all param values for the editor packet.
        let pkt_snapshot = editor::snapshot_params(p);
        // Release the immutable borrow on self.params.
        let _ = p;

        let input_gain = db_to_linear(input_gain_db);
        let output_gain = db_to_linear(output_gain_db);

        // Update genre profile if needed.
        self.current_profile = GenreProfile::for_genre(genre);

        // If NOT auto mode, read manual EQ/comp/stereo/limiter params.
        if !auto_mode {
            self.apply_manual_params();
        }

        // Auto-tune every 2048 samples (~20x/sec at 44.1k).
        let auto_interval = 2048;

        for mut frame in buffer.iter_samples() {
            let num_channels = frame.len();
            if num_channels < 2 {
                continue;
            }

            let dry_l = *frame.get_mut(0).unwrap();
            let dry_r = *frame.get_mut(1).unwrap();

            // Master bypass — the webview's header On/Off toggle. When
            // disabled, the plugin is a passthrough: no gain, no DSP, no mix.
            // Meters still run on the dry signal so the right rail keeps
            // updating; otherwise toggling Off would freeze the LUFS readouts.
            if !master_enabled {
                self.input_meter.process(dry_l, dry_r);
                self.output_meter.process(dry_l, dry_r);
                self.output_stereo_meter.process(dry_l, dry_r);
                // Leaving frame[0]/[1] unwritten passes the dry signal through
                // unchanged — nih-plug iterates in place.
                continue;
            }

            // Input gain.
            let mut l = dry_l * input_gain;
            let mut r = dry_r * input_gain;

            // Input metering.
            self.input_meter.process(l, r);

            // Feed analyzer (mono sum).
            self.analyzer.push_sample((l + r) * 0.5);

            // Auto engine (throttled).
            if auto_mode {
                self.samples_since_auto += 1;
                if self.samples_since_auto >= auto_interval {
                    self.samples_since_auto = 0;
                    if self.analyzer.take_frame_ready() {
                        let result = self.auto_engine.compute(
                            self.analyzer.spectrum_ref(),
                            self.sample_rate,
                            &self.current_profile,
                            intensity,
                        );
                        // Apply auto results to DSP modules.
                        for i in 0..4 {
                            self.eq_l.set_band(i, result.eq_bands[i]);
                            self.eq_r.set_band(i, result.eq_bands[i]);
                            self.compressor.set_band_params(i, result.comp_bands[i]);
                        }
                        self.compressor.set_crossover_freqs(
                            result.comp_xover[0],
                            result.comp_xover[1],
                            result.comp_xover[2],
                        );
                        self.stereo.width = result.stereo_width;
                        self.stereo.mono_bass_freq = result.mono_bass_freq;
                        // The mono-bass on/off switch is user intent, not an
                        // auto-engine output — honor the param in Auto mode
                        // exactly like apply_manual_params does.
                        self.stereo.bass_mono = self.params.stereo_mono_bass.value();
                        self.stereo.update_filters();
                        self.limiter.set_ceiling(result.limiter_ceiling_db);
                    }
                }
            }

            // EQ.
            if eq_enabled {
                l = self.eq_l.process(l);
                r = self.eq_r.process(r);
            }

            // Multiband compressor.
            if comp_enabled {
                let (cl, cr) = self.compressor.process(l, r);
                l = cl;
                r = cr;
            }

            // Saturation — placed post-compressor so it sees a level-stable
            // input, and pre-stereo so width processing doesn't fight the
            // harmonic content the saturator just generated. Off by default;
            // when disabled, returns input unchanged in one branch.
            if sat_enabled {
                let (sl, sr_out) = self.saturation.process(l, r);
                l = sl;
                r = sr_out;
            }

            // Stereo processor.
            if stereo_enabled {
                let (sl, sr_out) = self.stereo.process(l, r);
                l = sl;
                r = sr_out;
            }

            // Limiter.
            if limiter_enabled {
                let (ll, lr) = self.limiter.process(l, r);
                l = ll;
                r = lr;
            }

            // Output gain.
            l *= output_gain;
            r *= output_gain;

            // Dry/wet mix.
            l = dry_l * (1.0 - mix) + l * mix;
            r = dry_r * (1.0 - mix) + r * mix;

            // Output metering.
            self.output_meter.process(l, r);
            self.output_stereo_meter.process(l, r);

            // Write output.
            *frame.get_mut(0).unwrap() = l;
            *frame.get_mut(1).unwrap() = r;
        }

        // Send state packet to editor (~60 fps at 44.1k / 512 block size).
        self.update_counter += 1;
        if self.update_counter >= 4 {
            self.update_counter = 0;

            let mut packet = pkt_snapshot;
            packet.input_lufs = self.input_meter.momentary_lufs();
            packet.output_lufs = self.output_meter.momentary_lufs();
            packet.lufs_short_term = self.output_meter.short_term_lufs();
            packet.lufs_integrated = self.output_meter.integrated_lufs();
            packet.true_peak_db = self.output_meter.true_peak();
            // Dynamic range as PLR — peak-to-loudness ratio against the 3 s
            // short-term measurement. Falls back to 0 (rather than negative)
            // before the short-term window has filled.
            let st = self.output_meter.short_term_lufs();
            packet.dynamic_range = if st > -119.0 {
                (packet.true_peak_db - st).max(0.0)
            } else {
                0.0
            };
            packet.correlation = self.output_stereo_meter.correlation();
            packet.ms_ratio = self.output_stereo_meter.mid_fraction();
            packet.sample_rate = self.sample_rate;
            packet.track_duration = if self.sample_rate > 0.0 {
                self.max_pos_samples as f32 / self.sample_rate
            } else {
                0.0
            };
            packet.is_playing = self.last_is_playing;
            // Update peak-momentary hold on the output bus. We refresh this in
            // the packet-emission block (~15 Hz) rather than per-sample — the
            // momentary LUFS itself is a 400 ms moving average, so checking
            // for new maxima at 60 ms intervals captures peaks just as well
            // and keeps the hot path branch-free.
            let momentary = self.output_meter.momentary_lufs();
            if momentary > self.lufs_max_momentary {
                self.lufs_max_momentary = momentary;
            }
            packet.lufs_max_momentary = self.lufs_max_momentary;
            // Snapshot, not consume: in Auto mode the auto engine takes the
            // fresh-frame flag first, which used to blank the UI spectrum.
            packet.spectrum = self.analyzer.spectrum_snapshot();

            let _ = self.editor_packet_tx.try_send(packet);
        }

        ProcessStatus::Normal
    }
}

impl HardwaveLoudLab {
    /// Apply manual (non-auto) parameter values to DSP modules.
    fn apply_manual_params(&mut self) {
        let p = &self.params;

        // EQ bands.
        let eq_bands = [
            EqBandParams {
                freq: p.eq_low_freq.value(),
                gain_db: p.eq_low_gain.value(),
                q: p.eq_low_q.value(),
                enabled: true,
            },
            EqBandParams {
                freq: p.eq_low_mid_freq.value(),
                gain_db: p.eq_low_mid_gain.value(),
                q: p.eq_low_mid_q.value(),
                enabled: true,
            },
            EqBandParams {
                freq: p.eq_high_mid_freq.value(),
                gain_db: p.eq_high_mid_gain.value(),
                q: p.eq_high_mid_q.value(),
                enabled: true,
            },
            EqBandParams {
                freq: p.eq_high_freq.value(),
                gain_db: p.eq_high_gain.value(),
                q: p.eq_high_q.value(),
                enabled: true,
            },
        ];

        for i in 0..4 {
            self.eq_l.set_band(i, eq_bands[i]);
            self.eq_r.set_band(i, eq_bands[i]);
        }

        // Compressor crossover.
        self.compressor.set_crossover_freqs(
            p.comp_xover_low.value(),
            p.comp_xover_mid.value(),
            p.comp_xover_high.value(),
        );

        // Compressor bands.
        let comp_params = [
            BandCompParams {
                threshold_db: p.comp_sub_thresh.value(),
                ratio: p.comp_sub_ratio.value(),
                attack_ms: p.comp_sub_attack.value(),
                release_ms: p.comp_sub_release.value(),
                makeup_db: 0.0,
            },
            BandCompParams {
                threshold_db: p.comp_lm_thresh.value(),
                ratio: p.comp_lm_ratio.value(),
                attack_ms: p.comp_lm_attack.value(),
                release_ms: p.comp_lm_release.value(),
                makeup_db: 0.0,
            },
            BandCompParams {
                threshold_db: p.comp_hm_thresh.value(),
                ratio: p.comp_hm_ratio.value(),
                attack_ms: p.comp_hm_attack.value(),
                release_ms: p.comp_hm_release.value(),
                makeup_db: 0.0,
            },
            BandCompParams {
                threshold_db: p.comp_hi_thresh.value(),
                ratio: p.comp_hi_ratio.value(),
                attack_ms: p.comp_hi_attack.value(),
                release_ms: p.comp_hi_release.value(),
                makeup_db: 0.0,
            },
        ];

        for i in 0..4 {
            self.compressor.set_band_params(i, comp_params[i]);
        }

        // Stereo.
        self.stereo.width = p.stereo_width.value();
        self.stereo.bass_mono = p.stereo_mono_bass.value();
        self.stereo.mono_bass_freq = p.stereo_mono_bass_freq.value();
        self.stereo.update_filters();

        // Saturation. set_params is allocation-free; Saturation copies the
        // struct in. Reading these in the slow-path keeps the audio loop free
        // of per-sample param.value() calls.
        self.saturation.set_params(dsp::SaturationParams {
            drive_db: p.sat_drive.value(),
            mix: p.sat_mix.value(),
            enabled: p.sat_enabled.value(),
        });

        // Per-band makeup gain. Plumbing the existing BandCompParams field
        // that has been dormant in compressor.rs — pulling the four params
        // straight into the compressor's per-band state. Allocation-free.
        // Note: ratios + thresholds + attack + release are kept where they
        // already are (auto-engine handles them when auto_mode=true; manual
        // changes via handle_ipc set them directly). Makeup gain has no
        // matching auto-engine path; it's manual-only, which is exactly the
        // Advanced-mode use case we're enabling.
        {
            let comp_makeups = [
                p.comp_sub_makeup.value(),
                p.comp_lm_makeup.value(),
                p.comp_hm_makeup.value(),
                p.comp_hi_makeup.value(),
            ];
            for (i, mk) in comp_makeups.iter().enumerate() {
                self.compressor.set_band_makeup(i, *mk);
            }
        }

        // Limiter.
        self.limiter.set_ceiling(p.limiter_ceiling.value());
    }
}

impl ClapPlugin for HardwaveLoudLab {
    const CLAP_ID: &'static str = "com.hardwavestudios.loudlab";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("AI-assisted mastering plugin with genre-aware auto-tuning");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = Some("https://hardwavestudios.com/support");
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Mastering,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for HardwaveLoudLab {
    const VST3_CLASS_ID: [u8; 16] = *b"HWLoudLab_v0001\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Mastering,
        Vst3SubCategory::Stereo,
    ];
}

nih_export_clap!(HardwaveLoudLab);
nih_export_vst3!(HardwaveLoudLab);

#[inline(always)]
fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
