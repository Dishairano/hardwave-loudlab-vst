//! Saturation — soft-clipping with tanh, optional DC blocker, wet/dry mix.
//!
//! Placement in the chain: after the multiband compressor, before the stereo
//! width module. This is the standard "glue" position in a mastering chain —
//! the compressor has already evened out the dynamics so saturation hits a
//! consistent level, and the stereo stage doesn't fight the harmonic content
//! the saturator just generated.
//!
//! What this module is NOT (Sprint-3 scope):
//!   - Multi-character (no Tube/Tape/Transformer/Transistor curves yet). One
//!     curve: tanh. It's the most musically neutral soft-clip and sounds
//!     "right" across more genres than any of the named characters at low
//!     drives. Multi-character variants land in Sprint 4 once we have ear-
//!     time to tune them.
//!   - 2× oversampled around the tanh (Oversampler2x; same anti-aliasing
//!     fix as the limiter's soft-clip). Hot/bright material previously
//!     aliased into the top end — fixed in v0.6.13.
//!   - Per-band. One-knob master-bus saturation.
//!
//! Design constraints:
//!   - No allocation on the audio thread. All state is fixed-size.
//!   - Unity drive = unity gain. The normalisation step divides out
//!     tanh(drive_lin) so input=output at the dry/wet endpoints. Without
//!     this, drive=0 dB silently makes things 23% quieter (atanh peculiarity).
//!   - DC blocker is a single-pole HPF at ~5 Hz, applied per channel post-
//!     saturation. Without it, asymmetric input (kicks) creates a slow
//!     DC drift that messes up the limiter's true-peak estimate downstream.

#[derive(Clone, Copy, Debug)]
pub struct SaturationParams {
    /// Drive in dB, applied before tanh. 0 = neutral, 12 = noticeable
    /// harmonic content, 24 = heavy distortion. Range [0, 24] is what the
    /// Rust param exposes; this module accepts anything and clamps.
    pub drive_db: f32,
    /// Dry/wet mix, 0.0 = fully dry (passthrough), 1.0 = fully wet.
    pub mix: f32,
    /// Enable/disable. When false, sample passes through unchanged with
    /// zero CPU cost (single branch).
    pub enabled: bool,
}

impl Default for SaturationParams {
    fn default() -> Self {
        Self {
            drive_db: 0.0,
            mix: 1.0,
            enabled: false,
        }
    }
}

/// Single-pole DC blocker. Removes the asymmetric component that tanh-clipped
/// kicks accumulate over time, before that DC offset reaches the limiter.
#[derive(Clone, Copy)]
struct DcBlocker {
    x1: f32,
    y1: f32,
    coeff: f32,
}

impl DcBlocker {
    fn new(sample_rate: f32) -> Self {
        // Cut-off ~5 Hz: coeff = 1 - (2π * 5 / sr). At 44.1kHz that's ~0.9993.
        let coeff = 1.0 - (2.0 * std::f32::consts::PI * 5.0 / sample_rate);
        Self { x1: 0.0, y1: 0.0, coeff }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.coeff * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.coeff = 1.0 - (2.0 * std::f32::consts::PI * 5.0 / sample_rate);
    }
}

pub struct Saturation {
    params: SaturationParams,
    dc_blocker_l: DcBlocker,
    dc_blocker_r: DcBlocker,
    // 2× oversampling around the tanh — same anti-aliasing fix as the limiter,
    // matters once the user opts in to saturation on hot/bright material.
    os_l: super::oversample::Oversampler2x,
    os_r: super::oversample::Oversampler2x,
}

impl Saturation {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            params: SaturationParams::default(),
            dc_blocker_l: DcBlocker::new(sample_rate),
            dc_blocker_r: DcBlocker::new(sample_rate),
            os_l: super::oversample::Oversampler2x::new(),
            os_r: super::oversample::Oversampler2x::new(),
        }
    }

    pub fn set_params(&mut self, params: SaturationParams) {
        self.params = params;
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.dc_blocker_l.set_sample_rate(sample_rate);
        self.dc_blocker_r.set_sample_rate(sample_rate);
    }

    pub fn reset(&mut self) {
        self.dc_blocker_l.reset();
        self.dc_blocker_r.reset();
        self.os_l.reset();
        self.os_r.reset();
    }

    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.params.enabled || self.params.mix <= 0.0 {
            return (left, right);
        }

        // Convert drive dB → linear gain factor. Clamped at module boundary
        // (the Rust param range is already [0, 24]; this is defense in
        // depth in case set_params is called from a future code path that
        // doesn't enforce the bound).
        let drive_db = self.params.drive_db.clamp(0.0, 36.0);
        let drive_lin = 10.0_f32.powf(drive_db / 20.0);

        // Normalisation factor so unity drive = unity output gain. tanh(g) at
        // small g is approximately g, but as g grows it saturates toward 1.
        // Dividing by tanh(g) restores peak-to-peak parity at the wet/dry
        // crossfade. Clamp to a small epsilon so drive_db → 0 doesn't NaN.
        let norm = drive_lin.tanh().max(1e-6);

        // Process through 2× oversampler — closure captures drive_lin & norm.
        let f = |x: f32| (x * drive_lin).tanh() / norm;
        let mut l_wet = self.os_l.process(left, f);
        let mut r_wet = self.os_r.process(right, f);

        // DC blocker on the wet branch only — the dry branch is already DC-
        // free (assuming the input is).
        l_wet = self.dc_blocker_l.process(l_wet);
        r_wet = self.dc_blocker_r.process(r_wet);

        let mix = self.params.mix.clamp(0.0, 1.0);
        let l_out = left * (1.0 - mix) + l_wet * mix;
        let r_out = right * (1.0 - mix) + r_wet * mix;

        (l_out, r_out)
    }
}
