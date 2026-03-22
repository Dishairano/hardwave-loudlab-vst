//! Genre-specific spectral target curves and dynamic profiles.
//!
//! Each genre defines:
//! - A target spectral magnitude curve (dB relative to flat, at key anchor
//!   frequencies) that the auto-EQ tries to match.
//! - Preferred compressor settings per band.
//! - Stereo preferences (width, mono-bass cutoff).
//! - Limiter ceiling.

use crate::dsp::compressor::BandCompParams;
use crate::dsp::eq::{EqBandParams, FilterType};
use crate::params::Genre;

/// Complete mastering target profile for a genre.
#[derive(Debug, Clone)]
pub struct GenreProfile {
    pub eq_bands: [EqBandParams; 4],
    pub comp_bands: [BandCompParams; 4],
    pub comp_xover: [f32; 3], // low, mid, high crossover Hz
    pub stereo_width: f32,
    pub mono_bass_freq: f32,
    pub limiter_ceiling_db: f32,
}

impl GenreProfile {
    /// Return the target profile for the given genre.
    pub fn for_genre(genre: Genre) -> Self {
        match genre {
            Genre::Hardstyle => Self::hardstyle(),
            Genre::Rawstyle => Self::rawstyle(),
            Genre::Hardcore => Self::hardcore(),
            Genre::Frenchcore => Self::frenchcore(),
            Genre::Edm => Self::edm(),
            Genre::HipHop => Self::hiphop(),
            Genre::Flat => Self::flat(),
        }
    }

    // ── Hardstyle ────────────────────────────────────────────────────────────
    fn hardstyle() -> Self {
        Self {
            eq_bands: [
                EqBandParams { freq: 60.0, gain_db: 2.5, q: 0.8, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 400.0, gain_db: -1.5, q: 1.0, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 3500.0, gain_db: 1.5, q: 0.9, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 12000.0, gain_db: 2.0, q: 0.7, enabled: true, filter_type: FilterType::Peak },
            ],
            comp_bands: [
                BandCompParams { threshold_db: -10.0, ratio: 3.0, attack_ms: 10.0, release_ms: 120.0, makeup_db: 1.0 },
                BandCompParams { threshold_db: -14.0, ratio: 2.5, attack_ms: 5.0, release_ms: 80.0, makeup_db: 0.5 },
                BandCompParams { threshold_db: -16.0, ratio: 2.0, attack_ms: 3.0, release_ms: 60.0, makeup_db: 0.0 },
                BandCompParams { threshold_db: -18.0, ratio: 1.8, attack_ms: 1.5, release_ms: 40.0, makeup_db: 0.0 },
            ],
            comp_xover: [120.0, 2500.0, 8000.0],
            stereo_width: 1.15,
            mono_bass_freq: 120.0,
            limiter_ceiling_db: -0.3,
        }
    }

    // ── Rawstyle ─────────────────────────────────────────────────────────────
    fn rawstyle() -> Self {
        Self {
            eq_bands: [
                EqBandParams { freq: 55.0, gain_db: 3.5, q: 0.7, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 350.0, gain_db: -2.0, q: 1.2, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 2000.0, gain_db: 2.0, q: 0.8, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 10000.0, gain_db: 1.0, q: 0.6, enabled: true, filter_type: FilterType::Peak },
            ],
            comp_bands: [
                BandCompParams { threshold_db: -8.0, ratio: 4.0, attack_ms: 8.0, release_ms: 100.0, makeup_db: 2.0 },
                BandCompParams { threshold_db: -12.0, ratio: 3.5, attack_ms: 4.0, release_ms: 70.0, makeup_db: 1.0 },
                BandCompParams { threshold_db: -15.0, ratio: 3.0, attack_ms: 2.0, release_ms: 50.0, makeup_db: 0.5 },
                BandCompParams { threshold_db: -18.0, ratio: 2.5, attack_ms: 1.0, release_ms: 35.0, makeup_db: 0.0 },
            ],
            comp_xover: [100.0, 2000.0, 7000.0],
            stereo_width: 1.05,
            mono_bass_freq: 150.0,
            limiter_ceiling_db: -0.1,
        }
    }

    // ── Hardcore ──────────────────────────────────────────────────────────────
    fn hardcore() -> Self {
        Self {
            eq_bands: [
                EqBandParams { freq: 50.0, gain_db: 3.0, q: 0.7, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 300.0, gain_db: -1.0, q: 1.0, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 4000.0, gain_db: 2.5, q: 0.8, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 14000.0, gain_db: 1.5, q: 0.6, enabled: true, filter_type: FilterType::Peak },
            ],
            comp_bands: [
                BandCompParams { threshold_db: -6.0, ratio: 5.0, attack_ms: 6.0, release_ms: 80.0, makeup_db: 3.0 },
                BandCompParams { threshold_db: -10.0, ratio: 4.0, attack_ms: 3.0, release_ms: 60.0, makeup_db: 2.0 },
                BandCompParams { threshold_db: -14.0, ratio: 3.5, attack_ms: 2.0, release_ms: 45.0, makeup_db: 1.0 },
                BandCompParams { threshold_db: -16.0, ratio: 3.0, attack_ms: 1.0, release_ms: 30.0, makeup_db: 0.5 },
            ],
            comp_xover: [110.0, 2200.0, 7500.0],
            stereo_width: 1.1,
            mono_bass_freq: 140.0,
            limiter_ceiling_db: -0.1,
        }
    }

