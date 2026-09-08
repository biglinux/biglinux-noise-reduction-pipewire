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
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use async_channel::Sender as AsyncSender;
use big_os_kit::subprocess::BigSubprocessChild;
use log::{debug, warn};

pub use analyzer::{Analyzer, AnalyzerConfig, amplitude_to_db, hann_window, peak_dbfs, rms_dbfs};
use capture::Capture;
pub use types::{
    DEFAULT_BAND_COUNT, DEFAULT_FFT_SIZE, DEFAULT_HOP_SIZE, DEFAULT_SAMPLE_RATE, Event,
    SpectrumFrame,
};

/// Handle over a running audio monitor. Drop or call [`Self::shutdown`]
/// to stop the worker and reap the pw-cat child.
pub struct AudioMonitor {
    events_rx: async_channel::Receiver<Event>,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    capture_child: Arc<Mutex<Option<Arc<Mutex<BigSubprocessChild>>>>>,
    worker: Option<JoinHandle<()>>,
}

/// How long the worker sleeps between active checks while paused. Long
/// enough that a paused monitor barely registers on top, short enough
/// that resuming feels instant on the spectrum widget.
const PAUSED_POLL: Duration = Duration::from_millis(100);

/// Options controlling the monitor loop.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub analyzer: AnalyzerConfig,
    pub hop_size: usize,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
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
    pub fn start(monitor_config: MonitorConfig) -> Self {
        let (events_tx, events_rx) = async_channel::bounded::<Event>(64);
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(true));
        let capture_child = Arc::new(Mutex::new(None));
        let worker_stop = Arc::clone(&stop);
        let worker_active = Arc::clone(&active);
        let worker_capture_child = Arc::clone(&capture_child);

        let worker_events_tx = events_tx.clone();
        let worker = match thread::Builder::new()
            .name("biglinux-microphone/audio-monitor".into())
            .spawn(move || {
                run_worker(
                    monitor_config,
                    worker_events_tx,
                    worker_stop,
                    worker_active,
                    worker_capture_child,
                );
            }) {
            Ok(worker) => Some(worker),
            Err(error) => {
                let _ = events_tx
                    .try_send(Event::Fatal(format!("audio monitor thread spawn: {error}")));
                None
            }
        };

        Self {
            events_rx,
            stop,
            active,
            capture_child,
            worker,
        }
    }

    #[must_use]
    pub fn events(&self) -> async_channel::Receiver<Event> {
        self.events_rx.clone()
    }

    /// Pause / resume the FFT loop. When paused the worker stops pumping
    /// pw-cat and emitting frames so the pipe back-pressures pw-cat into
    /// blocking on its write — both processes drop to ~0 CPU until the
    /// next `set_active(true)` call. Used to silence the monitor while
    /// the spectrum widget is hidden.
    pub fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn contract_handle() -> (Self, AsyncSender<Event>) {
        let (events_tx, events_rx) = async_channel::bounded::<Event>(64);
        let monitor = Self {
            events_rx,
            stop: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicBool::new(true)),
            capture_child: Arc::new(Mutex::new(None)),
            worker: None,
        };
        (monitor, events_tx)
    }

    #[cfg(test)]
    pub(crate) fn is_active_for_contract(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        stop_capture_child(self.take_capture_child());
        self.join_worker();
    }

    fn take_capture_child(&self) -> Option<Arc<Mutex<BigSubprocessChild>>> {
        self.capture_child.lock().ok()?.take()
    }

    fn join_worker(&mut self) {
        if let Some(handle) = self.worker.take()
            && handle.join().is_err()
        {
            warn!("audio monitor worker panicked");
        }
    }
}

impl Drop for AudioMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let capture_child = self.take_capture_child();
        let Some(worker) = self.worker.take() else {
            stop_capture_child(capture_child);
            return;
        };
        // Interrupt the pipe read before allocating the reaper thread. If the
        // OS refuses that thread, the worker still observes EOF, drops its
        // capture, reaps the child itself, and exits detached.
        stop_capture_child(capture_child);
        let _ = thread::Builder::new()
            .name("biglinux-microphone/audio-monitor-reaper".into())
            .spawn(move || {
                if worker.join().is_err() {
                    warn!("audio monitor worker panicked");
                }
            });
    }
}

fn stop_capture_child(capture_child: Option<Arc<Mutex<BigSubprocessChild>>>) {
    if let Some(child) = capture_child
        && let Ok(mut child) = child.lock()
    {
        let _ = child.kill();
    }
}

fn run_worker(
    monitor_config: MonitorConfig,
    tx: AsyncSender<Event>,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    capture_child: Arc<Mutex<Option<Arc<Mutex<BigSubprocessChild>>>>>,
) {
    let mut capture = match Capture::spawn(
        monitor_config.analyzer.sample_rate,
        monitor_config.analyzer.fft_size,
        monitor_config.hop_size,
    ) {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.try_send(Event::Fatal(format!("pw-cat spawn: {e}")));
            return;
        }
    };
    if let Ok(mut registered_child) = capture_child.lock() {
        *registered_child = Some(capture.cancellation_handle());
    }

    let mut analyzer = Analyzer::new(monitor_config.analyzer.clone());

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
            break;
        }
        if !capture.ready() {
            continue;
        }
        let window = capture.window_snapshot();
        let frame = analyzer.analyze_samples(&window);
        match tx.try_send(Event::Frame(frame)) {
            // UI is falling behind — drop the frame; the next tick will
            // overwrite the visualisation anyway.
            Ok(()) | Err(async_channel::TrySendError::Full(_)) => {}
            Err(async_channel::TrySendError::Closed(_)) => break,
        }
    }
    drop(capture);
    if let Ok(mut registered_child) = capture_child.lock() {
        registered_child.take();
    }
    debug!("audio monitor: loop exited");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn dropping_monitor_never_joins_a_blocked_worker_on_the_calling_thread() {
        let (events_tx, events_rx) = async_channel::bounded::<Event>(1);
        let capture_child = big_os_kit::subprocess::BigSubprocessSpec::builder()
            .program("sleep")
            .arg("30")
            .build()
            .spawn()
            .expect("spawn contract capture child");
        let capture_child = Arc::new(Mutex::new(capture_child));
        let capture_observer = capture_child.clone();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (worker_done_tx, worker_done_rx) = std::sync::mpsc::channel::<()>();
        let worker = thread::spawn(move || {
            let _ = release_rx.recv();
            let _ = worker_done_tx.send(());
        });
        let monitor = AudioMonitor {
            events_rx,
            stop: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicBool::new(true)),
            capture_child: Arc::new(Mutex::new(Some(capture_child))),
            worker: Some(worker),
        };
        let (drop_returned_tx, drop_returned_rx) = std::sync::mpsc::channel::<()>();
        let dropper = thread::spawn(move || {
            drop(monitor);
            let _ = drop_returned_tx.send(());
        });

        let returned_without_worker = drop_returned_rx
            .recv_timeout(Duration::from_millis(250))
            .is_ok();
        let child_deadline = std::time::Instant::now() + Duration::from_secs(1);
        let capture_was_killed = loop {
            if capture_observer
                .lock()
                .unwrap()
                .try_wait()
                .expect("poll contract capture child")
                .is_some()
            {
                break true;
            }
            if std::time::Instant::now() >= child_deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(5));
        };
        let _ = release_tx.send(());
        worker_done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("detached reaper must let the worker finish");
        dropper.join().expect("drop caller must not panic");
        drop(events_tx);

        assert!(returned_without_worker);
        assert!(capture_was_killed);
    }
}
