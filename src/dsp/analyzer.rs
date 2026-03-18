use rustfft::{num_complex::Complex, FftPlanner};
use std::f32::consts::PI;

const FFT_SIZE: usize = 2048;

pub struct SpectrumAnalyzer {
    sample_rate: f32,
    buffer: Vec<f32>,
    write_pos: usize,
    window: Vec<f32>,
    fft_scratch: Vec<Complex<f32>>,
    fft_input: Vec<Complex<f32>>,
    magnitude_db: Vec<f32>,
    frame_ready: bool,
    planner_cache: FftPlannerCache,
}

/// Cached FFT plan so we never allocate during process.
struct FftPlannerCache {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl SpectrumAnalyzer {
    pub fn new(sample_rate: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch_len = fft.get_inplace_scratch_len();

        Self {
            sample_rate,
            buffer: vec![0.0; FFT_SIZE],
            write_pos: 0,
            window: Self::build_hanning_window(),
            fft_scratch: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            fft_input: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            magnitude_db: vec![-120.0; FFT_SIZE / 2],
            frame_ready: false,
            planner_cache: FftPlannerCache {
                fft,
                scratch: vec![Complex::new(0.0, 0.0); scratch_len],
            },
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.buffer.iter_mut().for_each(|s| *s = 0.0);
        self.write_pos = 0;
        self.frame_ready = false;
        self.magnitude_db.iter_mut().for_each(|s| *s = -120.0);
    }

    fn build_hanning_window() -> Vec<f32> {
        (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1.0 - (2.0 * PI * i as f32 / (FFT_SIZE as f32 - 1.0)).cos())
            })
            .collect()
    }

    /// Feed one sample into the ring buffer. When a full frame is collected the
    /// FFT is computed immediately so the result is available via `get_spectrum`.
    pub fn push_sample(&mut self, s: f32) {
        self.buffer[self.write_pos] = s;
        self.write_pos += 1;

        if self.write_pos >= FFT_SIZE {
            self.write_pos = 0;
            self.compute_fft();
            self.frame_ready = true;
        }
    }

    fn compute_fft(&mut self) {
        // Apply window and copy into complex input buffer.
        for i in 0..FFT_SIZE {
            self.fft_input[i] = Complex::new(self.buffer[i] * self.window[i], 0.0);
        }

        // Copy to scratch for in-place transform.
        self.fft_scratch.copy_from_slice(&self.fft_input);

        self.planner_cache
            .fft
            .process_with_scratch(&mut self.fft_scratch, &mut self.planner_cache.scratch);

        // Convert to magnitude in dB (only first half — positive frequencies).
        let norm = 1.0 / FFT_SIZE as f32;
        for i in 0..(FFT_SIZE / 2) {
            let mag = self.fft_scratch[i].norm() * norm;
            self.magnitude_db[i] = if mag > 1e-12 {
                20.0 * mag.log10()
            } else {
                -120.0
            };
        }
    }

    /// Returns `Some(spectrum)` when a new FFT frame has been computed since the
    /// last call, otherwise `None`. The returned `Vec<f32>` contains FFT_SIZE/2
    /// magnitude values in dB.
    pub fn get_spectrum(&mut self) -> Option<Vec<f32>> {
        if self.frame_ready {
            self.frame_ready = false;
            Some(self.magnitude_db.clone())
        } else {
            None
        }
    }

    /// Number of bins in the spectrum (FFT_SIZE / 2).
    pub fn bin_count(&self) -> usize {
        FFT_SIZE / 2
    }

    /// Frequency corresponding to a given bin index.
    pub fn bin_to_freq(&self, bin: usize) -> f32 {
        bin as f32 * self.sample_rate / FFT_SIZE as f32
    }
}
