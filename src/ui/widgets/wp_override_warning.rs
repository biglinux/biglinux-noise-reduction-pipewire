//! Warning dialog for a stale `~/.local/share/wireplumber/scripts/biglinux/`
//! override.
//!
//! WirePlumber's base-dirs lookup picks the user-local copy first, so
//! anything in that path silently shadows the packaged AEC routing
//! script. Package routing changes remain shadowed until the override
//! is removed, and the cause is invisible from the symptoms.
//!
//! On application startup we check for the override and, if present,
//! show this `AdwAlertDialog`. The user can:
//!
//! * **Remove override** — delete the file and prompt them to restart
//!   WirePlumber (we don't restart automatically because doing so kills
//!   active calls).
//! * **Keep it** — leave the override in place. Picking this combined
//!   with the "don't warn me again" checkbox stores
//!   [`UiConfig::dismiss_wp_override_warning`](crate::config::UiConfig::dismiss_wp_override_warning)
//!   so the dialog stays
//!   silent on subsequent launches.
//!
//! The dialog is purely informational and never blocks audio toggles.

use std::path::{Path, PathBuf};

use crate::pipeline::remove_file_if_exists;
use adw::prelude::*;
use gtk::glib;

use crate::diagnostics::user_local_wp_script_override;
use crate::ui::app_kit_edges::dialogs;
use crate::ui::i18n::i18n;

const RESPONSE_REMOVE: &str = "remove";

/// User decision emitted by the warning dialog to the Relm4 root.
#[derive(Debug)]
pub(in crate::ui) enum OverrideWarningDecision {
    Keep { should_dismiss: bool },
    Remove { path: PathBuf, should_dismiss: bool },
}

/// Result of removing the override on a worker thread.
#[derive(Debug)]
pub(in crate::ui) enum OverrideRemoval {
    Removed,
    AlreadyMissing,
}

/// Show the warning when a stale override exists and the user has not
/// previously asked to silence it. No-op otherwise so app activation
/// stays cheap.
pub fn maybe_show(
    parent: &impl IsA<gtk::Widget>,
    should_suppress: bool,
    emit: impl Fn(OverrideWarningDecision) + 'static,
) {
    if should_suppress {
        return;
    }
    let Some(override_path) = user_local_wp_script_override() else {
        return;
    };
    show(parent, override_path, emit);
}

fn show(
    parent: &impl IsA<gtk::Widget>,
    override_path: PathBuf,
    emit: impl Fn(OverrideWarningDecision) + 'static,
) {
    // Cataloged shared dialog (cancel = "Keep", destructive confirm = "Remove
    // override") instead of a hand-built `adw::AlertDialog`.
    let (dialog, content) = dialogs::content_action_dialog(
        &i18n("WirePlumber configuration overridden"),
        &i18n("Keep"),
        RESPONSE_REMOVE,
        &i18n("Remove override"),
        true,
    );

    let body = gtk::Label::builder()
        .label(format_body(&override_path))
        .use_markup(true)
        .wrap(true)
        .xalign(0.0)
        .build();
    content.append(&body);

    let dismiss_check = gtk::CheckButton::builder()
        .label(i18n("Don't warn again"))
        .margin_top(8)
        .build();
    content.append(&dismiss_check);

    let path_for_response = override_path.clone();
    let dismiss_for_response = dismiss_check.clone();
    dialog.connect_response(None, move |_, response| {
        let should_dismiss = dismiss_for_response.is_active();
        let decision = if response == RESPONSE_REMOVE {
            OverrideWarningDecision::Remove {
                path: path_for_response.clone(),
                should_dismiss,
            }
        } else {
            OverrideWarningDecision::Keep { should_dismiss }
        };
        emit(decision);
    });

    dialog.present(Some(parent));
}

pub(in crate::ui) fn remove_override(path: &Path) -> Result<OverrideRemoval, String> {
    let existed = path.exists();
    remove_file_if_exists(path).map_err(|error| error.to_string())?;
    if existed {
        Ok(OverrideRemoval::Removed)
    } else {
        Ok(OverrideRemoval::AlreadyMissing)
    }
}

pub(in crate::ui) fn present_removal_result(
    parent: &gtk::Widget,
    path: &Path,
    result: &Result<OverrideRemoval, String>,
) {
    match result {
        Ok(OverrideRemoval::Removed) => {
            log::info!(
                "ui: removed stale wireplumber override at {}",
                path.display()
            );
            notify_restart_needed(parent);
        }
        Ok(OverrideRemoval::AlreadyMissing) => {
            log::warn!(
                "ui: stale wireplumber override disappeared before removal at {}",
                path.display()
            );
        }
        Err(cause) => {
            log::warn!(
                "ui: failed to remove wireplumber override at {}: {cause}",
                path.display()
            );
            notify_remove_failed(parent, cause);
        }
    }
}

/// The screen contract requires the removal to *visibly* report that
/// WirePlumber continues running the loaded script until restarted — a
/// journal line is not a report. We deliberately do not restart
/// WirePlumber ourselves: that would cut any active call.
fn notify_restart_needed(parent: &gtk::Widget) {
    log::info!("ui: stale wireplumber override removed — restart wireplumber for the fix to load");
    dialogs::error_dialog(
        &i18n("Override removed"),
        &i18n(
            "The packaged audio routing will be used after WirePlumber restarts. \
             Log out and back in, or run \u{201c}systemctl --user restart wireplumber\u{201d}. \
             Audio keeps working in the meantime with the old routing.",
        ),
        &i18n("OK"),
    )
    .present(Some(parent));
}

fn notify_remove_failed(parent: &gtk::Widget, cause: &str) {
    dialogs::error_dialog(
        &i18n("Could not remove the override"),
        &format!(
            "{cause}\n\n{}",
            i18n("Remove the file manually, then restart WirePlumber.")
        ),
        &i18n("OK"),
    )
    .present(Some(parent));
}

fn format_body(path: &Path) -> String {
    let translated = i18n(
        "The file <tt>{path}</tt> is masking the packaged AEC routing script. \
         Package updates won't take effect while this local copy exists.\n\n\
         Recommended: remove the override and restart WirePlumber.",
    );
    translated.replace("{path}", &glib::markup_escape_text(&path.to_string_lossy()))
}
