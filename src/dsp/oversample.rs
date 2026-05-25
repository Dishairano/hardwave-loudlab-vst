//! 2× oversampling for a memoryless nonlinearity (the limiter's tanh soft-clip).
//!
//! Why: applying `tanh` at the base rate generates harmonics above Nyquist that
//! fold back as aliasing — audible as a gritty, "low-quality"/"192 kbps" top end
//! on hot, bright harder-styles material (the founders feedback). Running the
//! nonlinearity at 2× and band-limiting removes most of that fold-back.
//!
//! Design: a single windowed-sinc (Hann) FIR lowpass with cutoff at the base
//! Nyquist (0.25 cycles/sample at the 2× rate). Used twice — once to band-limit
//! the zero-stuffed up-sampled stream (anti-imaging, with +6 dB to compensate
//! the zero-stuff), once to band-limit before decimation (anti-aliasing).
//! Deliberately a plain full-rate convolution rather than a polyphase split:
//! at a 32-tap kernel it's a handful of MACs per sample on the master bus, and
//! it's far harder to get wrong (no phase-assignment bugs). Linear phase, so it
//! only adds a fixed delay (TAPS/4 samples at the base rate) — see LATENCY.

/// FIR length (even). 32 taps gives a usable stopband for a soft-clip; cheap
/// enough for a master-bus stage.
const TAPS: usize = 32;

/// Group delay this adds, in *base-rate* samples (linear-phase FIR delay
/// (TAPS-1)/2 at the 2× rate ≈ TAPS/4 at the base rate). Caller can fold this
/// into plugin latency reporting if exact PDC matters.
#[allow(dead_code)] // exposed for folding into plugin PDC reporting later
pub const LATENCY_SAMPLES: usize = TAPS / 4;

pub struct Oversampler2x {
    /// Anti-aliasing/imaging kernel, normalised to unity DC gain.
    h: [f32; TAPS],
    /// Zero-stuffed up-sample history at the 2× rate (ring buffer).
    up_hist: [f32; TAPS],
    up_pos: usize,
    /// Post-nonlinearity history at the 2× rate (ring buffer) for decimation.
    down_hist: [f32; TAPS],
    down_pos: usize,
}

impl Default for Oversampler2x {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler2x {
    pub fn new() -> Self {
        // Windowed-sinc lowpass, cutoff fc = 0.25 cycles/sample (= base Nyquist
        // at the 2× rate). h[n] = 2·fc·sinc(2·fc·(n-c)) · Hann(n), DC-normalised.
        let fc = 0.25_f32;
        let c = (TAPS as f32 - 1.0) / 2.0;
        let mut h = [0.0_f32; TAPS];
        let mut sum = 0.0_f32;
        for (n, hn) in h.iter_mut().enumerate() {
            let m = n as f32 - c;
            let sinc = if m.abs() < 1e-6 {
                2.0 * fc
            } else {
                (2.0 * std::f32::consts::PI * fc * m).sin() / (std::f32::consts::PI * m)
            };
            // Hann window.
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / (TAPS as f32 - 1.0)).cos();
            *hn = sinc * w;
            sum += *hn;
        }
        // Normalise to unity DC gain.
        for hn in h.iter_mut() {
            *hn /= sum;
        }
        Self {
            h,
            up_hist: [0.0; TAPS],
            up_pos: 0,
            down_hist: [0.0; TAPS],
            down_pos: 0,
        }
    }

    /// FIR output over a ring buffer whose newest sample is at `pos-1`.
    #[inline(always)]
    fn fir(hist: &[f32; TAPS], pos: usize, h: &[f32; TAPS]) -> f32 {
        let mut acc = 0.0_f32;
        // hist[(pos-1-k) mod TAPS] aligns with h[k] (newest ↔ h[0]).
        for k in 0..TAPS {
            let idx = (pos + TAPS - 1 - k) % TAPS;
            acc += hist[idx] * h[k];
        }
        acc
    }

    #[inline]
    fn push(hist: &mut [f32; TAPS], pos: &mut usize, x: f32) {
        hist[*pos] = x;
        *pos = (*pos + 1) % TAPS;
    }

