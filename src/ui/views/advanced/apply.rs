use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::services::system_audio::restart_pipewire_user_stack;

use super::super::super::app_kit_edges::dialogs;
use super::super::super::i18n::i18n;
use super::{TuningSelection, UserTweaks, refresh_banner};

// ── Apply / Reset ────────────────────────────────────────────────────

/// Outcome of the apply worker, distinguishing a config-write failure
/// from a service-restart failure so the UI can show the right message.
enum ApplyOutcome {
    Ok,
    WriteFailed(String),
    RestartFailed(String),
}

pub(super) fn apply_clicked(
    button: &gtk::Button,
    selection: &Rc<TuningSelection>,
    banner: &adw::Banner,
) {
    if selection.busy.replace(true) {
        return;
    }
    let expected = selection.applied.borrow().clone();
    let tweaks = selection.borrow().clone();
    let applied = tweaks.clone();
    let preview = selection.preview.borrow_mut().take();
    let old_label = button.label();
    if let Some(content) = selection.content.upgrade() {
        content.set_sensitive(false);
    }

    button.set_sensitive(false);
    button.set_label(&i18n("Restarting audio…"));

    let button_weak = button.downgrade();
    let banner_weak = banner.downgrade();
    let selection = Rc::clone(selection);
    glib::spawn_future_local(async move {
        // Write the drop-ins and restart the stack on the same worker so
        // the load-bearing order (fsync the config, *then* bounce the
        // daemons that re-read it) is preserved off the main loop —
        // offloading the write separately could race it past the restart.
        let outcome = gio::spawn_blocking(move || {
            if let Some(mut preview) = preview && let Err(error) = preview.stop() {
                return ApplyOutcome::RestartFailed(error.to_string());
            }
            let guard = crate::config::storage::SettingsLock::acquire();
            let _guard = match guard {
                Ok(guard) => guard,
                Err(error) => return ApplyOutcome::WriteFailed(error.to_string()),
            };
            if UserTweaks::load_from_disk() != expected {
                return ApplyOutcome::WriteFailed("Audio settings changed in another application. Reopen the tuning page before applying.".to_owned());
            }
            if let Err(e) = tweaks.apply() {
                return ApplyOutcome::WriteFailed(e.to_string());
            }
            match restart_pipewire_user_stack() {
                Ok(()) => ApplyOutcome::Ok,
                Err(e) => ApplyOutcome::RestartFailed(e.to_string()),
            }
        })
        .await
        .unwrap_or_else(|_| ApplyOutcome::RestartFailed("worker thread panicked".to_owned()));

        selection.busy.set(false);
        if matches!(outcome, ApplyOutcome::Ok) {
            *selection.applied.borrow_mut() = applied;
        }
        if let Some(content) = selection.content.upgrade() {
            content.set_sensitive(true);
        }
        let Some(button) = button_weak.upgrade() else {
            return;
        };
        button.set_sensitive(true);
        button.set_label(old_label.as_deref().unwrap_or(""));

        if let Some(banner) = banner_weak.upgrade() {
            refresh_banner(&banner, &selection);
        }

        match outcome {
            ApplyOutcome::Ok => {}
            ApplyOutcome::WriteFailed(message) => {
                dialogs::error_dialog(
                    &i18n("Failed to write configuration"),
                    &message,
                    &i18n("OK"),
                )
                .present(Some(&button));
            }
            ApplyOutcome::RestartFailed(message) => {
                dialogs::error_dialog(&i18n("Audio service restart failed"), &message, &i18n("OK"))
                    .present(Some(&button));
            }
        }
    });
}

#[cfg(test)]
pub(in crate::ui) fn trigger_apply_button_contract(button: &gtk::Button, banner: &adw::Banner) {
    let selection = Rc::new(TuningSelection::new(UserTweaks::default()));
    apply_clicked(button, &selection, banner);
}

pub(in crate::ui) fn reset_audio_settings_dialog() -> adw::AlertDialog {
    dialogs::confirm_dialog(
        &i18n("Restore default audio settings?"),
        &i18n(
            "All your custom audio settings will be removed and the \
             audio service will restart with the defaults.",
        ),
        &i18n("Cancel"),
        "reset",
        &i18n("Restore"),
        true,
    )
}
