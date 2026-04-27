//! Warning dialog for a stale `~/.local/share/wireplumber/scripts/biglinux/`
//! override.
//!
//! WirePlumber's base-dirs lookup picks the user-local copy first, so
//! anything in that path silently shadows the packaged AEC routing
//! script. After a package update fixes the routing, the user keeps
//! seeing the old behaviour until the override is removed — and the
//! cause is invisible from the symptoms.
//!
//! On every app activation we check for the override and, if present,
//! show this `AdwAlertDialog`. The user can:
//!
//! * **Remove override** — delete the file and prompt them to restart
//!   WirePlumber (we don't restart automatically because doing so kills
//!   active calls).
//! * **Keep it** — leave the override in place. Picking this combined
//!   with the "don't warn me again" checkbox stores
//!   [`UiConfig::dismiss_wp_override_warning`] so the dialog stays
//!   silent on subsequent launches.
//!
//! The dialog is purely informational and never blocks audio toggles.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::diagnostics::user_local_wp_script_override;
use crate::ui::i18n::i18n;
use crate::ui::state::AppState;

const RESPONSE_REMOVE: &str = "remove";
const RESPONSE_KEEP: &str = "keep";

/// Show the warning when a stale override exists and the user has not
/// previously asked to silence it. No-op otherwise so app activation
/// stays cheap.
pub fn maybe_show(parent: &impl IsA<gtk::Widget>, state: Rc<AppState>) {
    if state.settings().ui.dismiss_wp_override_warning {
        return;
    }
    let Some(override_path) = user_local_wp_script_override() else {
        return;
    };
    show(parent, state, override_path);
}

fn show(parent: &impl IsA<gtk::Widget>, state: Rc<AppState>, override_path: PathBuf) {
    let dialog = adw::AlertDialog::builder()
        .heading(i18n("Configuração do WirePlumber sobreposta"))
        .body(format_body(&override_path))
        .body_use_markup(true)
        .default_response(RESPONSE_REMOVE)
        .close_response(RESPONSE_KEEP)
        .build();

    dialog.add_response(RESPONSE_KEEP, &i18n("Manter"));
    dialog.add_response(RESPONSE_REMOVE, &i18n("Remover sobreposição"));
    dialog.set_response_appearance(RESPONSE_REMOVE, adw::ResponseAppearance::Destructive);

    let dismiss_check = gtk::CheckButton::builder()
        .label(i18n("Não avisar de novo"))
        .margin_top(8)
        .build();
    dialog.set_extra_child(Some(&dismiss_check));

    let state_for_response = Rc::clone(&state);
    let path_for_response = override_path.clone();
    let dismiss_for_response = dismiss_check.clone();
    dialog.connect_response(None, move |_, response| {
        let dismiss = dismiss_for_response.is_active();
        handle_response(
            response,
            &path_for_response,
            dismiss,
            Rc::clone(&state_for_response),
        );
    });

    dialog.present(Some(parent));
}

fn handle_response(response: &str, path: &Path, dismiss: bool, state: Rc<AppState>) {
    match response {
        RESPONSE_REMOVE => {
            match std::fs::remove_file(path) {
                Ok(()) => {
                    log::info!("ui: removed stale wireplumber override at {}", path.display());
                    notify_restart_needed();
                }
                Err(e) => {
                    log::warn!(
                        "ui: failed to remove wireplumber override at {}: {e}",
                        path.display()
                    );
                }
            }
            // The override is gone — no point setting the dismiss flag,
            // there is nothing left to warn about. Honour the checkbox
            // anyway so a recurrence (e.g. a sync tool restoring the
            // file) does not re-prompt against the user's wishes.
            if dismiss {
                persist_dismiss(&state);
            }
        }
        RESPONSE_KEEP => {
            if dismiss {
                persist_dismiss(&state);
            }
        }
        _ => {}
    }
}

fn persist_dismiss(state: &Rc<AppState>) {
    state.mutate(|s| s.ui.dismiss_wp_override_warning = true);
}

fn notify_restart_needed() {
    glib::MainContext::default().spawn_local(async {
        log::info!(
            "ui: stale wireplumber override removed — restart wireplumber for the fix to load"
        );
    });
}

fn format_body(path: &Path) -> String {
    let translated = i18n(
        "O arquivo <tt>{path}</tt> está mascarando a versão do pacote do script \
         de roteamento do AEC. Atualizações do pacote não terão efeito enquanto \
         essa cópia local existir.\n\n\
         Recomendado: remover a sobreposição e reiniciar o WirePlumber.",
    );
    translated.replace("{path}", &glib::markup_escape_text(&path.to_string_lossy()))
}
