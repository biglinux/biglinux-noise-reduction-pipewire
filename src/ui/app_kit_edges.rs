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
        label_for_screen_readers(&dialog, heading);
        dialog.add_response("cancel", cancel_label);
        dialog.add_response(confirm_response, confirm_label);
        // Always a tone, never bare: a confirm button that renders flat reads
        // as one more secondary choice next to Cancel.
        dialog.set_response_appearance(
            confirm_response,
            if is_destructive {
                adw::ResponseAppearance::Destructive
            } else {
                adw::ResponseAppearance::Suggested
            },
        );
        dialog
    }

    pub fn error_dialog(heading: &str, body: &str, close_label: &str) -> adw::AlertDialog {
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .body(body)
            .close_response("close")
            .default_response("close")
            .build();
        label_for_screen_readers(&dialog, heading);
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
        // `default_response` stays on cancel rather than on the action: this
        // dialog is what the destructive flows use, and Enter must not be the
        // fast path to one.
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .close_response("cancel")
            .default_response("cancel")
            .build();
        label_for_screen_readers(&dialog, heading);
        dialog.add_response("cancel", cancel_label);
        dialog.add_response(action_response, action_label);
        dialog.set_response_appearance(
            action_response,
            if is_destructive {
                adw::ResponseAppearance::Destructive
            } else {
                adw::ResponseAppearance::Suggested
            },
        );
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_start(16)
            .margin_end(16)
            .build();
        dialog.set_extra_child(Some(&content));
        (dialog, content)
    }

    /// The heading is the dialog's accessible name. `adw::AlertDialog` does
    /// not derive one, so without this a screen reader announces an unnamed
    /// dialog and the user hears only the button labels.
    fn label_for_screen_readers(dialog: &adw::AlertDialog, heading: &str) {
        dialog.update_property(&[gtk::accessible::Property::Label(heading)]);
    }
}
