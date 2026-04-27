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
use std::time::Duration;

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
    active: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// How long the worker sleeps between active checks while paused. Long
/// enough that a paused monitor barely registers on top, short enough
/// that resuming feels instant on the spectrum widget.
const PAUSED_POLL: Duration = Duration::from_millis(100);

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
        let active = Arc::new(AtomicBool::new(true));
        let worker_stop = Arc::clone(&stop);
        let worker_active = Arc::clone(&active);

        let worker = thread::Builder::new()
            .name("biglinux-microphone/audio-monitor".into())
            .spawn(move || run_worker(cfg, events_tx, worker_stop, worker_active))
            .expect("OS refused to spawn audio monitor thread");

        Self {
            events_rx,
            stop,
            active,
            worker: Some(worker),
        }
    }

    #[must_use]
    pub fn events(&self) -> async_channel::Receiver<Event> {
        self.events_rx.clone()
    }

    /// Pause / resume the FFT loop. When paused the worker stops pumping
    /// pw-cat and emitting frames so the pipe back-pressures pw-cat into
    /// blocking on its write — both processes drop to ~0 CPU until the
    /// next [`set_active(true)`] call. Used to silence the monitor while
    /// the spectrum widget is hidden.
    pub fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Release);
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

fn run_worker(
    cfg: MonitorConfig,
    tx: AsyncSender<Event>,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
) {
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
        if !active.load(Ordering::Acquire) {
            // Spectrum widget is hidden: skip pump+FFT+send so pw-cat
            // back-pressures itself into idle. Cheap atomic poll.
            thread::sleep(PAUSED_POLL);
            continue;
        }
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
    fn paused_poll_is_short_enough_for_resume_to_feel_instant() {
        // The spectrum's perceived latency on resume comes from how long
        // the worker sleeps while paused before re-checking the active
        // flag. Anything above ~150 ms is visible as a stutter when the
        // user flips between views.
        assert!(PAUSED_POLL <= Duration::from_millis(150));
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
