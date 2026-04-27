//! `MicrophoneApplication` — the top-level `adw::Application`.
//!
//! Owns the shared [`AppState`], the [`PwService`] main-loop worker, and
//! the [`AudioMonitor`] capture. Activating the app creates (or raises)
//! the main window. Shutting down the app tears down the background
//! services in reverse order.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use crate::config::{app_id, AppSettings};
use crate::pipeline;
use crate::services::audio_monitor::{AudioMonitor, MonitorConfig};

use super::state::AppState;
use super::widgets::wp_override_warning;
use super::window;

/// Thin wrapper around `adw::Application` that keeps the domain services
/// alive for the app's lifetime.
pub struct MicrophoneApplication {
    inner: adw::Application,
    state: Rc<AppState>,
    monitor: RefCell<Option<Rc<AudioMonitor>>>,
}

impl MicrophoneApplication {
    #[must_use]
    pub fn new() -> Rc<Self> {
        let inner = adw::Application::builder()
            .application_id(app_id())
            .flags(gio::ApplicationFlags::FLAGS_NONE)
            .build();

        let state = AppState::new(AppSettings::load());
        let app = Rc::new(Self {
            inner,
            state,
            monitor: RefCell::new(None),
        });
        app.connect_signals();
        app
    }

    /// Run the application. Blocks until the last window closes.
    pub fn run(self: &Rc<Self>) -> glib::ExitCode {
        self.inner.run()
    }

    fn connect_signals(self: &Rc<Self>) {
        let me = Rc::clone(self);
        self.inner.connect_activate(move |_| me.on_activate());

        let me = Rc::clone(self);
        self.inner.connect_shutdown(move |_| me.on_shutdown());
    }

    fn on_activate(self: &Rc<Self>) {
        // If a window already exists (second activation), raise it
        // instead of spawning a new one.
        if let Some(existing) = self.inner.active_window() {
            existing.present();
            return;
        }

        // Self-heal on the first activation of the session: scrub any
        // file the Python configurator (or an older Rust revision) left
        // behind, then rewrite our own configs so the on-disk state
        // matches the current binary. The user-level systemd unit does
        // the same at login, but covering the GUI path catches users
        // who upgrade the package mid-session.
        pipeline::purge_legacy_files();
        if let Err(e) = pipeline::apply(&self.state.settings()) {
            log::warn!("app: self-heal apply failed: {e}");
        }

        let monitor = self
            .monitor
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(AudioMonitor::start(MonitorConfig::default())))
            .clone();

        let window = window::build(&self.inner, Rc::clone(&self.state), Rc::clone(&monitor));
        window.present();

        wp_override_warning::maybe_show(&window, Rc::clone(&self.state));
    }

    fn on_shutdown(&self) {
        // Flush in-flight setting changes before we tear down services.
        self.state.flush();

        if let Some(monitor) = self.monitor.borrow_mut().take() {
            drop_service(monitor);
        }
    }
}

/// Consume an `Rc<Service>` on shutdown, performing the full teardown
/// only when we own the last reference. In practice we always do by this
/// point, but guarding keeps us robust if a future feature captures the
/// service into a long-lived closure.
fn drop_service<T>(service: Rc<T>) {
    if let Ok(owned) = Rc::try_unwrap(service) {
        drop(owned);
    }
}
