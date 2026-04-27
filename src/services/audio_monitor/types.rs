//! Plain-data types surfaced by the spectrum analyser.

/// Default number of frequency buckets emitted per frame. 30 matches
/// the legacy Python widget so the same frequency labels (63 Hz,
/// 180 Hz, 500 Hz, 1.5 kHz, 4 kHz, 9.5 kHz) land on their expected
/// column positions.
pub const DEFAULT_BAND_COUNT: usize = 30;

/// Default FFT window size (samples). 2048 at 48 kHz ≈ 42 ms, a decent
/// balance between frequency resolution (~23 Hz/bin) and latency.
pub const DEFAULT_FFT_SIZE: usize = 2048;

/// Default hop size (samples) between successive FFT frames. At 48 kHz,
/// 512 samples ≈ 10.6 ms → frames emitted at ~94 Hz; downstream sinks
/// (UI) can pace themselves with `frame_skip`.
pub const DEFAULT_HOP_SIZE: usize = 512;

/// Default sample rate captured from pw-cat.
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

/// A single spectrum snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumFrame {
    /// Log-spaced band magnitudes in dBFS, typically in `[-80.0, 0.0]`.
    /// Order is low-frequency → high-frequency.
    pub bands_db: Vec<f32>,
    /// Full-frame RMS level in dBFS.
    pub rms_db: f32,
    /// Time-domain peak level in dBFS — the loudest single sample in
    /// the window. Used by the UI meter to show instantaneous peaks
    /// that the RMS averages away.
    pub peak_db: f32,
    /// Monotonic frame counter since the capture started.
    pub seq: u64,
}

/// Events emitted by [`crate::services::audio_monitor::AudioMonitor`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// New spectrum frame computed from the capture stream.
    Frame(SpectrumFrame),
    /// The underlying `pw-cat` process or reader thread died. The service
    /// is inoperative; the consumer should surface the error and stop
    /// visualising.
    Fatal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_internally_consistent() {
        const { assert!(DEFAULT_HOP_SIZE < DEFAULT_FFT_SIZE) };
        assert!(DEFAULT_FFT_SIZE.is_power_of_two());
        assert!(DEFAULT_HOP_SIZE.is_power_of_two());
        const { assert!(DEFAULT_BAND_COUNT <= DEFAULT_FFT_SIZE / 2) };
    }
}
