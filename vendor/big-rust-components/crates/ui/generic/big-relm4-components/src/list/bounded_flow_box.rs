// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared wrapper for small, bounded `GtkFlowBox` icon grids.
//!
//! Use this for finite local icon/template pickers. Unbounded, network-fed, or
//! large result sets still belong on virtualized `GridView`/`ListView` models.

use relm4::gtk;
use relm4::gtk::prelude::*;

/// Visual and policy settings for [`BigBoundedFlowBox`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct BigBoundedFlowBoxSpec {
    /// Expected item count that still fits the bounded-grid contract.
    pub expected_max_items: u32,
    /// Selection mode applied to the flow box.
    pub selection_mode: gtk::SelectionMode,
    /// Whether children use homogeneous sizing.
    pub homogeneous: bool,
    /// Minimum children per line.
    pub min_children_per_line: u32,
    /// Maximum children per line.
    pub max_children_per_line: u32,
    /// Row spacing.
    pub row_spacing: u32,
    /// Column spacing.
    pub column_spacing: u32,
    /// Initial widget visibility.
    pub visible: bool,
}

impl BigBoundedFlowBoxSpec {
    /// Create a spec for a bounded local flow grid.
    #[must_use]
    pub fn new(expected_max_items: u32) -> Self {
        Self {
            expected_max_items,
            selection_mode: gtk::SelectionMode::None,
            homogeneous: false,
            min_children_per_line: 1,
            max_children_per_line: 6,
            row_spacing: 0,
            column_spacing: 0,
            visible: true,
        }
    }

    /// Set the selection mode.
    #[must_use]
    pub fn selection_mode(mut self, selection_mode: gtk::SelectionMode) -> Self {
        self.selection_mode = selection_mode;
        self
    }

    /// Set homogeneous child sizing.
    #[must_use]
    pub fn homogeneous(mut self, homogeneous: bool) -> Self {
        self.homogeneous = homogeneous;
        self
    }

    /// Set children-per-line bounds.
    #[must_use]
    pub fn children_per_line(mut self, minimum: u32, maximum: u32) -> Self {
        self.min_children_per_line = minimum.max(1);
        self.max_children_per_line = maximum.max(self.min_children_per_line);
        self
    }

    /// Set row and column spacing.
    #[must_use]
    pub fn spacing(mut self, row_spacing: u32, column_spacing: u32) -> Self {
        self.row_spacing = row_spacing;
        self.column_spacing = column_spacing;
        self
    }

    /// Set initial visibility.
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

/// Small local flow-box surface with one shared clear/append policy.
#[derive(Debug, Clone)]
pub struct BigBoundedFlowBox {
    root: gtk::FlowBox,
    expected_max_items: u32,
}

impl BigBoundedFlowBox {
    /// Build a bounded flow box from `spec`.
    #[must_use]
    pub fn new(spec: BigBoundedFlowBoxSpec) -> Self {
        let root = gtk::FlowBox::builder()
            .selection_mode(spec.selection_mode)
            .homogeneous(spec.homogeneous)
            .min_children_per_line(spec.min_children_per_line)
            .max_children_per_line(spec.max_children_per_line)
            .row_spacing(spec.row_spacing)
            .column_spacing(spec.column_spacing)
            .visible(spec.visible)
            .build();

        Self {
            root,
            expected_max_items: spec.expected_max_items,
        }
    }

    /// Borrow the root `GtkFlowBox`.
    #[must_use]
    pub fn root(&self) -> &gtk::FlowBox {
        &self.root
    }

    /// Expected item count for the bounded-grid contract.
    #[must_use]
    pub fn expected_max_items(&self) -> u32 {
        self.expected_max_items
    }

    /// Append a child widget.
    pub fn append(&self, child: &impl IsA<gtk::Widget>) {
        self.root.append(child);
    }

    /// Set root visibility.
    pub fn set_visible(&self, visible: bool) {
        self.root.set_visible(visible);
    }

    /// Remove every child from the flow box.
    pub fn clear(&self) {
        clear_flow_box_children(&self.root);
    }
}

/// Remove every child from a `GtkFlowBox`.
pub fn clear_flow_box_children(flow_box: &gtk::FlowBox) {
    while let Some(child) = flow_box.first_child() {
        flow_box.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_to_visible_non_selectable_grid() {
        let spec = BigBoundedFlowBoxSpec::new(12);

        assert_eq!(spec.expected_max_items, 12);
        assert_eq!(spec.selection_mode, gtk::SelectionMode::None);
        assert!(!spec.homogeneous);
        assert_eq!(spec.min_children_per_line, 1);
        assert_eq!(spec.max_children_per_line, 6);
        assert!(spec.visible);
    }

    #[test]
    fn children_per_line_keeps_valid_bounds() {
        let spec = BigBoundedFlowBoxSpec::new(12).children_per_line(4, 2);

        assert_eq!(spec.min_children_per_line, 4);
        assert_eq!(spec.max_children_per_line, 4);
    }
}
