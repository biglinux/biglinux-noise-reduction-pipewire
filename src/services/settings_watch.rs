//! Event-driven settings watcher for the login service.
//!
//! The standalone GUI may close while the audio backend is unavailable.
//! Saving the requested settings is independent of the graph; this watcher
//! applies them through the same serialized reconciler when they change.

use gio::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
                    let pending = std::rc::Rc::new(std::cell::Cell::new(true));
                    let notification = std::rc::Rc::clone(&pending);
                    monitor.connect_changed(move |_, file, other, _| {
                        if file.path().as_ref() == Some(&path)
                            || other.and_then(gio::File::path).as_ref() == Some(&path)
                        {
                            notification.set(true);
                        }
                    });
                    // Events wake the normal path; failed revisions also get
                    // bounded retries without requiring another file edit.
                    let mut cursor = ApplyCursor::default();
                    while !worker_stop.load(Ordering::Acquire) {
                        while context.iteration(false) {}
                        if pending.replace(false) || cursor.retry_due(Instant::now()) {
                            let current = std::fs::read(crate::config::settings_file()).ok();
                            if cursor.needs_apply(current.as_deref()) {
                                match apply_saved() {
                                    Ok(written) => {
                                        cursor.succeeded(written);
                                        // A concurrent non-participating writer can have
                                        // changed the file since dispatch. Compare again;
                                        // never acknowledge that edit as our own.
                                        pending.set(true);
                                    }
                                    Err(error) => {
                                        log::warn!("saved audio settings are pending: {error}");
                                        cursor.failed(Instant::now());
                                    }
                                }
                            } else {
                                cursor.retry_at = None;
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

fn apply_saved() -> std::io::Result<Vec<u8>> {
    let _guard = crate::config::storage::SettingsLock::acquire()?;
    let mut settings = crate::config::AppSettings::load_strict()?;
    settings.settle_quality();
    super::echo::settle(&mut settings.echo_cancel);
    let written = settings.save_with_revision()?;
    super::reconcile::apply(&settings, false)?;
    Ok(written)
}

impl Drop for SettingsWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Longest wait between retries of a revision that failed to apply.
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);

/// Revision acknowledgement is independent of event delivery and retry timing.
#[derive(Default)]
struct ApplyCursor {
    applied: Option<Vec<u8>>,
    retry_at: Option<Instant>,
    failures: u32,
}

impl ApplyCursor {
    fn needs_apply(&self, bytes: Option<&[u8]>) -> bool {
        self.applied.as_deref() != bytes
    }

    fn retry_due(&self, now: Instant) -> bool {
        self.retry_at.is_some_and(|deadline| now >= deadline)
    }

    fn succeeded(&mut self, written: Vec<u8>) {
        self.applied = Some(written);
        self.retry_at = None;
        self.failures = 0;
    }

    fn failed(&mut self, now: Instant) {
        let delay =
            Duration::from_millis(500 * (1_u64 << self.failures.min(6))).min(MAX_RETRY_BACKOFF);
        self.failures = self.failures.saturating_add(1);
        self.retry_at = Some(now + delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_keeps_revision_pending_without_another_event() {
        let mut cursor = ApplyCursor::default();
        cursor.succeeded(b"old".to_vec());
        let now = Instant::now();
        cursor.failed(now);
        assert!(cursor.needs_apply(Some(b"new")));
        assert!(!cursor.retry_due(now));
        assert!(cursor.retry_due(now + Duration::from_millis(500)));
        cursor.succeeded(b"new".to_vec());
        assert!(!cursor.needs_apply(Some(b"new")));
        assert!(!cursor.retry_due(now + MAX_RETRY_BACKOFF * 2));
    }

    #[test]
    fn acknowledging_our_write_does_not_consume_a_newer_external_revision() {
        let mut cursor = ApplyCursor::default();
        cursor.succeeded(b"submitted revision".to_vec());
        assert!(cursor.needs_apply(Some(b"external revision")));
        assert!(!cursor.needs_apply(Some(b"submitted revision")));
    }

    #[test]
    fn retry_backoff_is_bounded_and_success_resets_it() {
        let now = Instant::now();
        let mut cursor = ApplyCursor::default();
        for _ in 0..100 {
            cursor.failed(now);
            assert!(cursor.retry_at.unwrap() - now <= Duration::from_secs(30));
        }
        cursor.succeeded(b"ok".to_vec());
        cursor.failed(now);
        assert_eq!(cursor.retry_at, Some(now + Duration::from_millis(500)));
    }
}
