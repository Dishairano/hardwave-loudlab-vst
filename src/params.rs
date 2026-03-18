//! DAW-exposed parameters for Hardwave Master.

use nih_plug::prelude::*;

/// Genre presets for spectral/dynamic targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum Genre {
    #[name = "Hardstyle"]
    Hardstyle,
    #[name = "Rawstyle"]
    Rawstyle,
    #[name = "Hardcore"]
    Hardcore,
    #[name = "Frenchcore"]
    Frenchcore,
    #[name = "EDM"]
    Edm,
    #[name = "Hip-Hop"]
    HipHop,
    #[name = "Flat"]
    Flat,
}

#[derive(Params)]
pub struct HardwaveMasterParams {
    // ── Global ─────────────────────────────────────────────────────────────
    /// Genre target for AI auto-tuning
    #[id = "genre"]
    pub genre: EnumParam<Genre>,

    /// Master intensity (0 = bypass, 1 = full AI processing)
    #[id = "intensity"]
    pub intensity: FloatParam,

    /// Input gain (dB)
    #[id = "input_gain"]
    pub input_gain: FloatParam,

    /// Output gain (dB)
    #[id = "output_gain"]
    pub output_gain: FloatParam,

    /// Mix (dry/wet)
    #[id = "mix"]
    pub mix: FloatParam,

    /// Enable auto mode (AI sets all parameters)
    #[id = "auto_mode"]
    pub auto_mode: BoolParam,

    // ── EQ (4 bands) ───────────────────────────────────────────────────────
    #[id = "eq_enabled"]
    pub eq_enabled: BoolParam,

    #[id = "eq_low_freq"]
    pub eq_low_freq: FloatParam,
    #[id = "eq_low_gain"]
    pub eq_low_gain: FloatParam,
    #[id = "eq_low_q"]
    pub eq_low_q: FloatParam,

    #[id = "eq_low_mid_freq"]
    pub eq_low_mid_freq: FloatParam,
    #[id = "eq_low_mid_gain"]
    pub eq_low_mid_gain: FloatParam,
    #[id = "eq_low_mid_q"]
    pub eq_low_mid_q: FloatParam,

    #[id = "eq_high_mid_freq"]
    pub eq_high_mid_freq: FloatParam,
    #[id = "eq_high_mid_gain"]
    pub eq_high_mid_gain: FloatParam,
    #[id = "eq_high_mid_q"]
    pub eq_high_mid_q: FloatParam,

    #[id = "eq_high_freq"]
    pub eq_high_freq: FloatParam,
    #[id = "eq_high_gain"]
    pub eq_high_gain: FloatParam,
    #[id = "eq_high_q"]
    pub eq_high_q: FloatParam,

    // ── Multiband Compressor ───────────────────────────────────────────────
    #[id = "comp_enabled"]
    pub comp_enabled: BoolParam,

    // Crossover frequencies
    #[id = "comp_xover_low"]
    pub comp_xover_low: FloatParam,
    #[id = "comp_xover_mid"]
    pub comp_xover_mid: FloatParam,
    #[id = "comp_xover_high"]
    pub comp_xover_high: FloatParam,

    // Sub band
    #[id = "comp_sub_thresh"]
    pub comp_sub_thresh: FloatParam,
    #[id = "comp_sub_ratio"]
    pub comp_sub_ratio: FloatParam,
    #[id = "comp_sub_attack"]
    pub comp_sub_attack: FloatParam,
    #[id = "comp_sub_release"]
    pub comp_sub_release: FloatParam,

    // Low-mid band
    #[id = "comp_lm_thresh"]
    pub comp_lm_thresh: FloatParam,
    #[id = "comp_lm_ratio"]
    pub comp_lm_ratio: FloatParam,
    #[id = "comp_lm_attack"]
    pub comp_lm_attack: FloatParam,
    #[id = "comp_lm_release"]
    pub comp_lm_release: FloatParam,

    // High-mid band
    #[id = "comp_hm_thresh"]
    pub comp_hm_thresh: FloatParam,
    #[id = "comp_hm_ratio"]
    pub comp_hm_ratio: FloatParam,
    #[id = "comp_hm_attack"]
    pub comp_hm_attack: FloatParam,
    #[id = "comp_hm_release"]
    pub comp_hm_release: FloatParam,

    // High band
    #[id = "comp_hi_thresh"]
    pub comp_hi_thresh: FloatParam,
    #[id = "comp_hi_ratio"]
    pub comp_hi_ratio: FloatParam,
    #[id = "comp_hi_attack"]
    pub comp_hi_attack: FloatParam,
    #[id = "comp_hi_release"]
    pub comp_hi_release: FloatParam,

