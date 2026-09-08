// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Suite dialogs for named layouts (a saved set of files/folders/sessions):
//! a name-entry save dialog and a list dialog that applies or deletes
//! entries. Storage and the meaning of an item stay app-side; strings live
//! in the `big-components` textdomain.

use std::collections::BTreeMap;

use adw::prelude::*;
use relm4::gtk;
use serde_json::Value;

use crate::i18n::t;

/// Save callback: layout name + the captured items.
pub type SaveLayoutCallback = Box<dyn Fn(&str, &[Value])>;
/// Apply callback: the stored items of the activated layout.
pub type ApplyLayoutCallback = Box<dyn Fn(&[Value])>;
/// Delete callback: the layout name.
pub type DeleteLayoutCallback = Box<dyn Fn(&str)>;

/// Ask for a name and hand the captured `items` to `on_save`. With no items,
/// explains why there is nothing to save instead of offering an empty save.
pub fn present_save_layout_dialog(
    parent: Option<&gtk::Window>,
    items: Vec<Value>,
    on_save: SaveLayoutCallback,
) {
    let dialog = adw::AlertDialog::new(
        Some(&t("Save Layout")),
        Some(&if items.is_empty() {
            t("Nothing to capture yet — open what the layout should contain first.")
        } else {
            t("Save what is open now as a layout you can reopen from the Layouts dialog.")
        }),
    );
    if items.is_empty() {
        dialog.add_response("close", &t("Close"));
        dialog.present(parent);
        return;
    }
    let entry = gtk::Entry::builder()
        .placeholder_text(t("Layout name"))
        .activates_default(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(&t("Layout name"))]);
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", &t("Cancel"));
    dialog.add_response("save", &t("Save"));
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response != "save" {
            return;
        }
        let name = entry.text().trim().to_owned();
        if name.is_empty() {
            return;
        }
        on_save(&name, &items);
    });
    dialog.present(parent);
}

/// List the saved `layouts`: activating a row runs `on_apply(items)` and
/// closes; each row carries a delete button running `on_delete(name)`.
/// `subtitle_for` renders the per-row item summary (e.g. "3 files").
pub fn present_layouts_dialog(
    parent: Option<&gtk::Window>,
    layouts: BTreeMap<String, Vec<Value>>,
    subtitle_for: &dyn Fn(usize) -> String,
    on_apply: ApplyLayoutCallback,
    on_delete: DeleteLayoutCallback,
) {
    if layouts.is_empty() {
        let dialog = adw::AlertDialog::new(
            Some(&t("Layouts")),
            Some(&t(
                "No saved layouts yet. Use \u{201c}Save Layout\u{2026}\u{201d} first.",
            )),
        );
        dialog.add_response("close", &t("Close"));
        dialog.present(parent);
        return;
    }
    let dialog = adw::AlertDialog::new(
        Some(&t("Layouts")),
        Some(&t(
            "Open a saved layout in this window — entries that no longer exist are skipped.",
        )),
    );
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.update_property(&[gtk::accessible::Property::Label(&t("Saved layouts"))]);
    let on_apply = std::rc::Rc::new(on_apply);
    let on_delete = std::rc::Rc::new(on_delete);
    for (name, items) in layouts {
        let row = adw::ActionRow::builder()
            .title(&name)
            .subtitle(subtitle_for(items.len()))
            .activatable(true)
            .build();
        let delete = gtk::Button::from_icon_name("user-trash-symbolic");
        delete.add_css_class("flat");
        delete.set_valign(gtk::Align::Center);
        let delete_label = t("Delete layout");
        crate::feedback::tooltip::set(&delete, &delete_label);
        delete.update_property(&[gtk::accessible::Property::Label(&delete_label)]);
        {
            let on_delete = on_delete.clone();
            let name = name.clone();
            let list = list.clone();
            let row = row.clone();
            delete.connect_clicked(move |_| {
                on_delete(&name);
                list.remove(&row);
            });
        }
        row.add_suffix(&delete);
        {
            let on_apply = on_apply.clone();
            let dialog = dialog.clone();
            row.connect_activated(move |_| {
                on_apply(&items);
                dialog.close();
            });
        }
        list.append(&row);
    }
    dialog.set_extra_child(Some(&list));
    dialog.add_response("close", &t("Close"));
    dialog.present(parent);
}
