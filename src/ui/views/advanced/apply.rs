use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::services::system_audio::restart_pipewire_user_stack;

use super::super::super::app_kit_edges::dialogs;
use super::super::super::i18n::i18n;
#[cfg(test)]
use super::UserTweaks;
use super::{TuningRevision, TuningSelection, refresh_banner};

// ── Apply / Reset ────────────────────────────────────────────────────

/// Outcome of the apply worker, distinguishing a config-write failure
/// from a service-restart failure so the UI can show the right message.
enum ApplyStatus {
    Ok,
    WriteFailed(String),
    RestartFailed(String),
}

struct ApplyOutcome {
    persisted: Option<TuningRevision>,
    preview_restored: Option<bool>,
    status: ApplyStatus,
}

pub(super) fn apply_clicked(
    button: &gtk::Button,
    selection: &Rc<TuningSelection>,
    banner: &adw::Banner,
) {
    if selection.preview_busy.get() || selection.busy.replace(true) {
        return;
    }
    let expected = selection.persisted.borrow().clone();
    let tweaks = selection.borrow().clone();
    let applied = tweaks.clone();
    let preview = selection.preview.borrow_mut().take();
    let had_preview = preview.is_some();
    if had_preview {
        *selection.preview_feedback.borrow_mut() =
            Some(i18n("Restoring the previous audio buffer…"));
    }
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
            if let Some(mut preview) = preview
                && let Err(error) = preview.stop()
            {
                return ApplyOutcome {
                    persisted: None,
                    preview_restored: Some(false),
                    status: ApplyStatus::RestartFailed(error.to_string()),
                };
            }
            let _guard = match crate::config::storage::SettingsLock::acquire() {
                Ok(guard) => guard,
                Err(error) => {
                    return ApplyOutcome {
                        persisted: None,
                        preview_restored: had_preview.then_some(true),
                        status: ApplyStatus::WriteFailed(error.to_string()),
                    };
                }
            };
            let persisted = match tweaks.apply_checked(&expected) {
                Ok(persisted) => persisted,
                Err(failure) => {
                    return ApplyOutcome {
                        persisted: failure.persisted,
                        preview_restored: had_preview.then_some(true),
                        status: ApplyStatus::WriteFailed(failure.error.to_string()),
                    };
                }
            };
            let status = match restart_pipewire_user_stack() {
                Ok(()) => ApplyStatus::Ok,
                Err(error) => ApplyStatus::RestartFailed(error.to_string()),
            };
            ApplyOutcome {
                persisted: Some(persisted),
                preview_restored: had_preview.then_some(true),
                status,
            }
        })
        .await
        .unwrap_or_else(|_| ApplyOutcome {
            persisted: None,
            preview_restored: had_preview.then_some(false),
            status: ApplyStatus::RestartFailed("worker thread panicked".into()),
        });

        selection.busy.set(false);
        if let Some(restored) = outcome.preview_restored {
            *selection.preview_feedback.borrow_mut() = Some(if restored {
                i18n("Preview finished.")
            } else {
                i18n(
                    "The audio preview could not be started or restored. Check the audio connection and try again.",
                )
            });
        }
        if let Some(persisted) = outcome.persisted {
            *selection.persisted.borrow_mut() = persisted;
            selection.restart_pending.set(true);
        }
        if matches!(outcome.status, ApplyStatus::Ok) {
            *selection.applied.borrow_mut() = applied;
            selection.restart_pending.set(false);
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

        match outcome.status {
            ApplyStatus::Ok => {}
            ApplyStatus::WriteFailed(message) => {
                dialogs::error_dialog(
                    &i18n("Failed to write configuration"),
                    &message,
                    &i18n("OK"),
                )
                .present(Some(&button));
            }
            ApplyStatus::RestartFailed(message) => {
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