    /// Process one base-rate sample through `f` at 2×. `f` is the memoryless
    /// nonlinearity (e.g. `|x| x.tanh()`).
    #[inline]
    pub fn process<F: Fn(f32) -> f32>(&mut self, x: f32, f: F) -> f32 {
        // --- Upsample: zero-stuff [x, 0], band-limit with +6 dB (×2) ---
        Self::push(&mut self.up_hist, &mut self.up_pos, x);
        let u0 = 2.0 * Self::fir(&self.up_hist, self.up_pos, &self.h);
        Self::push(&mut self.up_hist, &mut self.up_pos, 0.0);
        let u1 = 2.0 * Self::fir(&self.up_hist, self.up_pos, &self.h);

        // --- Nonlinearity at the 2× rate ---
        let w0 = f(u0);
        let w1 = f(u1);

        // --- Downsample: band-limit then decimate (keep the 2nd phase) ---
        Self::push(&mut self.down_hist, &mut self.down_pos, w0);
        let _ = Self::fir(&self.down_hist, self.down_pos, &self.h);
        Self::push(&mut self.down_hist, &mut self.down_pos, w1);
        Self::fir(&self.down_hist, self.down_pos, &self.h)
    }

    pub fn reset(&mut self) {
        self.up_hist = [0.0; TAPS];
        self.down_hist = [0.0; TAPS];
        self.up_pos = 0;
        self.down_pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tanh(x: f32) -> f32 {
        x.tanh()
    }

    /// Goertzel single-bin power at normalised frequency `f` (cycles/sample).
    fn goertzel(samples: &[f32], f: f32) -> f32 {
        let w = 2.0 * std::f32::consts::PI * f;
        let coeff = 2.0 * w.cos();
        let (mut s0, mut s1, mut s2) = (0.0f32, 0.0f32, 0.0f32);
        for &x in samples {
            s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).abs() / samples.len() as f32
    }

    #[test]
    fn unity_at_low_level_and_dc() {
        // A quiet signal is in tanh's near-linear region — oversampled output
        // should track the input closely (allowing for FIR group delay).
        let mut os = Oversampler2x::new();
        let mut out = Vec::new();
        for n in 0..2048 {
            let x = 0.05 * (2.0 * std::f32::consts::PI * 0.01 * n as f32).sin();
            out.push(os.process(x, tanh));
        }
        // Energy must be preserved within a few % (band-limited, low level).
        let ein: f32 = (0..2048).map(|n| {
            let x = 0.05 * (2.0 * std::f32::consts::PI * 0.01 * n as f32).sin();
            x * x
        }).sum();
        let eout: f32 = out.iter().map(|y| y * y).sum();
        let ratio = eout / ein;
        assert!(ratio > 0.9 && ratio < 1.1, "low-level energy ratio {ratio} out of band");
    }

    #[test]
    fn no_nan_on_hot_signal() {
        let mut os = Oversampler2x::new();
        for n in 0..4096 {
            let x = 4.0 * (2.0 * std::f32::consts::PI * 0.4 * n as f32).sin();
            let y = os.process(x, tanh);
            assert!(y.is_finite(), "non-finite output at {n}: {y}");
            assert!(y.abs() <= 1.2, "soft-clip output exceeded ~unity: {y}");
        }
    }

    #[test]
    fn reduces_aliasing_vs_naive() {
        // Drive a hot sine near Nyquist (0.40 cyc/sample). tanh makes 3f, 5f…;
        // at the base rate those fold back to audible alias bins. The 3rd
        // harmonic 1.2 folds to |1.2-2*0.5*... | → alias near 0.2 cyc/sample.
        let f = 0.40_f32;
        let amp = 2.0_f32;
        let n = 8192;
        let naive: Vec<f32> = (0..n)
            .map(|i| tanh(amp * (2.0 * std::f32::consts::PI * f * i as f32).sin()))
            .collect();
        let mut os = Oversampler2x::new();
        let over: Vec<f32> = (0..n)
            .map(|i| os.process(amp * (2.0 * std::f32::consts::PI * f * i as f32).sin(), tanh))
            .collect();
        // Probe the alias bin (3rd-harmonic fold-back ≈ 0.20 cyc/sample).
        let alias = 0.20_f32;
        let a_naive = goertzel(&naive[1024..], alias);
        let a_over = goertzel(&over[1024..], alias);
        assert!(
            a_over < a_naive * 0.5,
            "oversampling should cut alias energy by >2×: naive={a_naive:e} over={a_over:e}"
        );
    }
}
