/// Brickwall lookahead limiter with a soft-clip input stage.
///
/// Signal flow:
///   1. Soft-clip (tanh-based) to tame transients before the limiter.
///   2. Lookahead delay (5 ms) — the gain envelope is computed ahead of time so
///      the limiter can smoothly attenuate peaks before they arrive.
///   3. Gain reduction with attack/release smoothing.
///   4. Hard clip at ceiling as a safety net.

pub const LOOKAHEAD_MS: f32 = 5.0;

/// Ceiling values the limiter itself will accept, in dB. The `limiter_ceiling`
/// parameter declares -6..0 dB; this is the wider bound the DSP trusts, so a
/// value that reaches the setter from somewhere other than that parameter still
/// lands on a usable ceiling instead of a degenerate one.
const MIN_CEILING_DB: f32 = -60.0;
const MAX_CEILING_DB: f32 = 0.0;

/// Ceiling used when the value handed to `set_ceiling` is not a real number.
/// Matches the `limiter_ceiling` parameter's own default.
const FALLBACK_CEILING_DB: f32 = -1.0;

/// Pull a ceiling into the range the limiter can work with.
///
/// A NaN ceiling used to reach `hard_clip` as a clamp bound, and `f32::clamp`
/// panics when a bound is NaN. That panic happens inside `Plugin::process`, so
/// it crosses the FFI boundary and takes the host process down with it.
#[inline]
fn sanitize_ceiling_db(db: f32) -> f32 {
    if db.is_finite() {
        db.clamp(MIN_CEILING_DB, MAX_CEILING_DB)
    } else {
        FALLBACK_CEILING_DB
    }
}

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

    // 2× oversampling for the tanh soft-clip, per channel — kills the high-end
    // aliasing that made hot/bright material sound gritty ("192 kbps" highs).
    clip_os_l: super::oversample::Oversampler2x,
    clip_os_r: super::oversample::Oversampler2x,
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
            clip_os_l: super::oversample::Oversampler2x::new(),
            clip_os_r: super::oversample::Oversampler2x::new(),
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
        self.clip_os_l.reset();
        self.clip_os_r.reset();
    }

    /// Call after changing `ceiling_db`.
    pub fn set_ceiling(&mut self, db: f32) {
        self.ceiling_db = sanitize_ceiling_db(db);
        self.ceiling_lin = db_to_lin(self.ceiling_db);
    }

    fn recalc(&mut self) {
        self.ceiling_db = sanitize_ceiling_db(self.ceiling_db);
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
        // --- Stage 1: Soft clip (tanh), oversampled 2× to avoid aliasing ---
        let soft_l = self.clip_os_l.process(left, soft_clip);
        let soft_r = self.clip_os_r.process(right, soft_clip);

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

/// Hard clip to `±ceil`.
///
/// Written with `max`/`min` rather than `clamp` on purpose: `f32::clamp` panics
/// when either bound is NaN, and nothing in the audio path may panic.
/// `set_ceiling` already refuses a non-finite ceiling, so this is the second
/// line — a future caller cannot reintroduce the host crash from here.
///
/// A sample that arrives non-finite is flushed to silence rather than passed
/// on: this is the last stage before the output bus, and a NaN leaving the
/// plug-in poisons every meter and every plug-in after it on the master.
#[inline(always)]
fn hard_clip(x: f32, ceil: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    x.max(-ceil).min(ceil)
}

#[inline(always)]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ceiling that is not a real number must not reach the audio path. A
    /// saved project, a preset or a host automation lane can hand the ceiling
    /// parameter a NaN, and `f32::clamp` panics when either bound is NaN — a
    /// panic inside `process()` crosses the FFI boundary and takes the host
    /// down with it.
    #[test]
    fn nan_ceiling_does_not_panic() {
        let mut limiter = BrickwallLimiter::new(48_000.0);
        limiter.set_ceiling(f32::NAN);
        for n in 0..512 {
            let x = 0.5 * (n as f32 * 0.01).sin();
            let (l, r) = limiter.process(x, x);
            assert!(l.is_finite(), "left output went non-finite at {n}: {l}");
            assert!(r.is_finite(), "right output went non-finite at {n}: {r}");
        }
    }

    /// The same for the infinities and for a value so far outside the
    /// parameter's own -6..0 dB range that `10^(db/20)` overflows or
    /// underflows.
    #[test]
    fn extreme_ceilings_stay_finite_and_bounded() {
        for db in [f32::INFINITY, f32::NEG_INFINITY, 1.0e38, -1.0e38, 0.0] {
            let mut limiter = BrickwallLimiter::new(48_000.0);
            limiter.set_ceiling(db);
            for n in 0..512 {
                let x = 4.0 * (n as f32 * 0.31).sin();
                let (l, r) = limiter.process(x, x);
                assert!(l.is_finite(), "ceiling {db}: left non-finite at {n}: {l}");
                assert!(r.is_finite(), "ceiling {db}: right non-finite at {n}: {r}");
                assert!(
                    l.abs() <= 1.0 && r.abs() <= 1.0,
                    "ceiling {db}: output above full scale at {n}: {l}, {r}"
                );
            }
        }
    }
}
