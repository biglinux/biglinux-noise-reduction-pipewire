// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared container mutation helpers.

use adw::prelude::*;
use relm4::gtk;

/// Remove every child from a `GtkBox`.
pub fn clear_box_children(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// Remove every child from a `GtkGrid`.
pub fn clear_grid_children(grid: &gtk::Grid) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
}

/// Remove every page widget from an `AdwViewStack` and return the count.
pub fn clear_view_stack_pages(stack: &adw::ViewStack) -> usize {
    let mut removed = 0;
    while let Some(child) = stack.first_child() {
        stack.remove(&child);
        removed += 1;
    }
    removed
}

/// Counts produced while detaching runtime widget references from a subtree.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BigWidgetDetachReport {
    /// Search bars whose key-capture widget or child was cleared.
    pub search_bars: usize,
    /// List boxes whose filter and rows were cleared.
    pub list_boxes: usize,
    /// List box rows removed from cleared list boxes.
    pub list_box_rows: usize,
    /// View stacks whose page widgets were removed.
    pub view_stacks: usize,
    /// Page widgets removed from view stacks.
    pub stack_children: usize,
    /// Navigation split views whose sidebar/content pages were detached.
    pub navigation_splits: usize,
    /// Scrolled windows whose child widget was cleared.
    pub scrolled_windows: usize,
}

impl BigWidgetDetachReport {
    /// Total number of container widgets whose child-owning links were cleared.
    #[must_use]
    pub fn detached_containers(&self) -> usize {
        self.search_bars
            + self.list_boxes
            + self.view_stacks
            + self.navigation_splits
            + self.scrolled_windows
    }
}

/// Detach common GTK/libadwaita runtime links in `root`'s subtree.
///
/// This helper is intended for dialog/window teardown paths that build large
/// transient widget trees with search bars, list filters, stacks, split views,
/// and scrollers. It clears back-references and child-owning links that can keep
/// a closed subtree alive longer than the caller expects.
pub fn detach_runtime_widget_links(root: &gtk::Widget) -> BigWidgetDetachReport {
    let mut report = BigWidgetDetachReport::default();
    detach_runtime_widget_links_into(root, &mut report);
    report
}

fn detach_runtime_widget_links_into(root: &gtk::Widget, report: &mut BigWidgetDetachReport) {
    if let Some(list_box) = root.downcast_ref::<gtk::ListBox>() {
        list_box.set_filter_func(|_| true);
    }

    let mut child = root.first_child();
    while let Some(widget) = child {
        detach_runtime_widget_links_into(&widget, report);
        child = widget.next_sibling();
    }

    if let Some(list_box) = root.downcast_ref::<gtk::ListBox>() {
        let mut rows = 0;
        while let Some(row) = list_box.row_at_index(0) {
            list_box.remove(&row);
            rows += 1;
        }
        report.list_box_rows += rows;
        report.list_boxes += 1;
    }
    if let Some(stack) = root.downcast_ref::<adw::ViewStack>() {
        report.view_stacks += 1;
        report.stack_children += clear_view_stack_pages(stack);
    }
    if let Some(split) = root.downcast_ref::<adw::NavigationSplitView>() {
        split.set_sidebar(None::<&adw::NavigationPage>);
        split.set_content(None::<&adw::NavigationPage>);
        report.navigation_splits += 1;
    }
    if let Some(search_bar) = root.downcast_ref::<gtk::SearchBar>() {
        search_bar.set_key_capture_widget(None::<&gtk::Widget>);
        search_bar.set_child(None::<&gtk::Widget>);
        report.search_bars += 1;
    }
    if let Some(scrolled) = root.downcast_ref::<gtk::ScrolledWindow>() {
        scrolled.set_child(None::<&gtk::Widget>);
        report.scrolled_windows += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::BigWidgetDetachReport;

    #[test]
    fn detach_report_counts_detached_containers() {
        let report = BigWidgetDetachReport {
            search_bars: 1,
            list_boxes: 2,
            list_box_rows: 5,
            view_stacks: 3,
            stack_children: 8,
            navigation_splits: 4,
            scrolled_windows: 6,
        };

        assert_eq!(report.detached_containers(), 16);
    }
}
