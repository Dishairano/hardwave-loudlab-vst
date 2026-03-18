//! Hardwave Master — AI-assisted mastering VST3/CLAP plugin.
//!
//! Signal chain:
//!   Input Gain → EQ (4-band parametric) → Multiband Compressor (4 bands)
//!   → Stereo Processor (width + mono bass) → Brickwall Limiter → Output Gain
//!
//! When Auto mode is enabled, the AI engine analyses the spectrum and adjusts
//! EQ, compressor, stereo, and limiter settings toward the selected genre target.

use nih_plug::prelude::*;
use std::num::NonZeroU32;
use std::sync::Arc;

mod auth;
mod auto;
mod dsp;
mod editor;
mod params;
mod profiles;
mod protocol;

use auto::AutoEngine;
use dsp::eq::EqBandParams;
use dsp::compressor::BandCompParams;
use dsp::{
    BrickwallLimiter, LufsMeter, MultibandCompressor, ParametricEq, SpectrumAnalyzer,
    StereoProcessor,
};
use params::{Genre, HardwaveMasterParams};
use profiles::GenreProfile;

struct HardwaveMaster {
    params: Arc<HardwaveMasterParams>,

    // DSP modules — EQ is applied independently per channel.
    eq_l: ParametricEq,
    eq_r: ParametricEq,
    compressor: MultibandCompressor,
    stereo: StereoProcessor,
    limiter: BrickwallLimiter,

    // Metering.
    analyzer: SpectrumAnalyzer,
    input_meter: LufsMeter,
    output_meter: LufsMeter,

    // Auto engine.
    auto_engine: AutoEngine,

    // State for throttled auto-compute (every N samples).
    samples_since_auto: usize,
    current_profile: GenreProfile,

    sample_rate: f32,
}

impl Default for HardwaveMaster {
    fn default() -> Self {
        let sr = 44100.0;
        Self {
            params: Arc::new(HardwaveMasterParams::default()),
            eq_l: ParametricEq::new(sr),
            eq_r: ParametricEq::new(sr),
            compressor: MultibandCompressor::new(sr),
            stereo: StereoProcessor::new(sr),
            limiter: BrickwallLimiter::new(sr),
            analyzer: SpectrumAnalyzer::new(sr),
            input_meter: LufsMeter::new(sr),
            output_meter: LufsMeter::new(sr),
            auto_engine: AutoEngine::new(),
            samples_since_auto: 0,
            current_profile: GenreProfile::for_genre(Genre::Hardstyle),
            sample_rate: sr,
        }
    }
}

impl Plugin for HardwaveMaster {
    const NAME: &'static str = "Hardwave Master";
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

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let sr = buffer_config.sample_rate;
        self.sample_rate = sr;

        self.eq_l.set_sample_rate(sr);
        self.eq_r.set_sample_rate(sr);
        self.compressor.set_sample_rate(sr);
        self.stereo.set_sample_rate(sr);
        self.limiter.set_sample_rate(sr);
        self.analyzer.set_sample_rate(sr);
        self.input_meter.set_sample_rate(sr);
        self.output_meter.set_sample_rate(sr);

        true
    }

    fn reset(&mut self) {
        self.eq_l.reset();
        self.eq_r.reset();
        self.compressor.reset();
        self.stereo.reset();
        self.limiter.reset();
        self.analyzer.reset();
        self.input_meter.reset();
        self.output_meter.reset();
        self.auto_engine.reset();
        self.samples_since_auto = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let p = &self.params;

        // Read param values once per buffer.
        let intensity = p.intensity.value();
        let input_gain_db = p.input_gain.value();
        let output_gain_db = p.output_gain.value();
        let mix = p.mix.value();
        let auto_mode = p.auto_mode.value();

        let eq_enabled = p.eq_enabled.value();
        let comp_enabled = p.comp_enabled.value();
        let stereo_enabled = p.stereo_enabled.value();
        let limiter_enabled = p.limiter_enabled.value();

        let input_gain = db_to_linear(input_gain_db);
        let output_gain = db_to_linear(output_gain_db);

        // Update genre profile if needed.
        let genre = p.genre.value();
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
                    if let Some(spectrum) = self.analyzer.get_spectrum() {
                        let result = self.auto_engine.compute(
                            &spectrum,
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

            // Write output.
            *frame.get_mut(0).unwrap() = l;
            *frame.get_mut(1).unwrap() = r;
        }

        ProcessStatus::Normal
    }
}

impl HardwaveMaster {
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

        // Limiter.
        self.limiter.set_ceiling(p.limiter_ceiling.value());
    }
}

impl ClapPlugin for HardwaveMaster {
    const CLAP_ID: &'static str = "com.hardwavestudios.master";
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

impl Vst3Plugin for HardwaveMaster {
    const VST3_CLASS_ID: [u8; 16] = *b"HWMaster__v0001\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Mastering,
        Vst3SubCategory::Stereo,
    ];
}

nih_export_clap!(HardwaveMaster);
nih_export_vst3!(HardwaveMaster);

#[inline(always)]
fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
