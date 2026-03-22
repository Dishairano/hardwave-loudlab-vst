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

    fn low_shelf(freq: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * a.sqrt() * alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * a.sqrt() * alpha);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * a.sqrt() * alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * a.sqrt() * alpha;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    fn high_shelf(freq: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * a.sqrt() * alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * a.sqrt() * alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * a.sqrt() * alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * a.sqrt() * alpha;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    fn low_pass(freq: f32, q: f32, sample_rate: f32) -> Self {
        let w0 = 2.0 * PI * freq / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    fn high_pass(freq: f32, q: f32, sample_rate: f32) -> Self {
        let w0 = 2.0 * PI * freq / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }
}

/// Filter type for each EQ band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterType {
    Peak = 0,
    LowShelf = 1,
    HighShelf = 2,
    LowPass = 3,
    HighPass = 4,
}

impl Default for FilterType {
    fn default() -> Self { FilterType::Peak }
}

impl From<i32> for FilterType {
    fn from(v: i32) -> Self {
        match v { 1 => FilterType::LowShelf, 2 => FilterType::HighShelf, 3 => FilterType::LowPass, 4 => FilterType::HighPass, _ => FilterType::Peak }
    }
}

/// Parameters for a single EQ band.
#[derive(Debug, Clone, Copy)]
pub struct EqBandParams {
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
    pub enabled: bool,
    pub filter_type: FilterType,
}

impl Default for EqBandParams {
    fn default() -> Self {
        Self {
            freq: 1000.0,
            gain_db: 0.0,
            q: 0.707,
            enabled: true,
            filter_type: FilterType::Peak,
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
            EqBandParams { freq: 100.0, gain_db: 0.0, q: 0.707, enabled: true, filter_type: FilterType::Peak },
            EqBandParams { freq: 500.0, gain_db: 0.0, q: 0.707, enabled: true, filter_type: FilterType::Peak },
            EqBandParams { freq: 2000.0, gain_db: 0.0, q: 0.707, enabled: true, filter_type: FilterType::Peak },
            EqBandParams { freq: 8000.0, gain_db: 0.0, q: 0.707, enabled: true, filter_type: FilterType::Peak },
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
        if !b.enabled {
            self.coeffs[i] = BiquadCoeffs::unity();
            return;
        }
        self.coeffs[i] = match b.filter_type {
            FilterType::Peak => {
                if b.gain_db.abs() > 0.001 {
                    BiquadCoeffs::peaking(b.freq, b.gain_db, b.q, self.sample_rate)
                } else {
                    BiquadCoeffs::unity()
                }
            }
            FilterType::LowShelf => BiquadCoeffs::low_shelf(b.freq, b.gain_db, b.q, self.sample_rate),
            FilterType::HighShelf => BiquadCoeffs::high_shelf(b.freq, b.gain_db, b.q, self.sample_rate),
            FilterType::LowPass => BiquadCoeffs::low_pass(b.freq, b.q, self.sample_rate),
            FilterType::HighPass => BiquadCoeffs::high_pass(b.freq, b.q, self.sample_rate),
        };
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
