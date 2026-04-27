//! Pure-logic FFT pipeline.
//!
//! The analyser holds a reusable buffer + Hann window + FFT plan so the
//! per-frame cost reduces to a memcpy, a multiply-add across the window,
//! and an in-place FFT.
//!
//! Band aggregation is log-spaced across `[20 Hz, nyquist]`. Each band
//! reports the peak bin magnitude in dBFS over its range, which reads
//! better on a visualiser than averaging (the peak tracks transients).

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use super::types::{SpectrumFrame, DEFAULT_BAND_COUNT, DEFAULT_FFT_SIZE, DEFAULT_SAMPLE_RATE};

/// Log-spaced band aggregator configuration.
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    pub fft_size: usize,
    pub sample_rate: u32,
    pub band_count: usize,
    pub min_hz: f32,
    pub max_hz: f32,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            fft_size: DEFAULT_FFT_SIZE,
            sample_rate: DEFAULT_SAMPLE_RATE,
            band_count: DEFAULT_BAND_COUNT,
            min_hz: 20.0,
            max_hz: 20_000.0,
        }
    }
}

/// One-shot FFT + banding helper. Created once per capture session and
/// fed successive windows of samples.
pub struct Analyzer {
    cfg: AnalyzerConfig,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    scratch: Vec<Complex32>,
    band_boundaries: Vec<(usize, usize)>,
    seq: u64,
}

impl Analyzer {
    #[must_use]
    pub fn new(cfg: AnalyzerConfig) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(cfg.fft_size);
        let window = hann_window(cfg.fft_size);
        let scratch = vec![Complex32::new(0.0, 0.0); cfg.fft_size];
        let band_boundaries = log_band_boundaries(&cfg);
        Self {
            cfg,
            fft,
            window,
            scratch,
            band_boundaries,
            seq: 0,
        }
    }

    /// Process one FFT window and return the resulting frame. `samples`
    /// must contain exactly `fft_size` values; anything shorter / longer
    /// triggers a panic because it is always a programming error at the
    /// call site.
    pub fn process(&mut self, samples: &[f32]) -> SpectrumFrame {
        assert_eq!(
            samples.len(),
            self.cfg.fft_size,
            "analyzer fed wrong-sized window",
        );

        // Windowed copy into the scratch buffer.
        for (i, &s) in samples.iter().enumerate() {
            self.scratch[i] = Complex32::new(s * self.window[i], 0.0);
        }

        self.fft.process(&mut self.scratch);

        // Magnitude spectrum, normalised for the Hann window sum so a
        // full-scale tone at bin `k` reads close to 0 dBFS.
        let norm = 2.0 / self.window.iter().sum::<f32>();
        let half = self.cfg.fft_size / 2;
        let mut mag = vec![0.0_f32; half];
        for (i, m) in mag.iter_mut().enumerate() {
            *m = self.scratch[i].norm() * norm;
        }

        let bands_db: Vec<f32> = self
            .band_boundaries
            .iter()
            .map(|&(lo, hi)| {
                let peak = mag[lo..=hi.min(half - 1)]
                    .iter()
                    .copied()
                    .fold(0.0_f32, f32::max);
                amplitude_to_db(peak)
            })
            .collect();

        let rms_db = rms_dbfs(samples);
        let peak_db = peak_dbfs(samples);

        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);

        SpectrumFrame {
            bands_db,
            rms_db,
            peak_db,
            seq,
        }
    }

    #[must_use]
    pub fn config(&self) -> &AnalyzerConfig {
        &self.cfg
    }
}

/// Allocate a Hann window of length `n`.
#[must_use]
pub fn hann_window(n: usize) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }
    let denom = (n - 1) as f32;
    (0..n)
        .map(|i| 0.5 - 0.5 * ((std::f32::consts::TAU * i as f32) / denom).cos())
        .collect()
}

