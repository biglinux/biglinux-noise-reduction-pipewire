// SPDX-License-Identifier: MIT

//! Geometry helpers for tab context menus and click-to-move placement.

use adw::prelude::*;
use relm4::gtk::Widget;

use super::DropPlacement;

/// Toggle `class` on the toplevel `adw::ApplicationWindow` of `anchor` (used to
/// mark the window while a tab context menu is open).
pub fn set_tab_context_menu_window_state(anchor: &impl IsA<Widget>, open: bool, class: &str) {
    let Some(window) = anchor.root().and_downcast::<adw::ApplicationWindow>() else {
        return;
    };
    if open {
        window.add_css_class(class);
    } else {
        window.remove_css_class(class);
    }
}

#[allow(clippy::cast_possible_truncation)]
pub(super) fn clamp_to_i32(value: f64) -> i32 {
    let rounded = value.round();
    if rounded >= f64::from(i32::MAX) {
        i32::MAX
    } else if rounded <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        rounded as i32
    }
}

/// Classify a pointer `x` over a tab of `tab_width` into a drop placement
/// (outer thirds = before/after, middle third = onto).
pub(super) fn drop_placement_for_pointer(x: f64, tab_width: f64) -> DropPlacement {
    if tab_width <= 0.0 {
        return DropPlacement::Right;
    }
    let edge_width = tab_width / 3.0;
    if x < edge_width {
        DropPlacement::Left
    } else if x >= tab_width - edge_width {
        DropPlacement::Right
    } else {
        DropPlacement::Inside
    }
}
