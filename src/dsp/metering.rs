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
// LUFS meter — BS.1770-4: momentary (400 ms), short-term (3 s), and integrated
// (gated, full-program). Also tracks true peak.
// ---------------------------------------------------------------------------

/// Number of samples in the 400 ms momentary window.
fn momentary_window_samples(sr: f32) -> usize {
    (sr * 0.4) as usize
}

/// Number of samples in the 3 s short-term window.
fn short_term_window_samples(sr: f32) -> usize {
    (sr * 3.0) as usize
}

/// BS.1770-4 integrated LUFS uses 400 ms blocks with 75% overlap, i.e. a new
/// block emitted every 100 ms. We accumulate per-block mean-square energy
/// directly via a small running ring of `step_samples` instead of re-scanning
/// the full 400 ms ring buffer, which would be costly per sample.
fn integrated_step_samples(sr: f32) -> usize {
    (sr * 0.1) as usize
}

/// Cap on retained block-energy samples for integrated LUFS — at 100 ms per
/// block this gives ~30 minutes of program. Plenty for any realistic master,
/// and bounded so the audio-thread ring stays cache-friendly.
const INTEGRATED_MAX_BLOCKS: usize = 18_000;

pub struct LufsMeter {
    sample_rate: f32,

    // K-weighting filters, one pair per channel (L, R).
    stage1_l: Biquad,
    stage2_l: Biquad,
    stage1_r: Biquad,
    stage2_r: Biquad,

    // ── Momentary (400 ms) ───────────────────────────────────────────────
    ms_ring: Vec<f32>,
    ring_pos: usize,
    ring_sum: f64,
    ring_count: usize,
    window_len: usize,

    // ── Short-term (3 s) ─────────────────────────────────────────────────
    st_ring: Vec<f32>,
    st_pos: usize,
    st_sum: f64,
    st_count: usize,
    st_window_len: usize,
    cached_short_term: f32,

    // ── Integrated (BS.1770-4 gated) ─────────────────────────────────────
    // Per-100ms-step mean-square buffer. Recompute integrated LUFS each time
    // a block boundary is crossed (so callers see a refreshed value at ~10
    // Hz, not on every audio sample).
    integ_step_ring: Vec<f32>,
    integ_step_pos: usize,
    integ_step_sum: f64,
    integ_step_count: usize,
    integ_step_len: usize,
    samples_since_block: usize,
    // Block-energy log (oldest at 0, newest at len-1, capped at MAX_BLOCKS).
    block_energies: Vec<f32>,
    cached_integrated: f32,

    // ── True peak ────────────────────────────────────────────────────────
    true_peak: f32,
    prev_l: f32,
    prev_r: f32,

    // ── Momentary cache ──────────────────────────────────────────────────
    cached_momentary: f32,
}

