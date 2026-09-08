// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Tooltip-aware icon button builders.

use adw::prelude::*;
use relm4::gtk;

/// Build an icon-only button with tooltip and accessible label set together.
#[must_use]
pub fn icon_button(icon_name: &str, label: &str, classes: &[&str]) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(&gtk::Image::from_icon_name(icon_name))
        .css_classes(classes)
        .build();
    super::set(&button, label);
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

/// Build an icon-only menu button with tooltip and accessible label set together.
#[must_use]
pub fn icon_menu_button(icon_name: &str, label: &str, classes: &[&str]) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .icon_name(icon_name)
        .css_classes(classes)
        .build();
    super::set(&button, label);
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

/// Build a text menu button with tooltip and accessible label set together.
#[must_use]
pub fn labeled_menu_button(label: &str, tooltip: &str, classes: &[&str]) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .label(label)
        .css_classes(classes)
        .build();
    super::set(&button, tooltip);
    button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    button
}

/// Build a menu button with app-owned child content and a shared tooltip and
/// accessible label policy.
#[must_use]
pub fn custom_menu_button(
    child: &impl IsA<gtk::Widget>,
    label: &str,
    classes: &[&str],
) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .child(child)
        .css_classes(classes)
        .build();
    super::set(&button, label);
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

/// Build an icon-only toggle button with tooltip and accessible label set together.
#[must_use]
pub fn icon_toggle_button(
    icon_name: &str,
    label: &str,
    active: bool,
    classes: &[&str],
) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::builder()
        .child(&gtk::Image::from_icon_name(icon_name))
        .css_classes(classes)
        .active(active)
        .build();
    super::set(&button, label);
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

/// Build a button with an icon, text label, tooltip, and accessible label.
#[must_use]
pub fn labeled_icon_button(
    icon_name: &str,
    label: &str,
    tooltip: &str,
    classes: &[&str],
) -> gtk::Button {
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    inner.append(&gtk::Image::from_icon_name(icon_name));
    inner.append(&gtk::Label::new(Some(label)));

    let button = gtk::Button::builder()
        .child(&inner)
        .css_classes(classes)
        .build();
    super::set(&button, tooltip);
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}