    // ── Frenchcore ────────────────────────────────────────────────────────────
    fn frenchcore() -> Self {
        Self {
            eq_bands: [
                EqBandParams { freq: 45.0, gain_db: 4.0, q: 0.6, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 250.0, gain_db: -2.5, q: 1.3, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 5000.0, gain_db: 3.0, q: 0.7, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 15000.0, gain_db: 2.0, q: 0.5, enabled: true, filter_type: FilterType::Peak },
            ],
            comp_bands: [
                BandCompParams { threshold_db: -5.0, ratio: 6.0, attack_ms: 5.0, release_ms: 70.0, makeup_db: 4.0 },
                BandCompParams { threshold_db: -8.0, ratio: 5.0, attack_ms: 2.5, release_ms: 50.0, makeup_db: 2.5 },
                BandCompParams { threshold_db: -12.0, ratio: 4.0, attack_ms: 1.5, release_ms: 40.0, makeup_db: 1.5 },
                BandCompParams { threshold_db: -15.0, ratio: 3.5, attack_ms: 0.8, release_ms: 25.0, makeup_db: 1.0 },
            ],
            comp_xover: [90.0, 1800.0, 7000.0],
            stereo_width: 1.0,
            mono_bass_freq: 160.0,
            limiter_ceiling_db: -0.1,
        }
    }

    // ── EDM (generic) ────────────────────────────────────────────────────────
    fn edm() -> Self {
        Self {
            eq_bands: [
                EqBandParams { freq: 80.0, gain_db: 1.5, q: 0.8, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 500.0, gain_db: -1.0, q: 1.0, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 3000.0, gain_db: 1.0, q: 0.9, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 12000.0, gain_db: 2.5, q: 0.7, enabled: true, filter_type: FilterType::Peak },
            ],
            comp_bands: [
                BandCompParams { threshold_db: -12.0, ratio: 2.5, attack_ms: 10.0, release_ms: 100.0, makeup_db: 0.5 },
                BandCompParams { threshold_db: -14.0, ratio: 2.0, attack_ms: 5.0, release_ms: 80.0, makeup_db: 0.0 },
                BandCompParams { threshold_db: -16.0, ratio: 2.0, attack_ms: 3.0, release_ms: 60.0, makeup_db: 0.0 },
                BandCompParams { threshold_db: -20.0, ratio: 1.5, attack_ms: 2.0, release_ms: 50.0, makeup_db: 0.0 },
            ],
            comp_xover: [120.0, 2500.0, 8000.0],
            stereo_width: 1.2,
            mono_bass_freq: 100.0,
            limiter_ceiling_db: -0.3,
        }
    }

    // ── Hip-Hop ──────────────────────────────────────────────────────────────
    fn hiphop() -> Self {
        Self {
            eq_bands: [
                EqBandParams { freq: 60.0, gain_db: 3.0, q: 0.7, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 400.0, gain_db: -1.5, q: 1.0, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 2500.0, gain_db: 1.5, q: 0.9, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 10000.0, gain_db: 1.0, q: 0.8, enabled: true, filter_type: FilterType::Peak },
            ],
            comp_bands: [
                BandCompParams { threshold_db: -10.0, ratio: 3.0, attack_ms: 15.0, release_ms: 150.0, makeup_db: 1.0 },
                BandCompParams { threshold_db: -14.0, ratio: 2.5, attack_ms: 8.0, release_ms: 100.0, makeup_db: 0.5 },
                BandCompParams { threshold_db: -18.0, ratio: 2.0, attack_ms: 5.0, release_ms: 70.0, makeup_db: 0.0 },
                BandCompParams { threshold_db: -20.0, ratio: 1.5, attack_ms: 3.0, release_ms: 50.0, makeup_db: 0.0 },
            ],
            comp_xover: [100.0, 2000.0, 8000.0],
            stereo_width: 1.1,
            mono_bass_freq: 120.0,
            limiter_ceiling_db: -0.5,
        }
    }

    // ── Flat (transparent, minimal processing) ───────────────────────────────
    fn flat() -> Self {
        Self {
            eq_bands: [
                EqBandParams { freq: 80.0, gain_db: 0.0, q: 0.707, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 500.0, gain_db: 0.0, q: 0.707, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 3000.0, gain_db: 0.0, q: 0.707, enabled: true, filter_type: FilterType::Peak },
                EqBandParams { freq: 10000.0, gain_db: 0.0, q: 0.707, enabled: true, filter_type: FilterType::Peak },
            ],
            comp_bands: [
                BandCompParams { threshold_db: -20.0, ratio: 1.5, attack_ms: 10.0, release_ms: 100.0, makeup_db: 0.0 },
                BandCompParams { threshold_db: -20.0, ratio: 1.5, attack_ms: 10.0, release_ms: 100.0, makeup_db: 0.0 },
                BandCompParams { threshold_db: -20.0, ratio: 1.5, attack_ms: 10.0, release_ms: 100.0, makeup_db: 0.0 },
                BandCompParams { threshold_db: -20.0, ratio: 1.5, attack_ms: 10.0, release_ms: 100.0, makeup_db: 0.0 },
            ],
            comp_xover: [120.0, 2500.0, 8000.0],
            stereo_width: 1.0,
            mono_bass_freq: 120.0,
            limiter_ceiling_db: -0.3,
        }
    }
}
