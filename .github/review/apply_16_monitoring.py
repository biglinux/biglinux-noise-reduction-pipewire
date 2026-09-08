from pathlib import Path
from _common import done, replace, write, commit

TITLE = 'fix(monitor): release hidden capture and recover from PipeWire restarts'
if not done(TITLE):
    path = 'src/services/audio_monitor.rs'
    text = Path(path).read_text().replace('async_channel::bounded::<Event>(64)', 'async_channel::bounded::<Event>(1)')
    text = text.replace('let active = Arc::new(AtomicBool::new(true));', 'let active = Arc::new(AtomicBool::new(false));', 1)
    text = text.replace('        self.active.store(on, Ordering::Release);', '''        self.active.store(on, Ordering::Release);
        if !on {
            let child = self.capture_child.lock().ok().and_then(|slot| slot.clone());
            stop_capture_child(child);
        }''')
    start = text.index('    let mut capture = match Capture::spawn(', text.index('fn run_worker('))
    end = text.index('\n#[cfg(test)]', start)
    text = text[:start] + r'''    let mut capture: Option<Capture> = None;
    let mut analyzer = Analyzer::new(monitor_config.analyzer.clone());
    let mut window = Vec::with_capacity(monitor_config.analyzer.fft_size);
    let mut retry_at = std::time::Instant::now();
    while !stop.load(Ordering::Acquire) {
        if !active.load(Ordering::Acquire) {
            drop(capture.take());
            if let Ok(mut slot) = capture_child.lock() { slot.take(); }
            thread::sleep(PAUSED_POLL);
            continue;
        }
        if capture.is_none() {
            if std::time::Instant::now() < retry_at {
                thread::sleep(PAUSED_POLL);
                continue;
            }
            match Capture::spawn(monitor_config.analyzer.sample_rate, monitor_config.analyzer.fft_size, monitor_config.hop_size) {
                Ok(next) => {
                    if let Ok(mut slot) = capture_child.lock() { *slot = Some(next.cancellation_handle()); }
                    capture = Some(next);
                    if stop.load(Ordering::Acquire) || !active.load(Ordering::Acquire) { continue; }
                }
                Err(error) => {
                    if tx.force_send(Event::Fatal(format!("Microphone monitoring unavailable: {error}"))).is_err() { break; }
                    retry_at = std::time::Instant::now() + Duration::from_secs(2);
                    continue;
                }
            }
        }
        let Some(current) = capture.as_mut() else { continue; };
        if let Err(error) = current.pump() {
            drop(capture.take());
            if let Ok(mut slot) = capture_child.lock() { slot.take(); }
            if active.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) {
                if tx.force_send(Event::Fatal(format!("Microphone monitoring interrupted: {error}"))).is_err() { break; }
                retry_at = std::time::Instant::now() + Duration::from_secs(2);
            }
            continue;
        }
        if !current.ready() { continue; }
        current.copy_window_into(&mut window);
        let frame = analyzer.analyze_samples(&window);
        // Latest-value delivery: after a slow UI frame, show the present,
        // not a queue of obsolete microphone levels.
        if tx.force_send(Event::Frame(frame)).is_err() { break; }
    }
    drop(capture);
    if let Ok(mut slot) = capture_child.lock() { slot.take(); }
}
''' + text[end:]
    text = text.replace('use log::{debug, warn};', 'use log::warn;')
    text = text.replace('Pause / resume the FFT loop. When paused the worker stops pumping', 'Pause / resume capture and FFT. Pausing terminates the owned')
    text = text.replace('pw-cat and emitting frames so the pipe back-pressures pw-cat into\n    /// blocking on its write — both processes drop to ~0 CPU until the\n    /// next `set_active(true)` call.', 'pw-cat child instead of blocking its real-time output pipe. Capture\n    /// is recreated when the meter becomes visible again.')
    Path(path).write_text(text)
    commit(TITLE, [path])

TITLE = 'fix(daemon): reconcile saved edits even after the settings window closes'
if not done(TITLE):
    # Use an event-driven GIO file monitor on its own context. Only stamps
    # changed by an atomic settings write cause the shared reconciler to run.
    write('src/services/settings_watch.rs', r'''//! Event-driven settings watcher for the login service.
//!
//! The standalone GUI may close while the audio backend is unavailable.
//! Saving the requested settings is independent of the graph; this watcher
//! applies them through the same serialized reconciler when they change.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use gio::prelude::*;

pub struct SettingsWatch {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SettingsWatch {
    pub fn start() -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new().name("settings-watch".into()).spawn(move || {
            let context = glib::MainContext::new();
            let _ = context.with_thread_default(|| {
                let path = crate::config::settings_file();
                let Some(parent) = path.parent() else { return; };
                let monitor = match gio::File::for_path(parent).monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
                    Ok(monitor) => monitor,
                    Err(error) => { log::error!("settings watcher unavailable: {error}"); return; }
                };
                let pending = std::rc::Rc::new(std::cell::Cell::new(false));
                let notification = std::rc::Rc::clone(&pending);
                monitor.connect_changed(move |_, file, other, _| {
                    if file.path().as_ref() == Some(&path) || other.and_then(gio::File::path).as_ref() == Some(&path) {
                        notification.set(true);
                    }
                });
                // One timestamp read per coalesced event, never a new
                // subprocess on an unrelated PipeWire monitor notification.
                let mut last = std::fs::read(crate::config::settings_file()).ok();
                while !worker_stop.load(Ordering::Acquire) {
                    while context.iteration(false) {}
                    if pending.replace(false) {
                        let now = std::fs::read(crate::config::settings_file()).ok();
                        if now != last {
                            let result = apply_saved();
                            if let Err(error) = result { log::warn!("saved audio settings are not applied: {error}"); }
                            last = std::fs::read(crate::config::settings_file()).ok();
                        }
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                monitor.cancel();
            });
        })?;
        Ok(Self { stop, thread: Some(thread) })
    }
}

fn apply_saved() -> std::io::Result<()> {
    let _guard = crate::config::storage::SettingsLock::acquire()?;
    let mut settings = crate::config::AppSettings::load_strict()?;
    settings.settle_quality();
    super::echo::settle(&mut settings.echo_cancel);
    settings.save()?;
    super::reconcile::apply(&settings, false)
}

impl Drop for SettingsWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}
''')
    file = Path('src/services.rs'); file.write_text(file.read_text() + '\npub mod settings_watch;\n')
    replace('src/bin/cli.rs', 'fn watch_echo() -> ExitCode {', '''fn watch_echo() -> ExitCode {
    let _settings_watch = match biglinux_microphone::services::settings_watch::SettingsWatch::start() {
        Ok(watch) => watch,
        Err(error) => return exit_with_error(&error.to_string()),
    };''')
    commit(TITLE, ['src/services/settings_watch.rs', 'src/services.rs', 'src/bin/cli.rs'])