impl LufsMeter {
    pub fn new(sample_rate: f32) -> Self {
        let window_len = momentary_window_samples(sample_rate);
        let st_window_len = short_term_window_samples(sample_rate);
        let integ_step_len = integrated_step_samples(sample_rate);
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

            st_ring: vec![0.0; st_window_len],
            st_pos: 0,
            st_sum: 0.0,
            st_count: 0,
            st_window_len,
            cached_short_term: -120.0,

            integ_step_ring: vec![0.0; integ_step_len],
            integ_step_pos: 0,
            integ_step_sum: 0.0,
            integ_step_count: 0,
            integ_step_len,
            samples_since_block: 0,
            block_energies: Vec::with_capacity(INTEGRATED_MAX_BLOCKS),
            cached_integrated: -120.0,

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
        let st_window_len = short_term_window_samples(sr);
        let integ_step_len = integrated_step_samples(sr);
        self.ms_ring.resize(window_len, 0.0);
        self.window_len = window_len;
        self.st_ring.resize(st_window_len, 0.0);
        self.st_window_len = st_window_len;
        self.integ_step_ring.resize(integ_step_len, 0.0);
        self.integ_step_len = integ_step_len;
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

        self.st_ring.iter_mut().for_each(|s| *s = 0.0);
        self.st_pos = 0;
        self.st_sum = 0.0;
        self.st_count = 0;
        self.cached_short_term = -120.0;

        self.integ_step_ring.iter_mut().for_each(|s| *s = 0.0);
        self.integ_step_pos = 0;
        self.integ_step_sum = 0.0;
        self.integ_step_count = 0;
        self.samples_since_block = 0;
        self.block_energies.clear();
        self.cached_integrated = -120.0;

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

        // --- Momentary (400 ms ring) ---
        let old = self.ms_ring[self.ring_pos];
        self.ms_ring[self.ring_pos] = ms;
        self.ring_sum += ms as f64 - old as f64;
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
        if self.ring_count > 0 {
            let mean = self.ring_sum / self.ring_count as f64;
            self.cached_momentary = lufs_from_mean_square(mean);
        }

        // --- Short-term (3 s ring) ---
        let st_old = self.st_ring[self.st_pos];
        self.st_ring[self.st_pos] = ms;
        self.st_sum += ms as f64 - st_old as f64;
        if self.st_sum < 0.0 {
            self.st_sum = 0.0;
        }
        self.st_pos += 1;
        if self.st_pos >= self.st_window_len {
            self.st_pos = 0;
        }
        if self.st_count < self.st_window_len {
            self.st_count += 1;
        }
        if self.st_count > 0 {
            let mean = self.st_sum / self.st_count as f64;
            self.cached_short_term = lufs_from_mean_square(mean);
        }

        // --- Integrated (BS.1770-4 gated) ---
        // Accumulate into a 100 ms step buffer. Each completed step yields one
        // block-energy value that the gate uses. We do a 75%-overlapping 400 ms
        // measurement implicitly by always pulling the *current* 400 ms ring
        // mean at each step boundary (which is the BS.1770-4 definition of a
        // gating block).
        let step_old = self.integ_step_ring[self.integ_step_pos];
        self.integ_step_ring[self.integ_step_pos] = ms;
        self.integ_step_sum += ms as f64 - step_old as f64;
        if self.integ_step_sum < 0.0 {
            self.integ_step_sum = 0.0;
        }
        self.integ_step_pos += 1;
        if self.integ_step_pos >= self.integ_step_len {
            self.integ_step_pos = 0;
        }
        if self.integ_step_count < self.integ_step_len {
            self.integ_step_count += 1;
        }

        self.samples_since_block += 1;
        if self.samples_since_block >= self.integ_step_len {
            self.samples_since_block = 0;
            // A new 100 ms block boundary has crossed. Record the current
            // 400 ms gating-block mean-square (i.e. the momentary window).
            if self.ring_count >= self.window_len {
                let block_ms = (self.ring_sum / self.ring_count as f64) as f32;
                if self.block_energies.len() < INTEGRATED_MAX_BLOCKS {
                    self.block_energies.push(block_ms);
                    // Recompute integrated LUFS over the gated set.
                    self.cached_integrated = integrate_gated(&self.block_energies);
                }
            }
        }

        // --- True peak (4x oversampled linear interpolation) ---
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

    /// Short-term LUFS (3 s window). Returns -120.0 until enough audio has
    /// been processed to fill the window meaningfully.
    pub fn short_term_lufs(&self) -> f32 {
        self.cached_short_term
    }

    /// Integrated LUFS (BS.1770-4 gated, full program since last reset).
    /// Returns -120.0 until at least one gating block has accumulated.
    pub fn integrated_lufs(&self) -> f32 {
        self.cached_integrated
    }
}

// ---------------------------------------------------------------------------
// BS.1770 helpers.
// ---------------------------------------------------------------------------

/// Convert a mean-square value (K-weighted, L+R summed) to LUFS via
/// BS.1770: LUFS = -0.691 + 10·log10(mean_ms).
#[inline]
fn lufs_from_mean_square(mean: f64) -> f32 {
    if mean > 1e-20 {
        -0.691 + 10.0 * (mean as f32).log10()
    } else {
        -120.0
    }
}

/// BS.1770-4 integrated loudness over a slice of per-block mean-square
/// energies. Applies the two-stage gate:
///   1. Absolute gate: discard blocks below -70 LUFS.
///   2. Relative gate: discard blocks below (ungated_mean - 10 LU).
/// The final integrated value is the LUFS of the mean of doubly-gated blocks.
fn integrate_gated(blocks: &[f32]) -> f32 {
    // Absolute gate at -70 LUFS:
    //   block_lufs ≥ -70  ⇔  -0.691 + 10·log10(ms) ≥ -70
    //                     ⇔  ms ≥ 10^((-70 + 0.691) / 10)
    let abs_gate_ms = 10.0_f32.powf((-70.0 + 0.691) / 10.0);

    let mut sum1: f64 = 0.0;
    let mut count1: usize = 0;
    for &ms in blocks {
        if ms >= abs_gate_ms {
            sum1 += ms as f64;
            count1 += 1;
        }
    }
    if count1 == 0 {
        return -120.0;
    }
    let ungated_mean = sum1 / count1 as f64;
    let ungated_lufs = lufs_from_mean_square(ungated_mean);

    // Relative gate at (ungated_lufs - 10) LU:
    //   block_lufs ≥ (ungated_lufs - 10)
    //   block_ms   ≥ ungated_mean · 10^(-1) = ungated_mean / 10
    let rel_gate_ms = (ungated_mean / 10.0) as f32;
    let rel_gate_ms = rel_gate_ms.max(abs_gate_ms);

    let mut sum2: f64 = 0.0;
    let mut count2: usize = 0;
    for &ms in blocks {
        if ms >= rel_gate_ms {
            sum2 += ms as f64;
            count2 += 1;
        }
    }
    if count2 == 0 {
        // No block survives the relative gate — fall back to ungated mean.
        return ungated_lufs;
    }
    lufs_from_mean_square(sum2 / count2 as f64)
}

// ---------------------------------------------------------------------------
// Stereo meter: rolling Pearson correlation + mid/side energy ratio over a
// fixed window. Designed to share the same hot path as the LUFS meter — fed
// per-sample, computed online with O(1) per-sample cost.
// ---------------------------------------------------------------------------

/// Window length for the stereo metrics, in seconds. Three seconds matches
/// the LUFS short-term window so the user sees consistently-timed responses
/// in the right rail.
const STEREO_WINDOW_SECS: f32 = 3.0;

pub struct StereoMeter {
    sample_rate: f32,
    window_len: usize,

