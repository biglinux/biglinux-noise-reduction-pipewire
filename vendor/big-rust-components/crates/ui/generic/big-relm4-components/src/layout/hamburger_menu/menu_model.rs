// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! GMenuModel conversion helpers for explicit action popovers.

use relm4::gtk::gio;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

use super::{BigMenuActionGroup, BigMenuActionItem};

/// Collect detailed action names from a menu model recursively.
#[must_use]
pub fn menu_action_names(menu: &impl IsA<gio::MenuModel>) -> Vec<String> {
    let model = menu.as_ref();
    let mut actions = Vec::new();
    for index in 0..model.n_items() {
        if let Some(action) = menu_item_string(model, index, "action") {
            actions.push(action);
        }
        if let Some(section) = model.item_link(index, "section") {
            actions.extend(menu_action_names(&section));
        }
        if let Some(submenu) = model.item_link(index, "submenu") {
            actions.extend(menu_action_names(&submenu));
        }
    }
    actions
}

/// Flatten a `GMenuModel` into named action rows for accessible popovers.
///
/// Native `GtkPopoverMenu` can expose unnamed `menu item` nodes through AT-SPI
/// in some GTK/portal combinations. This preserves the existing menu model as
/// the source of truth while rendering explicit rows with stable names.
#[must_use]
pub fn flat_action_items_from_menu_model(
    menu: &impl IsA<gio::MenuModel>,
) -> Vec<BigMenuActionItem> {
    let model = menu.as_ref();
    let mut items = Vec::new();
    for index in 0..model.n_items() {
        if let Some(section) = model.item_link(index, "section") {
            items.extend(flat_action_items_from_menu_model(&section));
        }
        if let Some(submenu) = model.item_link(index, "submenu") {
            items.extend(flat_action_items_from_menu_model(&submenu));
        }
        let Some(label) = menu_item_string(model, index, "label") else {
            continue;
        };
        let Some(action_name) = menu_item_string(model, index, "action") else {
            continue;
        };
        let target_value = menu_item_target(model, index);
        let Some(detailed_action) = detailed_action_name(&action_name, target_value.as_ref())
        else {
            continue;
        };
        items.push(BigMenuActionItem::new(label, detailed_action.to_string()));
    }
    items
}

/// Convert a `GMenuModel` into grouped custom action rows.
#[must_use]
pub fn action_groups_from_menu_model(menu: &impl IsA<gio::MenuModel>) -> Vec<BigMenuActionGroup> {
    let mut groups = Vec::new();
    collect_action_groups_from_menu_model(menu.as_ref(), None, &mut groups);
    groups
}

fn menu_item_string(
    menu: &impl IsA<gio::MenuModel>,
    index: i32,
    attribute: &str,
) -> Option<String> {
    menu.item_attribute_value(index, attribute, Some(glib::VariantTy::STRING))
        .and_then(|value| value.get::<String>())
}

fn menu_item_target(menu: &impl IsA<gio::MenuModel>, index: i32) -> Option<glib::Variant> {
    menu.item_attribute_value(index, "target", None)
}

pub(super) fn build_custom_grouped_action_menu_model(groups: &[BigMenuActionGroup]) -> gio::Menu {
    use glib::variant::ToVariant;

    let menu = gio::Menu::new();
    let mut next_custom_index = 0;
    for group in groups {
        if group.items.is_empty() {
            continue;
        }
        let section = gio::Menu::new();
        for item in &group.items {
            let menu_item = gio::MenuItem::new(Some(&item.label), None);
            menu_item.set_attribute_value(
                "custom",
                Some(&custom_action_child_id(next_custom_index).to_variant()),
            );
            section.append_item(&menu_item);
            next_custom_index += 1;
        }
        menu.append_section(group.heading.as_deref(), &section);
    }
    menu
}

