// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared wrapper for small, bounded, local `GtkListBox` surfaces.
//!
//! Use this only for image-free lists whose rows come from local user actions
//! and are expected to stay small, such as dialog rows, active conversion queues,
//! and short action lists. Network-fed, search-result, image-bearing, or
//! unbounded lists must use a virtualized `ListView`, `GridView`, or
//! `ColumnView` over a `GListModel`.

use relm4::gtk;
use relm4::gtk::prelude::*;

/// Visual and policy settings for [`BigBoundedListBox`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct BigBoundedListBoxSpec {
    /// Expected row count that still fits the bounded-list contract.
    pub expected_max_rows: u32,
    /// Selection mode applied to the root list box.
    pub selection_mode: gtk::SelectionMode,
    /// CSS classes applied to the root list box.
    pub css_classes: Vec<String>,
    /// Initial widget visibility.
    pub visible: bool,
    /// Start margin in logical pixels.
    pub margin_start: i32,
    /// End margin in logical pixels.
    pub margin_end: i32,
    /// Top margin in logical pixels.
    pub margin_top: i32,
    /// Bottom margin in logical pixels.
    pub margin_bottom: i32,
}

impl BigBoundedListBoxSpec {
    /// Create a spec for a bounded local list.
    #[must_use]
    pub fn new(expected_max_rows: u32) -> Self {
        Self {
            expected_max_rows,
            selection_mode: gtk::SelectionMode::None,
            css_classes: Vec::new(),
            visible: true,
            margin_start: 0,
            margin_end: 0,
            margin_top: 0,
            margin_bottom: 0,
        }
    }

    /// Set the selection mode.
    #[must_use]
    pub fn selection_mode(mut self, selection_mode: gtk::SelectionMode) -> Self {
        self.selection_mode = selection_mode;
        self
    }

    /// Add one CSS class.
    #[must_use]
    pub fn css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_classes.push(css_class.into());
        self
    }

    /// Set initial visibility.
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Set logical margins.
    #[must_use]
    pub fn margins(mut self, start: i32, end: i32, top: i32, bottom: i32) -> Self {
        self.margin_start = start.max(0);
        self.margin_end = end.max(0);
        self.margin_top = top.max(0);
        self.margin_bottom = bottom.max(0);
        self
    }
}

/// Small local list-box surface with one shared clear/remove/append policy.
#[derive(Debug, Clone)]
pub struct BigBoundedListBox {
    root: gtk::ListBox,
    expected_max_rows: u32,
}

impl BigBoundedListBox {
    /// Build a bounded list box from `spec`.
    #[must_use]
    pub fn new(spec: BigBoundedListBoxSpec) -> Self {
        let css_classes: Vec<&str> = spec.css_classes.iter().map(String::as_str).collect();
        let root = gtk::ListBox::builder()
            .selection_mode(spec.selection_mode)
            .css_classes(css_classes)
            .visible(spec.visible)
            .margin_start(spec.margin_start)
            .margin_end(spec.margin_end)
            .margin_top(spec.margin_top)
            .margin_bottom(spec.margin_bottom)
            .build();

        Self {
            root,
            expected_max_rows: spec.expected_max_rows,
        }
    }

    /// Borrow the root `GtkListBox`.
    #[must_use]
    pub fn root(&self) -> &gtk::ListBox {
        &self.root
    }

    /// Expected row count for the bounded-list contract.
    #[must_use]
    pub fn expected_max_rows(&self) -> u32 {
        self.expected_max_rows
    }

    /// Append a row widget.
    pub fn append(&self, row: &impl IsA<gtk::Widget>) {
        self.root.append(row);
    }

    /// Insert a row widget at `position`.
    pub fn insert(&self, row: &impl IsA<gtk::Widget>, position: i32) {
        self.root.insert(row, position);
    }

    /// Remove a row widget.
    pub fn remove(&self, row: &impl IsA<gtk::Widget>) {
        self.root.remove(row);
    }

    /// Set root visibility.
    pub fn set_visible(&self, visible: bool) {
        self.root.set_visible(visible);
    }

    /// Remove every row from the list.
    pub fn clear(&self) {
        clear_list_box_rows(&self.root);
    }
}

/// Remove every row from a `GtkListBox`.
pub fn clear_list_box_rows(list: &gtk::ListBox) {
    while let Some(row) = list.first_child() {
        list.remove(&row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_to_non_selectable_visible_list() {
        let spec = BigBoundedListBoxSpec::new(24);

        assert_eq!(spec.expected_max_rows, 24);
        assert_eq!(spec.selection_mode, gtk::SelectionMode::None);
        assert!(spec.css_classes.is_empty());
        assert!(spec.visible);
    }

    #[test]
    fn spec_clamps_negative_margins() {
        let spec = BigBoundedListBoxSpec::new(8).margins(-1, -2, 3, 4);

        assert_eq!(spec.margin_start, 0);
        assert_eq!(spec.margin_end, 0);
        assert_eq!(spec.margin_top, 3);
        assert_eq!(spec.margin_bottom, 4);
    }
}
