use std::f32::consts::PI;

// ---------------------------------------------------------------------------
// ITU-R BS.1770-4 K-weighting pre-filter (two cascaded biquads).
//
// Stage 1: High-shelf boost (+4 dB at high frequencies, modelling the
//          acoustic effect of the head).
// Stage 2: High-pass at ~38 Hz (RLB weighting — revised low-frequency
//          B-curve).
//
// The coefficients below are for 48 kHz. We recalculate for arbitrary sample
// rates using the bilinear transform.
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

    // ----- K-weighting stage 1: high shelf -----
    // Attempt to match the ITU reference coefficients by designing a high-shelf
    // boost of approximately +4 dB with a transition around 1500 Hz.
    fn set_k_weight_stage1(&mut self, sr: f32) {
        let db = 3.999_843_8;
        let f0 = 1681.974_5;
        let q = 0.7071752;

        let a = 10.0_f32.powf(db / 40.0);
        let w0 = 2.0 * PI * f0 / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        let a_plus_1 = a + 1.0;
        let a_minus_1 = a - 1.0;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let b0 = a * (a_plus_1 + a_minus_1 * cos_w0 + two_sqrt_a_alpha);
        let b1 = -2.0 * a * (a_minus_1 + a_plus_1 * cos_w0);
        let b2 = a * (a_plus_1 + a_minus_1 * cos_w0 - two_sqrt_a_alpha);
        let a0 = a_plus_1 - a_minus_1 * cos_w0 + two_sqrt_a_alpha;
        let a1 = 2.0 * (a_minus_1 - a_plus_1 * cos_w0);
        let a2 = a_plus_1 - a_minus_1 * cos_w0 - two_sqrt_a_alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    // ----- K-weighting stage 2: RLB highpass -----
    fn set_k_weight_stage2(&mut self, sr: f32) {
        let f0 = 38.135_47;
        let q = 0.5003_27;

        let w0 = 2.0 * PI * f0 / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

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
}

// ---------------------------------------------------------------------------
// LUFS meter (momentary, 400 ms) + true peak.
// ---------------------------------------------------------------------------

/// Number of samples in the 400 ms momentary window.
fn momentary_window_samples(sr: f32) -> usize {
    (sr * 0.4) as usize
}

pub struct LufsMeter {
    sample_rate: f32,

    // K-weighting filters, one pair per channel (L, R).
    stage1_l: Biquad,
    stage2_l: Biquad,
    stage1_r: Biquad,
    stage2_r: Biquad,

    // Ring buffer of per-sample mean-square values (K-weighted, summed L+R)
    // used for the 400 ms momentary measurement.
    ms_ring: Vec<f32>,
    ring_pos: usize,
    ring_sum: f64,   // running sum of the ring buffer for O(1) average
    ring_count: usize,
    window_len: usize,

    // True peak tracking (intersample estimation via 4x oversampling).
    true_peak: f32,

    // Previous samples for 4x linear interpolation (one per channel).
    prev_l: f32,
    prev_r: f32,

    // Cached momentary LUFS value.
    cached_momentary: f32,
}

impl LufsMeter {
    pub fn new(sample_rate: f32) -> Self {
        let window_len = momentary_window_samples(sample_rate);
        let mut meter = Self {
            sample_rate,
            stage1_l: Biquad::new(),
            stage2_l: Biquad::new(),
            stage1_r: Biquad::new(),
            stage2_r: Biquad::new(),
            ms_ring: vec![0.0; window_len],
            ring_pos: 0,
            ring_sum: 0.0,
            ring_count: 0,
            window_len,
            true_peak: 0.0,
            prev_l: 0.0,
            prev_r: 0.0,
            cached_momentary: -120.0,
        };
        meter.init_filters();
        meter
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        let window_len = momentary_window_samples(sr);
        self.ms_ring.resize(window_len, 0.0);
        self.window_len = window_len;
        self.init_filters();
        self.reset();
    }

    pub fn reset(&mut self) {
        self.stage1_l.reset();
        self.stage2_l.reset();
        self.stage1_r.reset();
        self.stage2_r.reset();
        self.ms_ring.iter_mut().for_each(|s| *s = 0.0);
        self.ring_pos = 0;
        self.ring_sum = 0.0;
        self.ring_count = 0;
        self.true_peak = 0.0;
        self.prev_l = 0.0;
        self.prev_r = 0.0;
        self.cached_momentary = -120.0;
    }

    fn init_filters(&mut self) {
        self.stage1_l.set_k_weight_stage1(self.sample_rate);
        self.stage2_l.set_k_weight_stage2(self.sample_rate);
        self.stage1_r.set_k_weight_stage1(self.sample_rate);
        self.stage2_r.set_k_weight_stage2(self.sample_rate);
    }

    /// Feed a stereo sample pair into the meter.
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) {
        // --- K-weighting ---
        let kl = self.stage2_l.tick(self.stage1_l.tick(left));
        let kr = self.stage2_r.tick(self.stage1_r.tick(right));

        // --- Mean square for LUFS ---
        // BS.1770 uses equal weighting for L and R (G_l = G_r = 1.0).
        let ms = kl * kl + kr * kr;

        // Update ring buffer.
        let old = self.ms_ring[self.ring_pos];
        self.ms_ring[self.ring_pos] = ms;
        self.ring_sum += ms as f64 - old as f64;
        // Clamp to avoid floating point drift going negative.
        if self.ring_sum < 0.0 {
            self.ring_sum = 0.0;
        }
        self.ring_pos += 1;
        if self.ring_pos >= self.window_len {
            self.ring_pos = 0;
        }
        if self.ring_count < self.window_len {
            self.ring_count += 1;
        }

        // Cache momentary LUFS.
        if self.ring_count > 0 {
            let mean = self.ring_sum / self.ring_count as f64;
            self.cached_momentary = if mean > 1e-20 {
                // -0.691 + 10 * log10(mean)  (BS.1770 equation)
                -0.691 + 10.0 * (mean as f32).log10()
            } else {
                -120.0
            };
        }

        // --- True peak (4x oversampled linear interpolation) ---
        // Simple 4x linear interpolation between previous and current sample
        // to estimate intersample peaks.
        self.update_true_peak(self.prev_l, left);
        self.update_true_peak(self.prev_r, right);
        self.prev_l = left;
        self.prev_r = right;
    }

    #[inline(always)]
    fn update_true_peak(&mut self, prev: f32, curr: f32) {
        // 4x linear interpolation: check at 0.25, 0.5, 0.75 intervals.
        let d = curr - prev;
        let p1 = (prev + d * 0.25).abs();
        let p2 = (prev + d * 0.5).abs();
        let p3 = (prev + d * 0.75).abs();
        let p4 = curr.abs();
        let peak = p1.max(p2).max(p3).max(p4);
        if peak > self.true_peak {
            self.true_peak = peak;
        }
    }

    /// Momentary LUFS (400 ms window).
    pub fn momentary_lufs(&self) -> f32 {
        self.cached_momentary
    }

    /// True peak in dBTP (decibels relative to true peak).
    pub fn true_peak(&self) -> f32 {
        if self.true_peak > 1e-12 {
            20.0 * self.true_peak.log10()
        } else {
            -120.0
        }
    }

    /// True peak as a linear value.
    pub fn true_peak_linear(&self) -> f32 {
        self.true_peak
    }

    /// Reset the true peak hold (call e.g. on playback start).
    pub fn reset_true_peak(&mut self) {
        self.true_peak = 0.0;
    }
}
