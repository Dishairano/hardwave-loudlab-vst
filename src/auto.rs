//! AI auto-tuning engine.
//!
//! When auto mode is active, this module:
//! 1. Reads the current spectrum from the analyzer.
//! 2. Compares it against the genre target curve.
//! 3. Computes corrective EQ gain adjustments to push the spectrum toward
//!    the target.
//! 4. Applies genre-specific compressor, stereo, and limiter settings.
//!
//! All adjustments are scaled by the `intensity` knob (0..1).

use crate::dsp::compressor::BandCompParams;
use crate::dsp::eq::EqBandParams;
use crate::profiles::GenreProfile;

/// Anchor frequencies for spectral matching (Hz).
/// These correspond roughly to the 4 EQ band center regions.
const ANCHOR_FREQS: [f32; 4] = [80.0, 500.0, 3000.0, 10000.0];

/// Maximum auto-EQ correction in dB (per band).
const MAX_AUTO_CORRECTION_DB: f32 = 6.0;

/// Smoothing factor for auto-EQ changes (exponential moving average).
/// Lower = smoother/slower response. 0.05 ≈ roughly 1-second convergence at 60 fps.
const SMOOTH_ALPHA: f32 = 0.05;

pub struct AutoEngine {
    /// Current smoothed EQ corrections (dB per band).
    smooth_eq: [f32; 4],
}

impl AutoEngine {
    pub fn new() -> Self {
        Self {
            smooth_eq: [0.0; 4],
        }
    }

    pub fn reset(&mut self) {
        self.smooth_eq = [0.0; 4];
    }

    /// Compute auto-tuned parameters given the current spectrum and target profile.
    ///
    /// `spectrum_db`: FFT magnitude spectrum (1024 bins).
    /// `sample_rate`: current sample rate for bin→freq conversion.
    /// `profile`: genre target profile.
    /// `intensity`: 0.0..1.0 master intensity knob.
    ///
    /// Returns the adjusted EQ bands, comp params, stereo width, mono-bass freq,
    /// and limiter ceiling.
    pub fn compute(
        &mut self,
        spectrum_db: &[f32],
        sample_rate: f32,
        profile: &GenreProfile,
        intensity: f32,
    ) -> AutoResult {
        let bin_count = spectrum_db.len();
        let fft_size = bin_count * 2;
        let bin_hz = sample_rate / fft_size as f32;

        // Measure average energy around each anchor frequency.
        let measured = ANCHOR_FREQS.map(|freq| {
            avg_energy_around(spectrum_db, freq, bin_hz, bin_count)
        });

        // Compute the target energy at each anchor from the profile's EQ gains.
        // The profile's gain_db represents the desired deviation from flat, so
        // we simply target (measured_flat + profile_gain). Since we don't know
        // the true flat reference we use the overall average level.
        let overall_avg: f32 = measured.iter().sum::<f32>() / 4.0;

        let mut eq_bands = profile.eq_bands;

        for i in 0..4 {
            // Target level at this anchor = overall average + profile offset.
            let target = overall_avg + profile.eq_bands[i].gain_db;
            let error = target - measured[i];

            // Clamp correction.
            let correction = error.clamp(-MAX_AUTO_CORRECTION_DB, MAX_AUTO_CORRECTION_DB);

            // Smooth the correction.
            self.smooth_eq[i] += SMOOTH_ALPHA * (correction - self.smooth_eq[i]);

            // Apply intensity scaling.
            eq_bands[i].gain_db = self.smooth_eq[i] * intensity;
            eq_bands[i].freq = profile.eq_bands[i].freq;
            eq_bands[i].q = profile.eq_bands[i].q;
        }

        // Comp: interpolate between default (gentle) and profile settings based on intensity.
        let comp_bands = profile.comp_bands.map(|target| {
            let default = BandCompParams::default();
            BandCompParams {
                threshold_db: lerp(default.threshold_db, target.threshold_db, intensity),
                ratio: lerp(default.ratio, target.ratio, intensity),
                attack_ms: lerp(default.attack_ms, target.attack_ms, intensity),
                release_ms: lerp(default.release_ms, target.release_ms, intensity),
                makeup_db: target.makeup_db * intensity,
            }
        });

        // Stereo: interpolate width from 1.0 (neutral) toward profile target.
        let stereo_width = lerp(1.0, profile.stereo_width, intensity);
        let mono_bass_freq = profile.mono_bass_freq;
        let limiter_ceiling_db = profile.limiter_ceiling_db;

        AutoResult {
            eq_bands,
            comp_bands,
            comp_xover: profile.comp_xover,
            stereo_width,
            mono_bass_freq,
            limiter_ceiling_db,
        }
    }
}

/// Result of auto-tuning computation.
pub struct AutoResult {
    pub eq_bands: [EqBandParams; 4],
    pub comp_bands: [BandCompParams; 4],
    pub comp_xover: [f32; 3],
    pub stereo_width: f32,
    pub mono_bass_freq: f32,
    pub limiter_ceiling_db: f32,
}

/// Average dB energy in spectrum around `center_freq` (±10% bandwidth).
fn avg_energy_around(spectrum_db: &[f32], center_freq: f32, bin_hz: f32, bin_count: usize) -> f32 {
    let low = (center_freq * 0.9 / bin_hz) as usize;
    let high = ((center_freq * 1.1 / bin_hz) as usize).min(bin_count - 1);
    if high <= low {
        return -60.0;
    }
    let sum: f32 = spectrum_db[low..=high].iter().sum();
    sum / (high - low + 1) as f32
}

#[inline(always)]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
