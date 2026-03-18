/// Brickwall lookahead limiter with a soft-clip input stage.
///
/// Signal flow:
///   1. Soft-clip (tanh-based) to tame transients before the limiter.
///   2. Lookahead delay (5 ms) — the gain envelope is computed ahead of time so
///      the limiter can smoothly attenuate peaks before they arrive.
///   3. Gain reduction with attack/release smoothing.
///   4. Hard clip at ceiling as a safety net.

const LOOKAHEAD_MS: f32 = 5.0;

pub struct BrickwallLimiter {
    sample_rate: f32,

    /// Ceiling in dB (typically -0.3 to -1.0).
    pub ceiling_db: f32,

    // Derived
    ceiling_lin: f32,

    // Lookahead delay lines (stereo).
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    delay_len: usize,
    delay_pos: usize,

    // Envelope follower state.
    env: f32,

    // Smoothing coefficients.
    attack_coeff: f32,
    release_coeff: f32,
}

impl BrickwallLimiter {
    pub fn new(sample_rate: f32) -> Self {
        let delay_len = ((LOOKAHEAD_MS * 0.001 * sample_rate) as usize).max(1);
        let mut limiter = Self {
            sample_rate,
            ceiling_db: -0.3,
            ceiling_lin: db_to_lin(-0.3),
            delay_l: vec![0.0; delay_len],
            delay_r: vec![0.0; delay_len],
            delay_len,
            delay_pos: 0,
            env: 0.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
        };
        limiter.recalc();
        limiter
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        let delay_len = ((LOOKAHEAD_MS * 0.001 * sr) as usize).max(1);
        self.delay_l.resize(delay_len, 0.0);
        self.delay_r.resize(delay_len, 0.0);
        self.delay_len = delay_len;
        self.recalc();
        self.reset();
    }

    pub fn reset(&mut self) {
        self.delay_l.iter_mut().for_each(|s| *s = 0.0);
        self.delay_r.iter_mut().for_each(|s| *s = 0.0);
        self.delay_pos = 0;
        self.env = 0.0;
    }

    /// Call after changing `ceiling_db`.
    pub fn set_ceiling(&mut self, db: f32) {
        self.ceiling_db = db;
        self.ceiling_lin = db_to_lin(db);
    }

    fn recalc(&mut self) {
        self.ceiling_lin = db_to_lin(self.ceiling_db);
        // Attack: very fast, roughly 0.1 ms so we catch peaks within the
        // lookahead window.
        self.attack_coeff = (-1.0 / (0.0001 * self.sample_rate)).exp();
        // Release: moderate, ~100 ms.
        self.release_coeff = (-1.0 / (0.1 * self.sample_rate)).exp();
    }

    /// Process a stereo sample pair. Returns (left, right).
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // --- Stage 1: Soft clip (tanh) ---
        let soft_l = soft_clip(left);
        let soft_r = soft_clip(right);

        // --- Stage 2: Compute desired gain from the *current* (pre-delay)
        //     sample so the gain change is applied to the *delayed* sample. ---
        let peak = soft_l.abs().max(soft_r.abs());
        let desired_gain = if peak > self.ceiling_lin {
            self.ceiling_lin / peak
        } else {
            1.0
        };

        // Smooth envelope.
        let coeff = if desired_gain < self.env {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.env = coeff * self.env + (1.0 - coeff) * desired_gain;
        // Never let the envelope exceed 1.0.
        if self.env > 1.0 {
            self.env = 1.0;
        }

        // --- Stage 3: Read from delay, write new samples. ---
        let out_l = self.delay_l[self.delay_pos];
        let out_r = self.delay_r[self.delay_pos];
        self.delay_l[self.delay_pos] = soft_l;
        self.delay_r[self.delay_pos] = soft_r;
        self.delay_pos += 1;
        if self.delay_pos >= self.delay_len {
            self.delay_pos = 0;
        }

        // Apply gain reduction.
        let limited_l = out_l * self.env;
        let limited_r = out_r * self.env;

        // --- Stage 4: Hard clip safety net. ---
        let final_l = hard_clip(limited_l, self.ceiling_lin);
        let final_r = hard_clip(limited_r, self.ceiling_lin);

        (final_l, final_r)
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Soft clip using tanh. The signal is driven gently so that small signals
/// pass through almost linearly while peaks are rounded off.
#[inline(always)]
fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

#[inline(always)]
fn hard_clip(x: f32, ceil: f32) -> f32 {
    x.clamp(-ceil, ceil)
}

#[inline(always)]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
