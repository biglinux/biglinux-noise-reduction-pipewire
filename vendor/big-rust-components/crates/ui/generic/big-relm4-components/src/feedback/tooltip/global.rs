// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Process-local tooltip facade functions.

use adw::prelude::*;
use relm4::gtk;

use super::{BigTooltipHandle, GLOBAL};

/// Attach a tooltip to a widget using the process-local helper.
pub fn set(widget: &impl IsA<gtk::Widget>, text: impl AsRef<str>) {
    let text = text.as_ref();
    if text.is_empty() {
        return;
    }
    with_global(|handle| handle.add(widget, text));
}

/// Attach a GTK-native tooltip through the shared tooltip policy for widgets
/// that are reparented or recycled by GTK containers.
///
/// The custom BigLinux tooltip owns a popover parented to the widget. That is
/// the preferred surface for stable widgets, but it is not safe for every GTK
/// recycling/reparenting path (`FlowBox` children, recycled gallery tiles).
/// Use this fallback only at those boundaries; it still keeps the AT-SPI label
/// in sync and keeps app code away from direct GTK tooltip calls.
pub fn set_reparent_safe_native(widget: &impl IsA<gtk::Widget>, text: &str) {
    if text.is_empty() {
        return;
    }
    let widget = widget.as_ref();
    with_global(|handle| handle.clear(widget));
    widget.set_property("tooltip-text", text);
    widget.update_property(&[gtk::accessible::Property::Label(text)]);
}

/// Attach a tooltip with vertical anchor offset.
pub fn set_with_offset(widget: &impl IsA<gtk::Widget>, text: &str, y_offset: i32) {
    if text.is_empty() {
        return;
    }
    with_global(|handle| handle.add_with_offset(widget, text, y_offset));
}

/// Update a previously attached tooltip, falling back to attach when needed.
pub fn update(widget: &impl IsA<gtk::Widget>, text: impl AsRef<str>) {
    let text = text.as_ref();
    with_global(|handle| handle.update_text(widget, text));
}

/// Attach a Pango-markup tooltip using the process-local helper.
pub fn set_markup(widget: &impl IsA<gtk::Widget>, markup: &str) {
    if markup.is_empty() {
        return;
    }
    with_global(|handle| handle.add_with_markup(widget, markup));
}

/// Update a previously attached tooltip's content as Pango markup.
pub fn update_markup(widget: &impl IsA<gtk::Widget>, markup: &str) {
    with_global(|handle| handle.update_markup(widget, markup));
}

/// Attach a Plasma-style hover card (Pango markup, follows the pointer).
pub fn set_card(widget: &impl IsA<gtk::Widget>, markup: &str) {
    if markup.is_empty() {
        return;
    }
    with_global(|handle| handle.add_card(widget, markup));
}

/// Attach a Plasma-style hover card with a leading icon tile.
pub fn set_card_with_icon(widget: &impl IsA<gtk::Widget>, icon: &str, markup: &str) {
    if markup.is_empty() {
        return;
    }
    with_global(|handle| handle.add_card_with_icon(widget, icon, markup));
}

/// Make every tooltip registered afterwards follow the pointer by default
/// (process-wide). Call once at app startup for a Plasma-style hover feel.
pub fn set_follow_default(follow: bool) {
    with_global(|handle| handle.set_follow_default(follow));
}

/// Show tooltips even when the root window is not active (shell panels).
pub fn set_show_when_inactive(show: bool) {
    with_global(|handle| handle.set_show_when_inactive(show));
}

/// Install an app-supplied hover-card stylesheet (see
/// [`BigTooltipHandle::set_surface_css`]). Empty string drops it.
pub fn set_surface_css(css: &str) {
    with_global(|handle| handle.set_surface_css(css));
}

/// Remove a tooltip attached through this module.
pub fn clear(widget: &impl IsA<gtk::Widget>) {
    with_global(|handle| handle.clear(widget));
}

/// Hide every active custom tooltip immediately.
pub fn hide_all() {
    with_global(BigTooltipHandle::hide_all);
}

/// Enable or disable tooltip display globally.
pub fn set_enabled(enabled: bool) {
    with_global(|handle| handle.set_enabled(enabled));
}

fn with_global<F: FnOnce(&BigTooltipHandle)>(f: F) {
    GLOBAL.with(|cell| {
        let mut handle = cell.borrow_mut();
        let handle = handle.get_or_insert_with(BigTooltipHandle::default);
        f(handle);
    });
}
