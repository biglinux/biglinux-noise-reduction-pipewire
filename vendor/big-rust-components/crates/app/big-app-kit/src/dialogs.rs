// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Dialog workflow contracts.
//!
//! The display-free specs (`BigDialogSpec`, `BigEntryValidation`, …) live
//! in `big_app_kit_core::dialogs` and are re-exported below; the libadwaita
//! `AlertDialog` builders stay here where gtk-rs is available.

use adw::prelude::*;
use relm4::gtk;
use std::cell::RefCell;
use std::rc::Rc;

#[doc(inline)]
pub use big_app_kit_core::dialogs::{
    BigDialogKind, BigDialogResolved, BigDialogSpec, BigEntryValidation, BigEntryValidationTone,
    BigValidatedEntryDialogSpec, persist_dialog_size,
};

type EntryAcceptCallback = Box<dyn FnOnce(String)>;
type EntryAcceptCell = Rc<RefCell<Option<EntryAcceptCallback>>>;

/// Build a single-button `AdwAlertDialog` with an empty content box.
/// Caller fills the returned `gtk::Box` and presents the dialog.
pub fn content_dialog(heading: &str, close_label: &str) -> (adw::AlertDialog, gtk::Box) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .close_response("close")
        .build();
    dialog.update_property(&[gtk::accessible::Property::Label(heading)]);
    dialog.add_response("close", close_label);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(16);
    content.set_margin_end(16);
    dialog.set_extra_child(Some(&content));
    (dialog, content)
}

/// Build a two-button dialog (cancel + custom action) with an empty
/// content box. `destructive = true` styles the action button in red.
pub fn content_action_dialog(
    heading: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    destructive: bool,
) -> (adw::AlertDialog, gtk::Box) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .close_response("cancel")
        .default_response(confirm_id)
        .build();
    dialog.update_property(&[gtk::accessible::Property::Label(heading)]);
    dialog.add_response("cancel", cancel_label);
    dialog.add_response(confirm_id, confirm_label);
    dialog.set_response_appearance(
        confirm_id,
        if destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(16);
    content.set_margin_end(16);
    dialog.set_extra_child(Some(&content));
    (dialog, content)
}

/// Present a single-entry action dialog with live validation.
///
/// The app owns text, normalization, validation rules, and the accepted value.
/// The shared helper owns the GTK/libadwaita structure, action enablement,
/// validation message styling, and callback lifetime plumbing.
pub fn present_validated_entry_dialog<N, V, A>(
    parent: &impl IsA<gtk::Widget>,
    spec: BigValidatedEntryDialogSpec,
    normalize: N,
    validate: V,
    on_accept: A,
) where
    N: Fn(&str) -> String + 'static,
    V: Fn(&str) -> BigEntryValidation + 'static,
    A: FnOnce(String) + 'static,
{
    let (dialog, content) = content_action_dialog(
        &spec.heading,
        &spec.cancel_label,
        &spec.action_id,
        &spec.action_label,
        spec.destructive,
    );
    dialog.set_body(&spec.body);

    let entry = adw::EntryRow::builder().title(&spec.entry_title).build();
    let validation_label = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();

    let group = adw::PreferencesGroup::new();
    group.add(&entry);

    let dialog_body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    dialog_body.append(&group);
    dialog_body.append(&validation_label);
    content.append(&dialog_body);
    dialog.set_response_enabled(&spec.action_id, false);

    let normalize = Rc::new(normalize);
    let validate = Rc::new(validate);
    {
        let dialog_weak = dialog.downgrade();
        let validation_label = validation_label.clone();
        let action_id = spec.action_id.clone();
        let normalize = normalize.clone();
        let validate = validate.clone();
        entry.connect_changed(move |row| {
            let Some(dialog) = dialog_weak.upgrade() else {
                return;
            };
            let normalized = normalize(row.text().as_str());
            let validation = validate(&normalized);
            apply_entry_validation(&validation_label, &validation);
            dialog.set_response_enabled(&action_id, validation.is_valid);
        });
    }

    let on_accept: EntryAcceptCell = Rc::new(RefCell::new(Some(Box::new(on_accept))));
    let on_accept_cell = on_accept.clone();
    let action_id = spec.action_id.clone();
    dialog.connect_response(None, move |dialog, response| {
        if response == action_id {
            let normalized = normalize(entry.text().as_str());
            if let Some(callback) = on_accept_cell.borrow_mut().take() {
                callback(normalized);
            }
        }
        dialog.close();
    });

    dialog.present(Some(parent));
}