pub(super) fn build_custom_action_menu_model_from_menu(
    menu: &impl IsA<gio::MenuModel>,
) -> (gio::Menu, Vec<BigMenuActionItem>) {
    let mut action_items = Vec::new();
    let custom_menu = copy_menu_model_with_custom_action_items(menu.as_ref(), &mut action_items);
    (custom_menu, action_items)
}

fn copy_menu_model_with_custom_action_items(
    menu: &gio::MenuModel,
    action_items: &mut Vec<BigMenuActionItem>,
) -> gio::Menu {
    use glib::variant::ToVariant;

    let custom_menu = gio::Menu::new();
    for index in 0..menu.n_items() {
        if let Some(section) = menu.item_link(index, "section") {
            let custom_section = copy_menu_model_with_custom_action_items(&section, action_items);
            if custom_section.n_items() > 0 {
                custom_menu.append_section(
                    menu_model_label_attribute(menu, index).as_deref(),
                    &custom_section,
                );
            }
            continue;
        }

        if let Some(submenu) = menu.item_link(index, "submenu") {
            let custom_submenu = copy_menu_model_with_custom_action_items(&submenu, action_items);
            if custom_submenu.n_items() > 0 {
                custom_menu.append_submenu(
                    menu_model_label_attribute(menu, index).as_deref(),
                    &custom_submenu,
                );
            }
            continue;
        }

        if let Some(item) = menu_model_action_item(menu, index) {
            let menu_item = gio::MenuItem::new(Some(&item.label), None);
            menu_item.set_attribute_value(
                "custom",
                Some(&custom_action_child_id(action_items.len()).to_variant()),
            );
            custom_menu.append_item(&menu_item);
            action_items.push(item);
        }
    }
    custom_menu
}

pub(super) fn custom_action_child_id(index: usize) -> String {
    format!("big-action-{index}")
}

fn collect_action_groups_from_menu_model(
    menu: &gio::MenuModel,
    inherited_heading: Option<String>,
    groups: &mut Vec<BigMenuActionGroup>,
) {
    let mut direct_items = Vec::new();
    for index in 0..menu.n_items() {
        if let Some(section) = menu.item_link(index, "section") {
            let heading =
                menu_model_label_attribute(menu, index).or_else(|| inherited_heading.clone());
            collect_action_groups_from_menu_model(&section, heading, groups);
            continue;
        }
        if let Some(submenu) = menu.item_link(index, "submenu") {
            let heading =
                menu_model_label_attribute(menu, index).or_else(|| inherited_heading.clone());
            let items = flat_action_items_from_menu_model(&submenu);
            if !items.is_empty() {
                groups.push(BigMenuActionGroup::new(heading, items));
            }
            continue;
        }
        if let Some(item) = menu_model_action_item(menu, index) {
            direct_items.push(item);
        }
    }
    if !direct_items.is_empty() {
        groups.push(BigMenuActionGroup::new(inherited_heading, direct_items));
    }
}

pub(super) fn menu_model_label_attribute(menu: &gio::MenuModel, index: i32) -> Option<String> {
    menu.item_attribute_value(index, "label", Some(glib::VariantTy::STRING))
        .and_then(|value| value.get::<String>())
}

pub(super) fn menu_model_action_item(
    menu: &gio::MenuModel,
    index: i32,
) -> Option<BigMenuActionItem> {
    let label = menu_model_label_attribute(menu, index)?;
    let action_name = menu_item_string(menu, index, "action")?;
    let target_value = menu_item_target(menu, index);
    let detailed_action = detailed_action_name(&action_name, target_value.as_ref())?;
    Some(BigMenuActionItem::new(label, detailed_action.to_string()))
}

fn detailed_action_name(
    action_name: &str,
    target_value: Option<&glib::Variant>,
) -> Option<glib::GString> {
    gio::Action::name_is_valid(action_name)
        .then(|| gio::Action::print_detailed_name(action_name, target_value))
}

#[cfg(test)]
pub(super) fn menu_item_labels(items: &[BigMenuActionItem]) -> Vec<String> {
    items.iter().map(|item| item.label.clone()).collect()
}
