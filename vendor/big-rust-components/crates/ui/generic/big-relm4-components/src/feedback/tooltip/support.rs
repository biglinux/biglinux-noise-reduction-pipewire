// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Small GTK object helpers for tooltip lifecycle code.

use adw::prelude::*;
use relm4::gtk;
use relm4::gtk::glib;
use std::cell::RefCell;

pub(super) fn live_widgets(widgets: &RefCell<Vec<glib::WeakRef<gtk::Widget>>>) -> Vec<gtk::Widget> {
    widgets
        .borrow()
        .iter()
        .filter_map(glib::WeakRef::upgrade)
        .collect()
}

pub(super) fn root_window_active(widget: &gtk::Widget) -> bool {
    widget
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
        .is_none_or(|window| window.is_active())
}
