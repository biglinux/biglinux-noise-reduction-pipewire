// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::gio;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

use super::BigMenuActionItem;

pub(super) fn build_action_menu_row(
    action_context: &gtk::Widget,
    popover: &gtk::Popover,
    item: &BigMenuActionItem,
) -> gtk::Button {
    let row_content = build_action_row_content(item);
    let label = row_content.label.clone();

    let row = gtk::Button::builder()
        .child(&row_content.root)
        .css_classes(["flat", "model"])
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .build();
    row.update_property(&[gtk::accessible::Property::Label(&item.label)]);
    row.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
    row.set_sensitive(item.enabled);

    let action = item.action.clone();
    let weak_action_context = action_context.downgrade();
    let weak_popover = popover.downgrade();
    row.connect_clicked(move |_| {
        if let Some(popover) = weak_popover.upgrade() {
            popover.popdown();
        }
        if let Some(action_context) = weak_action_context.upgrade() {
            let _ = activate_detailed_action(&action_context, &action);
        }
    });
    row
}

pub(super) fn build_action_panel_button(
    action_context: &gtk::Widget,
    item: &BigMenuActionItem,
    close_panel: Rc<dyn Fn()>,
) -> gtk::Button {
    let row_content = build_action_row_content(item);
    let label = row_content.label.clone();

    let row = gtk::Button::builder()
        .child(&row_content.root)
        .css_classes(["flat"])
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .build();
    row.update_property(&[gtk::accessible::Property::Label(&item.label)]);
    row.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
    row.set_sensitive(item.enabled);

    let action = item.action.clone();
    let weak_action_context = action_context.downgrade();
    row.connect_clicked(move |_| {
        close_panel();
        if let Some(action_context) = weak_action_context.upgrade() {
            let _ = activate_detailed_action(&action_context, &action);
        }
    });
    row
}

struct ActionRowContent {
    root: gtk::Box,
    label: gtk::Label,
}

fn build_action_row_content(action_item: &BigMenuActionItem) -> ActionRowContent {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .hexpand(true)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();

    if let Some(icon_name) = &action_item.leading_icon_name {
        let icon = gtk::Image::from_icon_name(icon_name);
        icon.set_pixel_size(16);
        icon.set_valign(gtk::Align::Center);
        root.append(&icon);
    }

    let label = gtk::Label::new(Some(&action_item.label));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    root.append(&label);

    if let Some(shortcut_label) = &action_item.shortcut_label {
        let shortcut = gtk::Label::new(Some(shortcut_label));
        shortcut.set_xalign(1.0);
        shortcut.add_css_class("dim-label");
        root.append(&shortcut);
    }

    ActionRowContent { root, label }
}

fn activate_detailed_action(action_context: &gtk::Widget, detailed_action: &str) -> bool {
    match parse_detailed_action(detailed_action) {
        Ok((action_name, target_value)) => action_context
            .activate_action(&action_name, target_value.as_ref())
            .is_ok(),
        Err(_) => action_context
            .activate_action(detailed_action, None)
            .is_ok(),
    }
}

pub(super) fn parse_detailed_action(
    detailed_action: &str,
) -> Result<(String, Option<glib::Variant>), glib::Error> {
    gio::Action::parse_detailed_name(detailed_action)
        .map(|(action_name, target_value)| (action_name.to_string(), target_value))
}
