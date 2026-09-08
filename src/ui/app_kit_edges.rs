//! Native window actions and dialogs.

use adw::prelude::*;
use gtk::gio;

pub mod desktop {
    use super::*;

    pub fn install_action<W, F>(owner: &W, name: &str, callback: F)
    where
        W: IsA<gio::ActionMap>,
        F: Fn() + 'static,
    {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(move |_, _| callback());
        owner.add_action(&action);
    }
}

pub mod dialogs {
    use super::*;

    pub fn confirm_dialog(
        heading: &str,
        body: &str,
        cancel_label: &str,
        confirm_response: &str,
        confirm_label: &str,
        is_destructive: bool,
    ) -> adw::AlertDialog {
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .body(body)
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_response("cancel", cancel_label);
        dialog.add_response(confirm_response, confirm_label);
        if is_destructive {
            dialog.set_response_appearance(confirm_response, adw::ResponseAppearance::Destructive);
        }
        dialog
    }

    pub fn error_dialog(heading: &str, body: &str, close_label: &str) -> adw::AlertDialog {
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .body(body)
            .close_response("close")
            .default_response("close")
            .build();
        dialog.add_response("close", close_label);
        dialog
    }

    pub fn content_action_dialog(
        heading: &str,
        cancel_label: &str,
        action_response: &str,
        action_label: &str,
        is_destructive: bool,
    ) -> (adw::AlertDialog, gtk::Box) {
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_response("cancel", cancel_label);
        dialog.add_response(action_response, action_label);
        if is_destructive {
            dialog.set_response_appearance(action_response, adw::ResponseAppearance::Destructive);
        }
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        dialog.set_extra_child(Some(&content));
        (dialog, content)
    }
}
