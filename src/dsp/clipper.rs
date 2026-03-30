/// Hard/soft clipper with optional 2× oversampling for soft modes.
///
/// Sits after the limiter to catch remaining peaks and add controlled
/// harmonic saturation. Three clipping curves:
///   - Hard: brick-wall clamp (zero aliasing, harsh)
///   - Soft (tanh): smooth saturation, rich harmonics
///   - Cubic: polynomial (x - x³/3), warmer than tanh

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipMode {
    Hard = 0,
    SoftTanh = 1,
    SoftCubic = 2,
}

impl ClipMode {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => Self::SoftTanh,
            2 => Self::SoftCubic,
            _ => Self::Hard,
        }
    }
}

pub struct Clipper {
    pub threshold_db: f32,
    threshold_lin: f32,
    pub mode: ClipMode,
    pub oversample: bool,

    // 2× oversampling: previous sample for interpolation
    prev_l: f32,
    prev_r: f32,
}

impl Clipper {
    pub fn new() -> Self {
        Self {
            threshold_db: -0.1,
            threshold_lin: db_to_lin(-0.1),
            mode: ClipMode::Hard,
            oversample: false,
            prev_l: 0.0,
            prev_r: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.prev_l = 0.0;
        self.prev_r = 0.0;
    }

    pub fn set_threshold(&mut self, db: f32) {
        self.threshold_db = db;
        self.threshold_lin = db_to_lin(db);
    }

    pub fn set_mode(&mut self, mode: ClipMode) {
        self.mode = mode;
    }

    pub fn set_oversample(&mut self, on: bool) {
        self.oversample = on;
    }

    /// Clip a single sample according to the current mode and threshold.
    #[inline(always)]
    fn clip_sample(&self, x: f32) -> f32 {
        let t = self.threshold_lin;
        match self.mode {
            ClipMode::Hard => x.clamp(-t, t),
            ClipMode::SoftTanh => {
                if t < 1e-10 {
                    return 0.0;
                }
                let driven = x / t;
                driven.tanh() * t
            }
            ClipMode::SoftCubic => {
                if t < 1e-10 {
                    return 0.0;
                }
                let driven = (x / t).clamp(-1.5, 1.5);
                let out = if driven.abs() > 1.0 {
                    driven.signum() * (2.0 / 3.0)
                } else {
                    driven - (driven * driven * driven) / 3.0
                };
                out * t
            }
        }
    }

    /// Process a stereo sample pair. Returns (left, right).
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        if self.oversample && self.mode != ClipMode::Hard {
            // 2× oversampling: interpolate midpoint, clip both, average
            let mid_l = (self.prev_l + left) * 0.5;
            let mid_r = (self.prev_r + right) * 0.5;
            self.prev_l = left;
            self.prev_r = right;

            let c_mid_l = self.clip_sample(mid_l);
            let c_cur_l = self.clip_sample(left);
            let c_mid_r = self.clip_sample(mid_r);
            let c_cur_r = self.clip_sample(right);

            ((c_mid_l + c_cur_l) * 0.5, (c_mid_r + c_cur_r) * 0.5)
        } else {
            (self.clip_sample(left), self.clip_sample(right))
        }
    }

    /// Return the current gain reduction in dB for metering.
    #[inline]
    pub fn gr_db(&self, input_peak: f32) -> f32 {
        if input_peak <= self.threshold_lin || input_peak < 1e-10 {
            0.0
        } else {
            20.0 * (self.threshold_lin / input_peak).log10()
        }
    }
}

#[inline(always)]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
