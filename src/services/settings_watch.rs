//! Event-driven settings watcher for the login service.
//!
//! The standalone GUI may close while the audio backend is unavailable.
//! Saving the requested settings is independent of the graph; this watcher
//! applies them through the same serialized reconciler when they change.

use gio::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct SettingsWatch {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SettingsWatch {
    pub fn start() -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("settings-watch".into())
            .spawn(move || {
                let context = glib::MainContext::new();
                let _ = context.with_thread_default(|| {
                    let path = crate::config::settings_file();
                    let Some(parent) = path.parent() else {
                        return;
                    };
                    if let Err(error) = std::fs::create_dir_all(parent) {
                    log::error!("settings watcher directory unavailable: {error}");
                    return;
                }
                let monitor = match gio::File::for_path(parent).monitor_directory(
                        gio::FileMonitorFlags::WATCH_MOVES,
                        gio::Cancellable::NONE,
                    ) {
                        Ok(monitor) => monitor,
                        Err(error) => {
                            log::error!("settings watcher unavailable: {error}");
                            return;
                        }
                    };
                    let pending = std::rc::Rc::new(std::cell::Cell::new(false));
                    let notification = std::rc::Rc::clone(&pending);
                    monitor.connect_changed(move |_, file, other, _| {
                        if file.path().as_ref() == Some(&path)
                            || other.and_then(gio::File::path).as_ref() == Some(&path)
                        {
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
                                if let Err(error) = result {
                                    log::warn!("saved audio settings are not applied: {error}");
                                }
                                last = std::fs::read(crate::config::settings_file()).ok();
                            }
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    monitor.cancel();
                });
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
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
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
