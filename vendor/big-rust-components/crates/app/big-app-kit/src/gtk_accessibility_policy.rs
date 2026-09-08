// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Minimal accessible-label policy for legacy `big-app-kit` GTK helpers.
//!
//! New reusable GTK widgets belong in `big-relm4-components`. This module only
//! keeps existing `big-app-kit` helpers independent from that crate so
//! `big-relm4-components` can consume display-free `big-app-kit` contracts.

use adw::prelude::*;
use relm4::gtk;

pub(crate) fn set_accessible_label<W>(widget: &W, label: &str)
where
    W: IsA<gtk::Widget> + IsA<gtk::Accessible>,
{
    widget.update_property(&[gtk::accessible::Property::Label(label)]);
}

pub(crate) fn icon_button(icon_name: &str, label: &str, classes: &[&str]) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(&gtk::Image::from_icon_name(icon_name))
        .css_classes(classes)
        .build();
    set_accessible_label(&button, label);
    button
}