    // Per-sample rings: left, right, mid², side². For correlation we keep
    // running sums of L, R, L², R², L·R over the window.
    l_ring: Vec<f32>,
    r_ring: Vec<f32>,
    m2_ring: Vec<f32>,
    s2_ring: Vec<f32>,
    pos: usize,
    count: usize,

    sum_l: f64,
    sum_r: f64,
    sum_l2: f64,
    sum_r2: f64,
    sum_lr: f64,
    sum_m2: f64,
    sum_s2: f64,

    cached_correlation: f32,
    cached_ms_ratio: f32, // mid energy fraction in [0, 1]
}

impl StereoMeter {
    pub fn new(sample_rate: f32) -> Self {
        let window_len = (sample_rate * STEREO_WINDOW_SECS) as usize;
        Self {
            sample_rate,
            window_len,
            l_ring: vec![0.0; window_len],
            r_ring: vec![0.0; window_len],
            m2_ring: vec![0.0; window_len],
            s2_ring: vec![0.0; window_len],
            pos: 0,
            count: 0,
            sum_l: 0.0,
            sum_r: 0.0,
            sum_l2: 0.0,
            sum_r2: 0.0,
            sum_lr: 0.0,
            sum_m2: 0.0,
            sum_s2: 0.0,
            cached_correlation: 0.0,
            cached_ms_ratio: 0.5,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        let window_len = (sr * STEREO_WINDOW_SECS) as usize;
        self.l_ring.resize(window_len, 0.0);
        self.r_ring.resize(window_len, 0.0);
        self.m2_ring.resize(window_len, 0.0);
        self.s2_ring.resize(window_len, 0.0);
        self.window_len = window_len;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.l_ring.iter_mut().for_each(|s| *s = 0.0);
        self.r_ring.iter_mut().for_each(|s| *s = 0.0);
        self.m2_ring.iter_mut().for_each(|s| *s = 0.0);
        self.s2_ring.iter_mut().for_each(|s| *s = 0.0);
        self.pos = 0;
        self.count = 0;
        self.sum_l = 0.0;
        self.sum_r = 0.0;
        self.sum_l2 = 0.0;
        self.sum_r2 = 0.0;
        self.sum_lr = 0.0;
        self.sum_m2 = 0.0;
        self.sum_s2 = 0.0;
        self.cached_correlation = 0.0;
        self.cached_ms_ratio = 0.5;
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) {
        let m = (l + r) * 0.5;
        let s = (l - r) * 0.5;
        let m2 = m * m;
        let s2 = s * s;

        // Swap out the oldest sample at `pos` for the new sample.
        let old_l = self.l_ring[self.pos];
        let old_r = self.r_ring[self.pos];
        let old_m2 = self.m2_ring[self.pos];
        let old_s2 = self.s2_ring[self.pos];

        self.sum_l += l as f64 - old_l as f64;
        self.sum_r += r as f64 - old_r as f64;
        self.sum_l2 += (l * l) as f64 - (old_l * old_l) as f64;
        self.sum_r2 += (r * r) as f64 - (old_r * old_r) as f64;
        self.sum_lr += (l * r) as f64 - (old_l * old_r) as f64;
        self.sum_m2 += m2 as f64 - old_m2 as f64;
        self.sum_s2 += s2 as f64 - old_s2 as f64;

        self.l_ring[self.pos] = l;
        self.r_ring[self.pos] = r;
        self.m2_ring[self.pos] = m2;
        self.s2_ring[self.pos] = s2;

        self.pos += 1;
        if self.pos >= self.window_len {
            self.pos = 0;
        }
        if self.count < self.window_len {
            self.count += 1;
        }

        if self.count > 1 {
            let n = self.count as f64;

            // Pearson correlation:
            //   ρ = (n·Σlr − Σl·Σr) / sqrt((n·Σl² − (Σl)²)(n·Σr² − (Σr)²))
            let num = n * self.sum_lr - self.sum_l * self.sum_r;
            let den_l = n * self.sum_l2 - self.sum_l * self.sum_l;
            let den_r = n * self.sum_r2 - self.sum_r * self.sum_r;
            let den = (den_l * den_r).max(0.0).sqrt();
            self.cached_correlation = if den > 1e-12 {
                (num / den).clamp(-1.0, 1.0) as f32
            } else {
                // Silent / single-channel input — degenerate; report +1.0
                // (perfectly correlated mono) rather than NaN.
                1.0
            };

            // Mid/side energy ratio: mid_fraction = E_mid / (E_mid + E_side).
            // Returned in [0, 1]; the webview converts to "64 / 36" style.
            let total = self.sum_m2 + self.sum_s2;
            self.cached_ms_ratio = if total > 1e-20 {
                (self.sum_m2 / total).clamp(0.0, 1.0) as f32
            } else {
                0.5
            };
        }
    }

    /// Pearson correlation in [-1.0, +1.0]. +1.0 = mono, 0 = uncorrelated,
    /// -1.0 = inverted (phase issue).
    pub fn correlation(&self) -> f32 {
        self.cached_correlation
    }

    /// Mid-channel energy as a fraction of total (mid + side) energy, in
    /// [0.0, 1.0]. A 64/36 mid/side balance returns 0.64.
    pub fn mid_fraction(&self) -> f32 {
        self.cached_ms_ratio
    }
}