fn apply_entry_validation(label: &gtk::Label, validation: &BigEntryValidation) {
    label.set_text(&validation.message);
    label.remove_css_class("success");
    label.remove_css_class("error");
    if validation.message.is_empty() {
        return;
    }
    match validation.tone {
        BigEntryValidationTone::Neutral => {}
        BigEntryValidationTone::Success => label.add_css_class("success"),
        BigEntryValidationTone::Error => label.add_css_class("error"),
    }
}

/// Build a one-button error dialog with the close button as both
/// default and close response.
pub fn error_dialog(heading: &str, body: &str, close_label: &str) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .close_response("close")
        .default_response("close")
        .build();
    dialog.update_property(&[gtk::accessible::Property::Label(heading)]);
    dialog.add_response("close", close_label);
    dialog
}

/// Build a confirm dialog (cancel + confirm). Cancel response id is
/// hard-coded to `"cancel"`; use [`confirm_dialog_with_cancel_id`] to
/// override.
pub fn confirm_dialog(
    heading: &str,
    body: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    destructive: bool,
) -> adw::AlertDialog {
    confirm_dialog_with_cancel_id(
        heading,
        body,
        "cancel",
        cancel_label,
        confirm_id,
        confirm_label,
        destructive,
    )
}

/// Like [`confirm_dialog`] but lets the caller pick the cancel
/// response id. Useful when the host code needs to disambiguate
/// cancel from a different "no-op" outcome.
pub fn confirm_dialog_with_cancel_id(
    heading: &str,
    body: &str,
    cancel_id: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    destructive: bool,
) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .close_response(cancel_id)
        .default_response(cancel_id)
        .build();
    dialog.add_response(cancel_id, cancel_label);
    dialog.add_response(confirm_id, confirm_label);
    dialog.set_response_appearance(
        confirm_id,
        if destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    dialog
}

/// Build a cancelable choice dialog with one or more explicit action
/// responses. Use when a flow has more than one real action, such as
/// "Discard" and "Save", while still keeping a separate cancel/close response.
pub fn choice_dialog(
    heading: &str,
    body: &str,
    close_id: &str,
    close_label: &str,
    default_response: &str,
    responses: &[(&str, &str, adw::ResponseAppearance)],
) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .close_response(close_id)
        .default_response(default_response)
        .build();
    dialog.add_response(close_id, close_label);
    for (response_id, label, appearance) in responses {
        dialog.add_response(response_id, label);
        dialog.set_response_appearance(response_id, *appearance);
    }
    dialog
}

/// Build a single-entry input dialog with cancel + suggested confirm
/// buttons and an accessible label on the entry.
pub fn entry_dialog(
    heading: &str,
    body: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    placeholder: &str,
    accessible_label: &str,
) -> (adw::AlertDialog, gtk::Entry) {
    entry_choice_dialog(
        heading,
        body,
        cancel_label,
        confirm_id,
        placeholder,
        accessible_label,
        &[(
            confirm_id,
            confirm_label,
            adw::ResponseAppearance::Suggested,
        )],
    )
}

/// Build an entry dialog with multiple custom response buttons. Each
/// `(id, label, appearance)` triple becomes a button; `default_response`
/// receives initial focus.
pub fn entry_choice_dialog(
    heading: &str,
    body: &str,
    cancel_label: &str,
    default_response: &str,
    placeholder: &str,
    accessible_label: &str,
    responses: &[(&str, &str, adw::ResponseAppearance)],
) -> (adw::AlertDialog, gtk::Entry) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .close_response("cancel")
        .default_response(default_response)
        .build();
    dialog.add_response("cancel", cancel_label);
    for (response_id, label, appearance) in responses {
        dialog.add_response(response_id, label);
        dialog.set_response_appearance(response_id, *appearance);
    }

    let entry = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .hexpand(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    dialog.set_extra_child(Some(&entry));

    (dialog, entry)
}

