//! Host-embed facade: mount the microphone window inside a FOREIGN GTK
//! application (the BigLinux multicall host) without owning the GTK app.
//!
//! Mirrors `MicrophoneApplication::on_activate` / `on_shutdown`, with the
//! teardown tied to the returned guard instead of application shutdown.
//! i18n binds the gettext domain without `setlocale`/`textdomain` (both
//! process-global and owned by the host).

use std::rc::Rc;

use adw::prelude::*;

use crate::config::AppSettings;
use crate::pipeline;
use crate::services::audio_monitor::{AudioMonitor, MonitorConfig};

use super::state::AppState;
use super::widgets::wp_override_warning;
use super::{i18n, window};

/// Owner handle for an embedded microphone window: flushes pending setting
/// writes and stops the PipeWire audio monitor when dropped (the host drops
/// it when the window closes).
pub struct MicrophoneWindowGuard {
    state: Rc<AppState>,
    monitor: Option<Rc<AudioMonitor>>,
}

impl Drop for MicrophoneWindowGuard {
    fn drop(&mut self) {
        self.state.flush();
        if let Some(monitor) = self.monitor.take() {
            if let Ok(owned) = Rc::try_unwrap(monitor) {
                drop(owned);
            }
        }
    }
}

/// Build (without presenting) the microphone window attached to `app`.
///
/// Performs the same first-activation work as the standalone app: legacy
/// config scrub, filter-chain self-heal, audio-monitor start.
#[must_use]
pub fn build_embedded_window(app: &adw::Application) -> (gtk::Window, MicrophoneWindowGuard) {
    i18n::init_gettext_embedded();
    pipeline::purge_legacy_files();
    let state = AppState::new(AppSettings::load());
    if let Err(e) = pipeline::apply(&state.settings()) {
        log::warn!("embed: self-heal apply failed: {e}");
    }
    let monitor = Rc::new(AudioMonitor::start(MonitorConfig::default()));
    let window = window::build(app, Rc::clone(&state), Rc::clone(&monitor));
    wp_override_warning::maybe_show(&window, Rc::clone(&state));
    (
        window.upcast(),
        MicrophoneWindowGuard {
            state,
            monitor: Some(monitor),
        },
    )
}
