use std::f32::consts::PI;

// ---------------------------------------------------------------------------
// Simple 1st-order lowpass for mono-bass crossover.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct OnePole {
    coeff: f32,
    state: f32,
}

impl OnePole {
    fn new() -> Self {
        Self {
            coeff: 0.0,
            state: 0.0,
        }
    }

    fn set_freq(&mut self, freq: f32, sr: f32) {
        let w = (PI * freq / sr).tan();
        self.coeff = w / (1.0 + w);
    }

    fn reset(&mut self) {
        self.state = 0.0;
    }

    /// Returns (lowpass, highpass).
    #[inline(always)]
    fn process(&mut self, x: f32) -> (f32, f32) {
        let lp = self.state + self.coeff * (x - self.state);
        self.state = lp;
        (lp, x - lp)
    }
}

// ---------------------------------------------------------------------------
// Mid/Side stereo processor.
// ---------------------------------------------------------------------------

pub struct StereoProcessor {
    sample_rate: f32,
    /// Width control: 0 = mono, 1 = normal, 2 = extra wide.
    pub width: f32,
    /// Frequency below which the signal is summed to mono.
    pub mono_bass_freq: f32,
    /// Enable mono-bass processing.
    pub bass_mono: bool,

    // Internal lowpass filters for mono bass (one per channel: mid & side).
    lp_mid: OnePole,
    lp_side: OnePole,
}

impl StereoProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let mut sp = Self {
            sample_rate,
            width: 1.0,
            mono_bass_freq: 200.0,
            bass_mono: true,
            lp_mid: OnePole::new(),
            lp_side: OnePole::new(),
        };
        sp.update_filters();
        sp
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        self.update_filters();
        self.reset();
    }

    pub fn reset(&mut self) {
        self.lp_mid.reset();
        self.lp_side.reset();
    }

    /// Call after changing `mono_bass_freq`.
    pub fn update_filters(&mut self) {
        self.lp_mid.set_freq(self.mono_bass_freq, self.sample_rate);
        self.lp_side.set_freq(self.mono_bass_freq, self.sample_rate);
    }

    /// Process one stereo sample pair. Returns (left, right).
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // Encode to mid/side.
        let mid = (left + right) * 0.5;
        let side = (left - right) * 0.5;

        let (out_mid, out_side);

        if self.bass_mono {
            // Split side signal into low and high.
            let (_side_low, side_high) = self.lp_side.process(side);
            // Split mid signal (needed to keep phase alignment).
            let (mid_low, mid_high) = self.lp_mid.process(mid);

            // Below the crossover, kill the side (mono bass).
            // Above the crossover, apply width to the side.
            out_mid = mid_low + mid_high;
            out_side = side_high * self.width;
        } else {
            out_mid = mid;
            out_side = side * self.width;
        }

        // Decode back to L/R.
        let out_l = out_mid + out_side;
        let out_r = out_mid - out_side;

        (out_l, out_r)
    }
}