/// Like [`entry_dialog`] but pre-populates the entry with `initial_text`
/// and selects the whole region so the user can retype or tweak in place.
pub fn entry_dialog_with_initial(
    heading: &str,
    body: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    placeholder: &str,
    initial_text: &str,
    accessible_label: &str,
) -> (adw::AlertDialog, gtk::Entry) {
    let (dialog, entry) = entry_dialog(
        heading,
        body,
        cancel_label,
        confirm_id,
        confirm_label,
        placeholder,
        accessible_label,
    );
    entry.set_text(initial_text);
    entry.select_region(0, -1);
    (dialog, entry)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConfirmAltDialogResponses<'a> {
    primary_id: &'a str,
    primary_appearance: adw::ResponseAppearance,
    alt_id: &'a str,
    alt_appearance: adw::ResponseAppearance,
    close_response: &'a str,
    default_response: &'a str,
}

fn confirm_alt_dialog_responses<'a>(
    primary_id: &'a str,
    primary_destructive: bool,
    alt_id: &'a str,
) -> ConfirmAltDialogResponses<'a> {
    ConfirmAltDialogResponses {
        primary_id,
        primary_appearance: if primary_destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
        alt_id,
        alt_appearance: adw::ResponseAppearance::Suggested,
        close_response: alt_id,
        default_response: alt_id,
    }
}

/// Build a two-button alternate-action dialog: no cancel, just `primary`
/// and `alt`. `alt` is the default/safer response (suggested style);
/// `primary` follows `primary_destructive`. Use when both buttons are
/// real actions (e.g. "Continue anyway" vs "Optimize first").
pub fn confirm_dialog_with_alt(
    heading: &str,
    body: &str,
    primary_id: &str,
    primary_label: &str,
    primary_destructive: bool,
    alt_id: &str,
    alt_label: &str,
) -> adw::AlertDialog {
    let responses = confirm_alt_dialog_responses(primary_id, primary_destructive, alt_id);
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .close_response(responses.close_response)
        .default_response(responses.default_response)
        .build();
    dialog.add_response(responses.alt_id, alt_label);
    dialog.add_response(responses.primary_id, primary_label);
    dialog.set_response_appearance(responses.primary_id, responses.primary_appearance);
    dialog.set_response_appearance(responses.alt_id, responses.alt_appearance);
    dialog
}

/// Build a confirm dialog that also exposes a "remember this choice" switch.
///
/// The dialog asks the user to confirm a destructive or sensitive action
/// (`heading` + `body`); the embedded [`adw::SwitchRow`] (`switch_title` /
/// `switch_subtitle`) lets them opt out of future prompts. The dialog
/// returns `cancel_id` when the cancel button is pressed and `confirm_id`
/// when confirmed; the switch state is read from the returned row.
pub fn confirm_switch_dialog(
    heading: &str,
    body: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    switch_title: &str,
    switch_subtitle: &str,
) -> (adw::AlertDialog, adw::SwitchRow) {
    let dialog = confirm_dialog(heading, body, cancel_label, confirm_id, confirm_label, true);
    let switch_row = adw::SwitchRow::builder()
        .title(switch_title)
        .subtitle(switch_subtitle)
        .active(false)
        .build();
    let extra = adw::PreferencesGroup::new();
    extra.add(&switch_row);
    dialog.set_extra_child(Some(&extra));

    (dialog, switch_row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires live GTK display; keep rendered GTK smoke out of ordinary unit gate"]
    fn entry_dialog_with_initial_populates_and_selects() {
        if gtk::init().is_err() {
            eprintln!("skip: no display");
            return;
        }
        let (_dialog, entry) = entry_dialog_with_initial(
            "Rename",
            "Pick a new name",
            "Cancel",
            "rename",
            "Rename",
            "New name",
            "photo.jpg",
            "New file name",
        );
        assert_eq!(entry.text().as_str(), "photo.jpg");
        assert!(entry.selection_bounds().is_some());
    }

    #[test]
    fn confirm_dialog_with_alt_response_contract_uses_alt_as_safe_default() {
        let responses = confirm_alt_dialog_responses("continue", true, "optimize");
        assert_eq!(responses.primary_id, "continue");
        assert_eq!(responses.alt_id, "optimize");
        assert_eq!(responses.default_response, "optimize");
        assert_eq!(responses.close_response, "optimize");
        assert_eq!(
            responses.primary_appearance,
            adw::ResponseAppearance::Destructive
        );
        assert_eq!(responses.alt_appearance, adw::ResponseAppearance::Suggested);
    }

    #[test]
    fn confirm_dialog_with_alt_response_contract_keeps_primary_suggested_when_safe() {
        let responses = confirm_alt_dialog_responses("continue", false, "optimize");
        assert_eq!(
            responses.primary_appearance,
            adw::ResponseAppearance::Suggested
        );
    }
}