    // ── Stereo ─────────────────────────────────────────────────────────────
    #[id = "stereo_enabled"]
    pub stereo_enabled: BoolParam,

    #[id = "stereo_width"]
    pub stereo_width: FloatParam,

    #[id = "stereo_mono_bass"]
    pub stereo_mono_bass: BoolParam,

    #[id = "stereo_mono_bass_freq"]
    pub stereo_mono_bass_freq: FloatParam,

    // ── Limiter ────────────────────────────────────────────────────────────
    #[id = "limiter_enabled"]
    pub limiter_enabled: BoolParam,

    #[id = "limiter_ceiling"]
    pub limiter_ceiling: FloatParam,
}

impl Default for HardwaveMasterParams {
    fn default() -> Self {
        Self {
            // Global
            genre: EnumParam::new("Genre", Genre::Hardstyle),
            intensity: FloatParam::new(
                "Intensity",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            input_gain: FloatParam::new(
                "Input Gain",
                0.0,
                FloatRange::Linear { min: -12.0, max: 12.0 },
            )
            .with_unit(" dB"),
            output_gain: FloatParam::new(
                "Output Gain",
                0.0,
                FloatRange::Linear { min: -12.0, max: 12.0 },
            )
            .with_unit(" dB"),
            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            auto_mode: BoolParam::new("Auto", true),

            // EQ
            eq_enabled: BoolParam::new("EQ On", true),
            eq_low_freq: FloatParam::new(
                "EQ Low Freq",
                80.0,
                FloatRange::Skewed { min: 20.0, max: 500.0, factor: FloatRange::skew_factor(-1.5) },
            )
            .with_unit(" Hz"),
            eq_low_gain: FloatParam::new(
                "EQ Low Gain",
                0.0,
                FloatRange::Linear { min: -12.0, max: 12.0 },
            )
            .with_unit(" dB"),
            eq_low_q: FloatParam::new(
                "EQ Low Q",
                0.7,
                FloatRange::Skewed { min: 0.1, max: 10.0, factor: FloatRange::skew_factor(-2.0) },
            ),
            eq_low_mid_freq: FloatParam::new(
                "EQ Low-Mid Freq",
                500.0,
                FloatRange::Skewed { min: 200.0, max: 2000.0, factor: FloatRange::skew_factor(-1.0) },
            )
            .with_unit(" Hz"),
            eq_low_mid_gain: FloatParam::new(
                "EQ Low-Mid Gain",
                0.0,
                FloatRange::Linear { min: -12.0, max: 12.0 },
            )
            .with_unit(" dB"),
            eq_low_mid_q: FloatParam::new(
                "EQ Low-Mid Q",
                0.7,
                FloatRange::Skewed { min: 0.1, max: 10.0, factor: FloatRange::skew_factor(-2.0) },
            ),
            eq_high_mid_freq: FloatParam::new(
                "EQ High-Mid Freq",
                3000.0,
                FloatRange::Skewed { min: 1000.0, max: 8000.0, factor: FloatRange::skew_factor(-1.0) },
            )
            .with_unit(" Hz"),
            eq_high_mid_gain: FloatParam::new(
                "EQ High-Mid Gain",
                0.0,
                FloatRange::Linear { min: -12.0, max: 12.0 },
            )
            .with_unit(" dB"),
            eq_high_mid_q: FloatParam::new(
                "EQ High-Mid Q",
                0.7,
                FloatRange::Skewed { min: 0.1, max: 10.0, factor: FloatRange::skew_factor(-2.0) },
            ),
            eq_high_freq: FloatParam::new(
                "EQ High Freq",
                10000.0,
                FloatRange::Skewed { min: 4000.0, max: 20000.0, factor: FloatRange::skew_factor(-1.0) },
            )
            .with_unit(" Hz"),
            eq_high_gain: FloatParam::new(
                "EQ High Gain",
                0.0,
                FloatRange::Linear { min: -12.0, max: 12.0 },
            )
            .with_unit(" dB"),
            eq_high_q: FloatParam::new(
                "EQ High Q",
                0.7,
                FloatRange::Skewed { min: 0.1, max: 10.0, factor: FloatRange::skew_factor(-2.0) },
            ),

            // Multiband Compressor
            comp_enabled: BoolParam::new("Comp On", true),
            comp_xover_low: FloatParam::new(
                "Xover Low",
                120.0,
                FloatRange::Skewed { min: 20.0, max: 500.0, factor: FloatRange::skew_factor(-1.5) },
            )
            .with_unit(" Hz"),
            comp_xover_mid: FloatParam::new(
                "Xover Mid",
                2500.0,
                FloatRange::Skewed { min: 500.0, max: 5000.0, factor: FloatRange::skew_factor(-1.0) },
            )
            .with_unit(" Hz"),
            comp_xover_high: FloatParam::new(
                "Xover High",
                8000.0,
                FloatRange::Skewed { min: 3000.0, max: 16000.0, factor: FloatRange::skew_factor(-1.0) },
            )
            .with_unit(" Hz"),

            // Sub band defaults
            comp_sub_thresh: FloatParam::new("Sub Thresh", -12.0, FloatRange::Linear { min: -40.0, max: 0.0 }).with_unit(" dB"),
            comp_sub_ratio: FloatParam::new("Sub Ratio", 2.0, FloatRange::Skewed { min: 1.0, max: 20.0, factor: FloatRange::skew_factor(-2.0) }),
            comp_sub_attack: FloatParam::new("Sub Attack", 10.0, FloatRange::Skewed { min: 0.1, max: 100.0, factor: FloatRange::skew_factor(-2.0) }).with_unit(" ms"),
            comp_sub_release: FloatParam::new("Sub Release", 100.0, FloatRange::Skewed { min: 10.0, max: 1000.0, factor: FloatRange::skew_factor(-1.5) }).with_unit(" ms"),

            // Low-mid band defaults
            comp_lm_thresh: FloatParam::new("LM Thresh", -15.0, FloatRange::Linear { min: -40.0, max: 0.0 }).with_unit(" dB"),
            comp_lm_ratio: FloatParam::new("LM Ratio", 3.0, FloatRange::Skewed { min: 1.0, max: 20.0, factor: FloatRange::skew_factor(-2.0) }),
            comp_lm_attack: FloatParam::new("LM Attack", 5.0, FloatRange::Skewed { min: 0.1, max: 100.0, factor: FloatRange::skew_factor(-2.0) }).with_unit(" ms"),
            comp_lm_release: FloatParam::new("LM Release", 80.0, FloatRange::Skewed { min: 10.0, max: 1000.0, factor: FloatRange::skew_factor(-1.5) }).with_unit(" ms"),

            // High-mid band defaults
            comp_hm_thresh: FloatParam::new("HM Thresh", -18.0, FloatRange::Linear { min: -40.0, max: 0.0 }).with_unit(" dB"),
            comp_hm_ratio: FloatParam::new("HM Ratio", 2.5, FloatRange::Skewed { min: 1.0, max: 20.0, factor: FloatRange::skew_factor(-2.0) }),
            comp_hm_attack: FloatParam::new("HM Attack", 3.0, FloatRange::Skewed { min: 0.1, max: 100.0, factor: FloatRange::skew_factor(-2.0) }).with_unit(" ms"),
            comp_hm_release: FloatParam::new("HM Release", 60.0, FloatRange::Skewed { min: 10.0, max: 1000.0, factor: FloatRange::skew_factor(-1.5) }).with_unit(" ms"),

            // High band defaults
            comp_hi_thresh: FloatParam::new("Hi Thresh", -20.0, FloatRange::Linear { min: -40.0, max: 0.0 }).with_unit(" dB"),
            comp_hi_ratio: FloatParam::new("Hi Ratio", 2.0, FloatRange::Skewed { min: 1.0, max: 20.0, factor: FloatRange::skew_factor(-2.0) }),
            comp_hi_attack: FloatParam::new("Hi Attack", 1.0, FloatRange::Skewed { min: 0.1, max: 100.0, factor: FloatRange::skew_factor(-2.0) }).with_unit(" ms"),
            comp_hi_release: FloatParam::new("Hi Release", 50.0, FloatRange::Skewed { min: 10.0, max: 1000.0, factor: FloatRange::skew_factor(-1.5) }).with_unit(" ms"),

            // Stereo
            stereo_enabled: BoolParam::new("Stereo On", true),
            stereo_width: FloatParam::new(
                "Width",
                1.0,
                FloatRange::Linear { min: 0.0, max: 2.0 },
            ),
            stereo_mono_bass: BoolParam::new("Mono Bass", true),
            stereo_mono_bass_freq: FloatParam::new(
                "Mono Bass Freq",
                120.0,
                FloatRange::Skewed { min: 20.0, max: 300.0, factor: FloatRange::skew_factor(-1.5) },
            )
            .with_unit(" Hz"),

            // Limiter
            limiter_enabled: BoolParam::new("Limiter On", true),
            limiter_ceiling: FloatParam::new(
                "Ceiling",
                -0.3,
                FloatRange::Linear { min: -6.0, max: 0.0 },
            )
            .with_unit(" dB"),
        }
    }
}
