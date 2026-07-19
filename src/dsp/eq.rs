use std::f32::consts::PI;

/// Second-order biquad filter state.
#[derive(Clone, Copy)]
struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadState {
    fn new() -> Self {
        Self {
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Biquad coefficients in Direct Form I.
#[derive(Clone, Copy)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoeffs {
    /// Peaking EQ (parametric bell) coefficients.
    /// `freq` in Hz, `gain_db` in dB, `q` is bandwidth Q.
    fn peaking(freq: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Pass-through (unity) coefficients.
    fn unity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

/// Parameters for a single EQ band.
#[derive(Debug, Clone, Copy)]
pub struct EqBandParams {
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
    pub enabled: bool,
}

impl Default for EqBandParams {
    fn default() -> Self {
        Self {
            freq: 1000.0,
            gain_db: 0.0,
            q: 0.707,
            enabled: true,
        }
    }
}

/// 4-band parametric EQ using cascaded biquad filters.
pub struct ParametricEq {
    sample_rate: f32,
    bands: [EqBandParams; 4],
    coeffs: [BiquadCoeffs; 4],
    states: [BiquadState; 4],
}

impl ParametricEq {
    pub fn new(sample_rate: f32) -> Self {
        let default_bands = [
            EqBandParams { freq: 100.0, gain_db: 0.0, q: 0.707, enabled: true },
            EqBandParams { freq: 500.0, gain_db: 0.0, q: 0.707, enabled: true },
            EqBandParams { freq: 2000.0, gain_db: 0.0, q: 0.707, enabled: true },
            EqBandParams { freq: 8000.0, gain_db: 0.0, q: 0.707, enabled: true },
        ];
        let mut eq = Self {
            sample_rate,
            bands: default_bands,
            coeffs: [BiquadCoeffs::unity(); 4],
            states: [BiquadState::new(); 4],
        };
        eq.recalc_all();
        eq
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        self.recalc_all();
        self.reset();
    }

    pub fn reset(&mut self) {
        for s in self.states.iter_mut() {
            s.reset();
        }
    }

    /// Update a single band's parameters by index (0..3).
    pub fn set_band(&mut self, index: usize, params: EqBandParams) {
        debug_assert!(index < 4);
        self.bands[index] = params;
        self.recalc_band(index);
    }

    /// Get current parameters for a band.
    pub fn get_band(&self, index: usize) -> EqBandParams {
        self.bands[index]
    }

    fn recalc_band(&mut self, i: usize) {
        let b = &self.bands[i];
        if b.enabled && b.gain_db.abs() > 0.001 {
            self.coeffs[i] = BiquadCoeffs::peaking(b.freq, b.gain_db, b.q, self.sample_rate);
        } else {
            self.coeffs[i] = BiquadCoeffs::unity();
        }
    }

    fn recalc_all(&mut self) {
        for i in 0..4 {
            self.recalc_band(i);
        }
    }

    /// Process a single sample through all 4 bands in series.
    pub fn process(&mut self, sample: f32) -> f32 {
        let mut out = sample;
        for i in 0..4 {
            out = self.process_biquad(i, out);
        }
        out
    }

    #[inline(always)]
    fn process_biquad(&mut self, i: usize, x: f32) -> f32 {
        let c = &self.coeffs[i];
        let s = &mut self.states[i];

        let y = c.b0 * x + c.b1 * s.x1 + c.b2 * s.x2 - c.a1 * s.y1 - c.a2 * s.y2;

        s.x2 = s.x1;
        s.x1 = x;
        s.y2 = s.y1;
        s.y1 = y;

        y
    }
}

/// Dedicated "Sub" control — a broad, low peaking bell (~45 Hz, wide Q) a
/// producer rides to push or pull the sub. Stereo (independent L/R state).
/// Runs as a user macro on top of everything else, independent of the 4-band
/// auto EQ, so it never fights the genre target. 0 dB = pass-through.
pub struct SubShelf {
    coeffs: BiquadCoeffs,
    state_l: BiquadState,
    state_r: BiquadState,
    sample_rate: f32,
}

impl SubShelf {
    const FREQ: f32 = 45.0;
    const Q: f32 = 0.6;

    pub fn new(sample_rate: f32) -> Self {
        Self {
            coeffs: BiquadCoeffs::unity(),
            state_l: BiquadState::new(),
            state_r: BiquadState::new(),
            sample_rate,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    /// Set the sub gain in dB. 0 dB collapses to unity (true pass-through).
    pub fn set_gain(&mut self, gain_db: f32) {
        self.coeffs = if gain_db.abs() > 0.001 {
            BiquadCoeffs::peaking(Self::FREQ, gain_db, Self::Q, self.sample_rate)
        } else {
            BiquadCoeffs::unity()
        };
    }

    pub fn reset(&mut self) {
        self.state_l.reset();
        self.state_r.reset();
    }

    #[inline(always)]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (
            Self::run(&self.coeffs, &mut self.state_l, l),
            Self::run(&self.coeffs, &mut self.state_r, r),
        )
    }

    #[inline(always)]
    fn run(c: &BiquadCoeffs, s: &mut BiquadState, x: f32) -> f32 {
        let y = c.b0 * x + c.b1 * s.x1 + c.b2 * s.x2 - c.a1 * s.y1 - c.a2 * s.y2;
        s.x2 = s.x1;
        s.x1 = x;
        s.y2 = s.y1;
        s.y1 = y;
        y
    }
}
