//! Rust → JS packet for the webview UI.

use serde::{Deserialize, Serialize};

/// Full state packet pushed to the webview at ~60 fps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterPacket {
    // ── Global ───────────────────────────────────────────────────────────────
    pub genre: String,
    pub intensity: f32,
    pub input_gain: f32,
    pub output_gain: f32,
    pub mix: f32,
    pub auto_mode: bool,

    // ── EQ ───────────────────────────────────────────────────────────────────
    pub eq_enabled: bool,
    pub eq_low_freq: f32,
    pub eq_low_gain: f32,
    pub eq_low_q: f32,
    pub eq_low_mid_freq: f32,
    pub eq_low_mid_gain: f32,
    pub eq_low_mid_q: f32,
    pub eq_high_mid_freq: f32,
    pub eq_high_mid_gain: f32,
    pub eq_high_mid_q: f32,
    pub eq_high_freq: f32,
    pub eq_high_gain: f32,
    pub eq_high_q: f32,

    // ── Compressor ───────────────────────────────────────────────────────────
    pub comp_enabled: bool,
    pub comp_xover_low: f32,
    pub comp_xover_mid: f32,
    pub comp_xover_high: f32,

    pub comp_sub_thresh: f32,
    pub comp_sub_ratio: f32,
    pub comp_sub_attack: f32,
    pub comp_sub_release: f32,

    pub comp_lm_thresh: f32,
    pub comp_lm_ratio: f32,
    pub comp_lm_attack: f32,
    pub comp_lm_release: f32,

    pub comp_hm_thresh: f32,
    pub comp_hm_ratio: f32,
    pub comp_hm_attack: f32,
    pub comp_hm_release: f32,

    pub comp_hi_thresh: f32,
    pub comp_hi_ratio: f32,
    pub comp_hi_attack: f32,
    pub comp_hi_release: f32,

    // ── Stereo ───────────────────────────────────────────────────────────────
    pub stereo_enabled: bool,
    pub stereo_width: f32,
    pub stereo_mono_bass: bool,
    pub stereo_mono_bass_freq: f32,

    // ── Limiter ──────────────────────────────────────────────────────────────
    pub limiter_enabled: bool,
    pub limiter_ceiling: f32,

    // ── Metering (read-only, pushed from DSP) ────────────────────────────────
    /// Momentary LUFS (400 ms K-weighted) of the input bus.
    pub input_lufs: f32,
    /// Momentary LUFS (400 ms K-weighted) of the output bus.
    pub output_lufs: f32,
    /// Short-term LUFS (3 s K-weighted) of the output bus. The "LUFS-S"
    /// meter in the right rail of the webview reads this directly.
    pub lufs_short_term: f32,
    /// BS.1770-4 integrated LUFS (gated) of the output bus since the last
    /// transport reset. The "LUFS-I" meter and the diagnostic readouts
    /// reference this.
    pub lufs_integrated: f32,
    /// True peak (dBTP) of the output bus — 4× oversampled estimate.
    pub true_peak_db: f32,
    /// Dynamic range (dB) as Peak-to-Loudness Ratio: max(0, true_peak_db
    /// − lufs_short_term). A higher number means more dynamic.
    pub dynamic_range: f32,
    /// L/R Pearson correlation over a 3 s window, in [-1.0, +1.0].
    /// +1.0 is mono, 0 is uncorrelated, -1.0 indicates phase inversion.
    pub correlation: f32,
    /// Mid-channel energy fraction in [0.0, 1.0]. A value of 0.64 means
    /// the signal is 64% mid / 36% side over the trailing 3 s window.
    pub ms_ratio: f32,
    /// Engine sample rate, in Hz (e.g. 44100.0). The webview header shows
    /// "@ 44.1 kHz" from this.
    pub sample_rate: f32,
    /// Furthest playback position the host has reported during this DAW
    /// session, in seconds. Drives the webview header's "Track loaded · MM:SS"
    /// readout. 0.0 until the host starts reporting transport position.
    pub track_duration: f32,
    /// Spectrum magnitudes (dB), 1024 bins, optional (sent every few frames).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spectrum: Option<Vec<f32>>,
}

/// JS → Rust messages from the webview.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum UiMessage {
    #[serde(rename = "set_param")]
    SetParam { id: String, value: f64 },
    #[serde(rename = "set_genre")]
    SetGenre { genre: String },
    #[serde(rename = "toggle_auto")]
    ToggleAuto { enabled: bool },
}
