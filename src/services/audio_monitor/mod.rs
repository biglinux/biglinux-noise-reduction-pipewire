//! Real-time spectrum analyser fed by `pw-cat`.
//!
//! Architecture:
//!
//! ```text
//! pw-cat (subprocess) ─ f32 LE ─▶ Capture ring buffer
//!                                     │ hop_size samples per tick
//!                                     ▼
//!                                 Analyzer (Hann + FFT + band agg)
//!                                     │
//!                                     ▼
//!                          async_channel::Sender<Event>
//!                                     │
//!                                     ▼
//!                                  UI / CLI
//! ```
//!
//! A single worker thread owns the capture and analyser: they share no
//! state with the rest of the app and the handle ([`AudioMonitor`]) does
//! nothing more than forward events + request shutdown.

mod analyzer;
mod capture;
mod types;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use async_channel::Sender as AsyncSender;
use log::{debug, warn};

pub use analyzer::{amplitude_to_db, hann_window, peak_dbfs, rms_dbfs, Analyzer, AnalyzerConfig};
pub use capture::{Capture, CaptureTarget};
pub use types::{
    Event, SpectrumFrame, DEFAULT_BAND_COUNT, DEFAULT_FFT_SIZE, DEFAULT_HOP_SIZE,
    DEFAULT_SAMPLE_RATE,
};

/// Handle over a running audio monitor. Drop or call [`Self::shutdown`]
/// to stop the worker and reap the pw-cat child.
pub struct AudioMonitor {
    events_rx: async_channel::Receiver<Event>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// Options controlling the monitor loop.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub target: CaptureTarget,
    pub analyzer: AnalyzerConfig,
    pub hop_size: usize,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            target: CaptureTarget::DefaultSource,
            analyzer: AnalyzerConfig::default(),
            hop_size: DEFAULT_HOP_SIZE,
        }
    }
}

impl AudioMonitor {
    /// Spawn the capture thread. Any error encountered during startup or
    /// while running is forwarded as [`Event::Fatal`] on the events
    /// channel; the worker then exits.
    #[must_use]
    pub fn start(cfg: MonitorConfig) -> Self {
        let (events_tx, events_rx) = async_channel::bounded::<Event>(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);

        let worker = thread::Builder::new()
            .name("biglinux-microphone/audio-monitor".into())
            .spawn(move || run_worker(cfg, events_tx, worker_stop))
            .expect("OS refused to spawn audio monitor thread");

        Self {
            events_rx,
            stop,
            worker: Some(worker),
        }
    }

    #[must_use]
    pub fn events(&self) -> async_channel::Receiver<Event> {
        self.events_rx.clone()
    }

    pub fn shutdown(mut self) {
        self.shutdown_internal();
    }

    fn shutdown_internal(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.worker.take() {
            if handle.join().is_err() {
                warn!("audio monitor worker panicked");
            }
        }
    }
}

impl Drop for AudioMonitor {
    fn drop(&mut self) {
        self.shutdown_internal();
    }
}

fn run_worker(cfg: MonitorConfig, tx: AsyncSender<Event>, stop: Arc<AtomicBool>) {
    let mut capture = match Capture::spawn(
        &cfg.target,
        cfg.analyzer.sample_rate,
        cfg.analyzer.fft_size,
        cfg.hop_size,
    ) {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.try_send(Event::Fatal(format!("pw-cat spawn: {e}")));
            return;
        }
    };

    let mut analyzer = Analyzer::new(cfg.analyzer.clone());

    debug!("audio monitor: loop start");
    while !stop.load(Ordering::Acquire) {
        if let Err(e) = capture.pump() {
            let _ = tx.try_send(Event::Fatal(format!("pw-cat read: {e}")));
            return;
        }
        if !capture.ready() {
            continue;
        }
        let window = capture.window_snapshot();
        let frame = analyzer.process(&window);
        match tx.try_send(Event::Frame(frame)) {
            // UI is falling behind — drop the frame; the next tick will
            // overwrite the visualisation anyway.
            Ok(()) | Err(async_channel::TrySendError::Full(_)) => {}
            Err(async_channel::TrySendError::Closed(_)) => break,
        }
    }
    debug!("audio monitor: loop exited");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_config_default_uses_default_source() {
        let cfg = MonitorConfig::default();
        assert!(matches!(cfg.target, CaptureTarget::DefaultSource));
        assert_eq!(cfg.hop_size, DEFAULT_HOP_SIZE);
        assert_eq!(cfg.analyzer.sample_rate, DEFAULT_SAMPLE_RATE);
        assert_eq!(cfg.analyzer.fft_size, DEFAULT_FFT_SIZE);
    }

    #[test]
    fn monitor_config_can_target_named_node() {
        let cfg = MonitorConfig {
            target: CaptureTarget::NodeName("mic-biglinux".into()),
            ..MonitorConfig::default()
        };
        match cfg.target {
            CaptureTarget::NodeName(ref name) => assert_eq!(name, "mic-biglinux"),
            CaptureTarget::DefaultSource => panic!("expected NodeName"),
        }
    }
}
