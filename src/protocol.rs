//! Rust → JS packet for the webview UI.

use serde::{Deserialize, Serialize};

/// Full state packet pushed to the webview at ~60 fps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterPacket {
    // ── Global ───────────────────────────────────────────────────────────────
    pub genre: String,
    pub intensity: f32,
    pub input_gain: f32,
    pub sub_gain: f32,
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

    // ── Saturation (Advanced mode) ───────────────────────────────────────────
    pub sat_enabled: bool,
    pub sat_drive: f32,
    pub sat_mix: f32,

    // ── Per-band makeup gain (Advanced MBC) ──────────────────────────────────
    pub comp_sub_makeup: f32,
    pub comp_lm_makeup: f32,
    pub comp_hm_makeup: f32,
    pub comp_hi_makeup: f32,

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
    /// Whether the host transport is currently playing. Drives the Step 1
    /// "Listening" indicator in the webview — green when true, grey when the
    /// user has paused. False until the host first signals play.
    pub is_playing: bool,
    /// Peak momentary LUFS seen since the last `reset_capture`. The Step 1
    /// "LUFS-M (drop peak)" stat reads this — it's the loudest 400 ms window
    /// observed so far, the spot where the limiter will work hardest.
    pub lufs_max_momentary: f32,
    /// Spectrum magnitudes (dB), 1024 bins, optional (sent every few frames).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spectrum: Option<Vec<f32>>,
}

/// JS -> Rust messages from the webview.
///
/// This is the whole protocol and `editor.rs` deserializes into it, so a
/// message the webview sends that is not listed here fails to parse instead of
/// being dropped in a catch-all arm. It had drifted badly while nothing used
/// it: `set_genre` and `toggle_auto` do not exist on either side, and
/// `reset_capture`, `save_token` and `clear_token`, which the handler has
/// always supported, were missing.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum UiMessage {
    #[serde(rename = "set_param")]
    SetParam { id: String, value: f64 },

    #[serde(rename = "reset_capture")]
    ResetCapture,

    #[serde(rename = "save_token")]
    SaveToken { token: String },

    #[serde(rename = "clear_token")]
    ClearToken,

    /// The UI's resize grip sends this on every drag. LoudLab's editor is a
    /// fixed 1100x700 (`EDITOR_WIDTH`/`EDITOR_HEIGHT`) with no resize plumbing,
    /// so the grip currently does nothing in a host. Listed here because the
    /// message is real and arriving; handling it needs the editor to carry a
    /// size and a resize channel, the way PumpControl and WideBoi do.
    #[serde(rename = "resize")]
    // The sizes are carried but not read yet, for the reason above.
    #[allow(dead_code)]
    Resize { width: u32, height: u32 },
}

#[cfg(test)]
mod ui_message_tests {
    use super::UiMessage;

    /// The payloads the shipped LoudLab webview sends, taken from
    /// apps/loudlab in vst-webviews. A rename on either side fails here.
    #[test]
    fn parses_every_message_the_ui_sends() {
        let cases = [
            r#"{"type":"set_param","id":"eq_low_gain","value":-1.4}"#,
            r#"{"type":"reset_capture"}"#,
            r#"{"type":"save_token","token":"abc"}"#,
            r#"{"type":"clear_token"}"#,
            r#"{"type":"resize","width":1400,"height":890}"#,
        ];
        for raw in cases {
            serde_json::from_str::<UiMessage>(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
        }
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        assert!(serde_json::from_str::<UiMessage>(r#"{"type":"set_genre","genre":"hardstyle"}"#).is_err());
        assert!(serde_json::from_str::<UiMessage>(r#"{"type":"set_param","id":"x"}"#).is_err());
    }
}
