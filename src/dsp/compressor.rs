use std::f32::consts::PI;

// ---------------------------------------------------------------------------
// Linkwitz-Riley 2nd-order (LR2) crossover — two cascaded 1st-order
// Butterworth sections, giving -6 dB at the crossover frequency and a flat
// summed magnitude response.
//
// We implement this as a pair of 2nd-order biquads (lowpass + highpass) whose
// coefficients are derived from the textbook Butterworth 2nd-order design with
// Q = 0.5 (which is equivalent to two cascaded 1st-order Butterworth filters).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn new() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    /// Butterworth 2nd-order lowpass with Q = 0.5 (Linkwitz-Riley).
    fn set_lr2_lowpass(&mut self, freq: f32, sr: f32) {
        let w0 = 2.0 * PI * freq / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * 0.5); // Q = 0.5

        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// Butterworth 2nd-order highpass with Q = 0.5 (Linkwitz-Riley).
    fn set_lr2_highpass(&mut self, freq: f32, sr: f32) {
        let w0 = 2.0 * PI * freq / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * 0.5);

        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline(always)]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

// ---------------------------------------------------------------------------
// 3-way crossover splits into 4 bands:
//   input -> LP1/HP1 @ freq_low
//            LP1 = sub band
//            HP1 -> LP2/HP2 @ freq_mid
//                   LP2 = low-mid band
//                   HP2 -> LP3/HP3 @ freq_high
//                          LP3 = high-mid band
//                          HP3 = high band
// We need one pair per channel (L/R) per crossover point = 6 filter pairs.
// ---------------------------------------------------------------------------

/// Stereo crossover: one LP + HP pair per channel.
#[derive(Clone, Copy)]
struct StereoCrossover {
    lp_l: Biquad,
    hp_l: Biquad,
    lp_r: Biquad,
    hp_r: Biquad,
}

impl StereoCrossover {
    fn new() -> Self {
        Self {
            lp_l: Biquad::new(),
            hp_l: Biquad::new(),
            lp_r: Biquad::new(),
            hp_r: Biquad::new(),
        }
    }

    fn set_freq(&mut self, freq: f32, sr: f32) {
        self.lp_l.set_lr2_lowpass(freq, sr);
        self.hp_l.set_lr2_highpass(freq, sr);
        self.lp_r.set_lr2_lowpass(freq, sr);
        self.hp_r.set_lr2_highpass(freq, sr);
    }

    fn reset(&mut self) {
        self.lp_l.reset();
        self.hp_l.reset();
        self.lp_r.reset();
        self.hp_r.reset();
    }

    /// Process stereo input; returns ((lp_l, lp_r), (hp_l, hp_r)).
    #[inline(always)]
    fn process(&mut self, l: f32, r: f32) -> ((f32, f32), (f32, f32)) {
        let lp_l = self.lp_l.tick(l);
        let hp_l = self.hp_l.tick(l);
        let lp_r = self.lp_r.tick(r);
        let hp_r = self.hp_r.tick(r);
        ((lp_l, lp_r), (hp_l, hp_r))
    }
}

// ---------------------------------------------------------------------------
// Per-band compressor with peak envelope follower.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct BandCompParams {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
}

impl Default for BandCompParams {
    fn default() -> Self {
        Self {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 50.0,
            makeup_db: 0.0,
        }
    }
}

#[derive(Clone, Copy)]
struct BandCompressor {
    params: BandCompParams,
    env: f32,         // envelope level (linear)
    attack_coeff: f32,
    release_coeff: f32,
    sample_rate: f32,
}

impl BandCompressor {
    fn new(sample_rate: f32) -> Self {
        let mut bc = Self {
            params: BandCompParams::default(),
            env: 0.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            sample_rate,
        };
        bc.recalc_coeffs();
        bc
    }

    fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        self.recalc_coeffs();
    }

    fn reset(&mut self) {
        self.env = 0.0;
    }

    fn set_params(&mut self, p: BandCompParams) {
        self.params = p;
        self.recalc_coeffs();
    }

    fn recalc_coeffs(&mut self) {
        self.attack_coeff = (-1.0 / (self.params.attack_ms * 0.001 * self.sample_rate)).exp();
        self.release_coeff = (-1.0 / (self.params.release_ms * 0.001 * self.sample_rate)).exp();
    }

    /// Process a stereo pair belonging to this band. Returns compressed (l, r).
    #[inline(always)]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // Detector: peak of L/R.
        let input_level = l.abs().max(r.abs());

        // Smooth envelope.
        let coeff = if input_level > self.env {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.env = coeff * self.env + (1.0 - coeff) * input_level;

        // Convert envelope to dB.
        let env_db = if self.env > 1e-12 {
            20.0 * self.env.log10()
        } else {
            -120.0
        };

        // Gain computer.
        let over_db = env_db - self.params.threshold_db;
        let gain_reduction_db = if over_db > 0.0 {
            over_db - over_db / self.params.ratio
        } else {
            0.0
        };

        let gain = db_to_linear(-gain_reduction_db + self.params.makeup_db);

        (l * gain, r * gain)
    }
}