/// Convert a linear magnitude (already FFT-normalised) to dBFS, clamped
/// to `-120 dB` so downstream consumers never see `-inf`.
#[must_use]
pub fn amplitude_to_db(mag: f32) -> f32 {
    if mag <= 1e-6 {
        -120.0
    } else {
        20.0 * mag.log10()
    }
}

/// RMS of a time-domain frame in dBFS, clamped at `-120 dB`.
#[must_use]
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -120.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    let rms = (sum_sq / samples.len() as f32).sqrt();
    amplitude_to_db(rms)
}

/// Peak of a time-domain frame in dBFS — the loudest absolute sample.
/// Clamped at `-120 dB` for silence.
#[must_use]
pub fn peak_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -120.0;
    }
    let peak = samples.iter().fold(0.0_f32, |acc, s| acc.max(s.abs()));
    amplitude_to_db(peak)
}

/// Compute `(low_bin, high_bin)` boundaries for `band_count` log-spaced
/// bands from `min_hz` to `max_hz` (inclusive on low end, exclusive on
/// the high end except for the last band which extends to Nyquist).
fn log_band_boundaries(cfg: &AnalyzerConfig) -> Vec<(usize, usize)> {
    let bins = cfg.fft_size / 2;
    let nyquist = f64::from(cfg.sample_rate) / 2.0;
    let min = f64::from(cfg.min_hz).max(1.0);
    let max = f64::from(cfg.max_hz).min(nyquist);
    let log_min = min.log10();
    let log_max = max.log10();
    let band_count = cfg.band_count.max(1);

    let mut out = Vec::with_capacity(band_count);
    let mut last_hi = 0;
    for i in 0..band_count {
        let t0 = i as f64 / band_count as f64;
        let t1 = (i + 1) as f64 / band_count as f64;
        let f0 = 10_f64.powf(log_min + t0 * (log_max - log_min));
        let f1 = 10_f64.powf(log_min + t1 * (log_max - log_min));

        let lo_bin = (f0 * cfg.fft_size as f64 / f64::from(cfg.sample_rate)).floor() as usize;
        let hi_bin = (f1 * cfg.fft_size as f64 / f64::from(cfg.sample_rate)).floor() as usize;

        // Guarantee monotonic coverage: each band owns at least one bin
        // and never overlaps with the previous one.
        let lo = lo_bin.max(last_hi).min(bins.saturating_sub(1));
        let hi = hi_bin.max(lo).min(bins.saturating_sub(1));
        out.push((lo, hi));
        last_hi = hi.saturating_add(1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_window_endpoints_are_zero() {
        let w = hann_window(16);
        assert!((w[0]).abs() < 1e-6);
        assert!((w[15]).abs() < 1e-6);
    }

    #[test]
    fn hann_window_peaks_at_centre() {
        let w = hann_window(32);
        let mid = w[15].max(w[16]);
        assert!(mid > 0.99);
    }

    #[test]
    fn hann_window_handles_degenerate_sizes() {
        assert!(hann_window(0).is_empty());
        assert_eq!(hann_window(1), vec![1.0]);
    }

    #[test]
    fn amplitude_to_db_clamps_silence() {
        assert!((amplitude_to_db(0.0) - -120.0).abs() < 1e-6);
        assert!((amplitude_to_db(1e-9) - -120.0).abs() < 1e-6);
    }

    #[test]
    fn amplitude_to_db_full_scale_is_zero_db() {
        assert!((amplitude_to_db(1.0)).abs() < 1e-6);
    }

    #[test]
    fn rms_dbfs_silence_is_clamped() {
        let zeros = vec![0.0_f32; 1024];
        assert!((rms_dbfs(&zeros) - -120.0).abs() < 1e-6);
        let empty: &[f32] = &[];
        assert!((rms_dbfs(empty) - -120.0).abs() < 1e-6);
    }

    #[test]
    fn rms_dbfs_full_scale_tone_near_minus_three() {
        // A full-scale square wave has RMS = 1.0 → 0 dBFS.
        let square: Vec<f32> = (0..1024)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        assert!((rms_dbfs(&square)).abs() < 0.01);
    }

    #[test]
    fn peak_dbfs_handles_silence_and_full_scale() {
        let zeros = vec![0.0_f32; 512];
        assert!((peak_dbfs(&zeros) - -120.0).abs() < 1e-6);

        let mut s = vec![0.1_f32; 512];
        s[200] = -0.9;
        // peak = 0.9 → ≈ -0.915 dBFS
        let db = peak_dbfs(&s);
        assert!((db - -0.915).abs() < 0.01);
    }

    #[test]
    fn log_band_boundaries_span_full_count() {
        let cfg = AnalyzerConfig::default();
        let bands = log_band_boundaries(&cfg);
        assert_eq!(bands.len(), cfg.band_count);
    }

    #[test]
    fn log_band_boundaries_are_non_overlapping_and_non_decreasing() {
        let cfg = AnalyzerConfig::default();
        let bands = log_band_boundaries(&cfg);
        for window in bands.windows(2) {
            let (_, prev_hi) = window[0];
            let (next_lo, _) = window[1];
            assert!(next_lo >= prev_hi, "overlap detected: {window:?}");
        }
    }

    #[test]
    fn analyzer_emits_expected_number_of_bands() {
        let cfg = AnalyzerConfig {
            fft_size: 1024,
            sample_rate: 48_000,
            band_count: 16,
            min_hz: 20.0,
            max_hz: 20_000.0,
        };
        let mut a = Analyzer::new(cfg.clone());
        let samples = vec![0.0_f32; cfg.fft_size];
        let frame = a.process(&samples);
        assert_eq!(frame.bands_db.len(), cfg.band_count);
    }

    #[test]
    fn analyzer_sequence_counter_advances_monotonically() {
        let cfg = AnalyzerConfig {
            fft_size: 256,
            sample_rate: 48_000,
            band_count: 8,
            min_hz: 20.0,
            max_hz: 20_000.0,
        };
        let mut a = Analyzer::new(cfg.clone());
        let silence = vec![0.0_f32; cfg.fft_size];
        let f1 = a.process(&silence);
        let f2 = a.process(&silence);
        let f3 = a.process(&silence);
        assert_eq!(f1.seq, 0);
        assert_eq!(f2.seq, 1);
        assert_eq!(f3.seq, 2);
    }

    #[test]
    fn analyzer_detects_strong_tone_in_expected_band() {
        // Synthesise a 1 kHz sine at sample rate 48 kHz.
        let cfg = AnalyzerConfig {
            fft_size: 2048,
            sample_rate: 48_000,
            band_count: 32,
            min_hz: 20.0,
            max_hz: 20_000.0,
        };
        let mut a = Analyzer::new(cfg.clone());
        let freq = 1000.0_f32;
        let samples: Vec<f32> = (0..cfg.fft_size)
            .map(|i| (std::f32::consts::TAU * freq * i as f32 / cfg.sample_rate as f32).sin() * 0.5)
            .collect();
        let frame = a.process(&samples);

        // Find the band containing 1 kHz and make sure it is loud (≥ −20 dB)
        // while the lowest and highest bands stay below −40 dB.
        let max_db = frame
            .bands_db
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max_db > -20.0,
            "expected peak around 1 kHz, got {max_db} dB"
        );
        assert!(frame.bands_db[0] < -40.0);
        assert!(frame.bands_db[cfg.band_count - 1] < -40.0);
    }

    #[test]
    #[should_panic(expected = "wrong-sized window")]
    fn analyzer_panics_on_wrong_sized_input() {
        let cfg = AnalyzerConfig {
            fft_size: 256,
            ..AnalyzerConfig::default()
        };
        let mut a = Analyzer::new(cfg);
        a.process(&[0.0_f32; 128]);
    }
}
