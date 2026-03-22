/// Brickwall lookahead limiter with a soft-clip input stage.
///
/// Signal flow:
///   1. Soft-clip (character-controlled tanh blend) to tame transients.
///   2. Lookahead delay (5 ms) — gain envelope is computed from the
///      pre-delay signal so limiting starts before peaks arrive.
///   3. Gain reduction triggered when signal exceeds `threshold_lin`,
///      with attack/release smoothing.
///   4. Hard clip at `ceiling_lin` as a safety net.

const LOOKAHEAD_MS: f32 = 5.0;

pub struct BrickwallLimiter {
    sample_rate: f32,

    /// Output ceiling in dB (e.g. -0.1).  Hard clip target.
    pub ceiling_db: f32,
    ceiling_lin: f32,

    /// Gain-reduction threshold in dB (e.g. -6.0).
    /// Limiting kicks in when the signal exceeds this level.
    pub threshold_db: f32,
    threshold_lin: f32,

    /// Soft-clip character: 0 = transparent (linear), 1 = full tanh saturation.
    pub character: f32,

    // Lookahead delay lines (stereo).
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    delay_len: usize,
    delay_pos: usize,

    // Envelope follower state (1.0 = no gain reduction).
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
            threshold_db: -6.0,
            threshold_lin: db_to_lin(-6.0),
            character: 0.5,
            delay_l: vec![0.0; delay_len],
            delay_r: vec![0.0; delay_len],
            delay_len,
            delay_pos: 0,
            env: 1.0, // start at unity — no gain reduction
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
        self.env = 1.0; // reset to unity, not 0 — avoids startup muting
    }

    pub fn set_ceiling(&mut self, db: f32) {
        self.ceiling_db = db;
        self.ceiling_lin = db_to_lin(db);
    }

    pub fn set_threshold(&mut self, db: f32) {
        self.threshold_db = db;
        self.threshold_lin = db_to_lin(db);
    }

    pub fn set_character(&mut self, character: f32) {
        self.character = character.clamp(0.0, 1.0);
    }

    fn recalc(&mut self) {
        self.ceiling_lin = db_to_lin(self.ceiling_db);
        self.threshold_lin = db_to_lin(self.threshold_db);
        // Attack: very fast (~0.1 ms) so peaks are caught within the lookahead window.
        self.attack_coeff = (-1.0 / (0.0001 * self.sample_rate)).exp();
        // Release: moderate (~100 ms).
        self.release_coeff = (-1.0 / (0.1 * self.sample_rate)).exp();
    }

    /// Process a stereo sample pair. Returns (left, right).
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // --- Stage 1: Soft-clip blend controlled by character ---
        let sc_l = soft_clip_blend(left, self.character);
        let sc_r = soft_clip_blend(right, self.character);

        // --- Stage 2: Compute desired gain from pre-delay signal.
        //     Gain reduction triggers when signal exceeds threshold_lin,
        //     targeting ceiling_lin as the output level. ---
        let peak = sc_l.abs().max(sc_r.abs());
        let desired_gain = if peak > self.threshold_lin {
            (self.ceiling_lin / peak).min(1.0)
        } else {
            1.0
        };

        // Smooth envelope — attack fast, release slow.
        let coeff = if desired_gain < self.env {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.env = coeff * self.env + (1.0 - coeff) * desired_gain;
        self.env = self.env.min(1.0);

        // --- Stage 3: Read from delay, write new sample. ---
        let out_l = self.delay_l[self.delay_pos];
        let out_r = self.delay_r[self.delay_pos];
        self.delay_l[self.delay_pos] = sc_l;
        self.delay_r[self.delay_pos] = sc_r;
        self.delay_pos += 1;
        if self.delay_pos >= self.delay_len {
            self.delay_pos = 0;
        }

        // Apply gain reduction to the delayed sample.
        let limited_l = out_l * self.env;
        let limited_r = out_r * self.env;

        // --- Stage 4: Hard-clip safety net at ceiling. ---
        let final_l = limited_l.clamp(-self.ceiling_lin, self.ceiling_lin);
        let final_r = limited_r.clamp(-self.ceiling_lin, self.ceiling_lin);

        (final_l, final_r)
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Blend between linear passthrough (character=0) and tanh saturation (character=1).
#[inline(always)]
fn soft_clip_blend(x: f32, character: f32) -> f32 {
    if character < 0.001 {
        return x;
    }
    let clipped = x.tanh();
    x * (1.0 - character) + clipped * character
}

#[inline(always)]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