// ---------------------------------------------------------------------------
// Multiband compressor — 4 bands.
// ---------------------------------------------------------------------------

/// Default crossover frequencies (Hz).
const DEFAULT_XOVER_LOW: f32 = 120.0;
const DEFAULT_XOVER_MID: f32 = 1200.0;
const DEFAULT_XOVER_HIGH: f32 = 6000.0;

pub struct MultibandCompressor {
    sample_rate: f32,

    // Three crossover points.
    xover_low: StereoCrossover,
    xover_mid: StereoCrossover,
    xover_high: StereoCrossover,

    // Per-band compressors.
    comp: [BandCompressor; 4],

    // Crossover frequencies.
    freq_low: f32,
    freq_mid: f32,
    freq_high: f32,
}

impl MultibandCompressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut xover_low = StereoCrossover::new();
        let mut xover_mid = StereoCrossover::new();
        let mut xover_high = StereoCrossover::new();
        xover_low.set_freq(DEFAULT_XOVER_LOW, sample_rate);
        xover_mid.set_freq(DEFAULT_XOVER_MID, sample_rate);
        xover_high.set_freq(DEFAULT_XOVER_HIGH, sample_rate);

        Self {
            sample_rate,
            xover_low,
            xover_mid,
            xover_high,
            comp: [BandCompressor::new(sample_rate); 4],
            freq_low: DEFAULT_XOVER_LOW,
            freq_mid: DEFAULT_XOVER_MID,
            freq_high: DEFAULT_XOVER_HIGH,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        self.xover_low.set_freq(self.freq_low, sr);
        self.xover_mid.set_freq(self.freq_mid, sr);
        self.xover_high.set_freq(self.freq_high, sr);
        for c in self.comp.iter_mut() {
            c.set_sample_rate(sr);
        }
    }

    pub fn reset(&mut self) {
        self.xover_low.reset();
        self.xover_mid.reset();
        self.xover_high.reset();
        for c in self.comp.iter_mut() {
            c.reset();
        }
    }

    /// Set crossover frequencies.
    pub fn set_crossover_freqs(&mut self, low: f32, mid: f32, high: f32) {
        self.freq_low = low;
        self.freq_mid = mid;
        self.freq_high = high;
        self.xover_low.set_freq(low, self.sample_rate);
        self.xover_mid.set_freq(mid, self.sample_rate);
        self.xover_high.set_freq(high, self.sample_rate);
    }

    /// Set parameters for a band (0 = sub, 1 = low-mid, 2 = high-mid, 3 = high).
    pub fn set_band_params(&mut self, band: usize, params: BandCompParams) {
        debug_assert!(band < 4);
        self.comp[band].set_params(params);
    }

    /// Get current parameters for a band.
    pub fn get_band_params(&self, band: usize) -> BandCompParams {
        self.comp[band].params
    }

    /// Process a stereo sample pair through the full multiband chain.
    /// Returns (left, right).
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // First split: sub | rest.
        let (sub, rest) = self.xover_low.process(left, right);

        // Second split on rest: low-mid | upper.
        let (low_mid, upper) = self.xover_mid.process(rest.0, rest.1);

        // Third split on upper: high-mid | high.
        let (high_mid, high) = self.xover_high.process(upper.0, upper.1);

        // Compress each band.
        let sub_out = self.comp[0].process(sub.0, sub.1);
        let lm_out = self.comp[1].process(low_mid.0, low_mid.1);
        let hm_out = self.comp[2].process(high_mid.0, high_mid.1);
        let hi_out = self.comp[3].process(high.0, high.1);

        // Sum bands.
        let out_l = sub_out.0 + lm_out.0 + hm_out.0 + hi_out.0;
        let out_r = sub_out.1 + lm_out.1 + hm_out.1 + hi_out.1;

        (out_l, out_r)
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

#[inline(always)]
fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
