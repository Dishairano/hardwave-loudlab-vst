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
    pub input_lufs: f32,
    pub output_lufs: f32,
    pub true_peak_db: f32,
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
