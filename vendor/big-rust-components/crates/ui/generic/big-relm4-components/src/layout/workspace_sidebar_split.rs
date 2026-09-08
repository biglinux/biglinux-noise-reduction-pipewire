// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared sidebar split for workspace windows.
//!
//! This widget owns the reusable `AdwOverlaySplitView` setup used when a
//! workspace has an app-owned sidebar and app-owned content. Consumers keep
//! their sidebar model, visibility policy, and action wiring; the framework owns
//! initial split geometry, root CSS classes, and root accessibility metadata.

use adw::prelude::*;
use relm4::gtk;

/// Display contract for a workspace sidebar split.
#[derive(Debug, Clone, PartialEq)]
pub struct BigWorkspaceSidebarSplitSpec {
    min_sidebar_width: f64,
    max_sidebar_width: f64,
    shows_sidebar_initially: bool,
    pins_sidebar_initially: bool,
    is_collapsed_initially: bool,
    accessible_name: Option<String>,
    css_classes: Vec<String>,
}

impl Default for BigWorkspaceSidebarSplitSpec {
    fn default() -> Self {
        Self {
            min_sidebar_width: 220.0,
            max_sidebar_width: 360.0,
            shows_sidebar_initially: false,
            pins_sidebar_initially: false,
            is_collapsed_initially: true,
            accessible_name: Some("Workspace sidebar split".to_owned()),
            css_classes: Vec::new(),
        }
    }
}

impl BigWorkspaceSidebarSplitSpec {
    /// Create the default workspace sidebar split spec.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the lower sidebar width bound.
    #[must_use]
    pub fn min_sidebar_width(mut self, width: f64) -> Self {
        self.min_sidebar_width = width;
        self
    }

    /// Set the upper sidebar width bound.
    #[must_use]
    pub fn max_sidebar_width(mut self, width: f64) -> Self {
        self.max_sidebar_width = width;
        self
    }

    /// Set the initial sidebar visibility.
    #[must_use]
    pub fn shows_sidebar_initially(mut self, shows_sidebar: bool) -> Self {
        self.shows_sidebar_initially = shows_sidebar;
        self
    }

    /// Set the initial pin state.
    #[must_use]
    pub fn pins_sidebar_initially(mut self, pins_sidebar: bool) -> Self {
        self.pins_sidebar_initially = pins_sidebar;
        self
    }

    /// Set the initial collapsed state.
    #[must_use]
    pub fn is_collapsed_initially(mut self, is_collapsed: bool) -> Self {
        self.is_collapsed_initially = is_collapsed;
        self
    }

    /// Set the split root accessible name.
    #[must_use]
    pub fn accessible_name(mut self, accessible_name: impl Into<String>) -> Self {
        self.accessible_name = Some(accessible_name.into());
        self
    }

    /// Add a CSS class to the split root.
    #[must_use]
    pub fn css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_classes.push(css_class.into());
        self
    }

    fn resolve(&self) -> BigWorkspaceSidebarSplitResolved {
        let min_sidebar_width = finite_or_default(self.min_sidebar_width, 220.0).max(1.0);
        let max_sidebar_width =
            finite_or_default(self.max_sidebar_width, 360.0).max(min_sidebar_width);
        BigWorkspaceSidebarSplitResolved {
            min_sidebar_width,
            max_sidebar_width,
            shows_sidebar_initially: self.shows_sidebar_initially,
            pins_sidebar_initially: self.pins_sidebar_initially,
            is_collapsed_initially: self.is_collapsed_initially,
            accessible_name: self
                .accessible_name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("Workspace sidebar split")
                .to_owned(),
            css_classes: self
                .css_classes
                .iter()
                .filter(|class| !class.trim().is_empty())
                .cloned()
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct BigWorkspaceSidebarSplitResolved {
    min_sidebar_width: f64,
    max_sidebar_width: f64,
    shows_sidebar_initially: bool,
    pins_sidebar_initially: bool,
    is_collapsed_initially: bool,
    accessible_name: String,
    css_classes: Vec<String>,
}

/// Reusable `AdwOverlaySplitView` shell for workspace sidebars.
#[derive(Clone)]
pub struct BigWorkspaceSidebarSplit {
    split_view: adw::OverlaySplitView,
}

impl BigWorkspaceSidebarSplit {
    /// Build a sidebar split around app-owned sidebar and content widgets.
    #[must_use]
    pub fn new(
        sidebar: &impl IsA<gtk::Widget>,
        content: &impl IsA<gtk::Widget>,
        spec: BigWorkspaceSidebarSplitSpec,
    ) -> Self {
        let resolved = spec.resolve();
        let split_view = adw::OverlaySplitView::builder()
            .sidebar(sidebar)
            .content(content)
            .min_sidebar_width(resolved.min_sidebar_width)
            .max_sidebar_width(resolved.max_sidebar_width)
            .show_sidebar(resolved.shows_sidebar_initially)
            .pin_sidebar(resolved.pins_sidebar_initially)
            .collapsed(resolved.is_collapsed_initially)
            .build();
        split_view.set_accessible_role(gtk::AccessibleRole::Group);
        split_view.update_property(&[gtk::accessible::Property::Label(&resolved.accessible_name)]);
        for css_class in resolved.css_classes {
            split_view.add_css_class(&css_class);
        }
        Self { split_view }
    }

    /// Borrow the underlying split view.
    #[must_use]
    pub fn split_view(&self) -> &adw::OverlaySplitView {
        &self.split_view
    }

    /// Consume the wrapper and return the split view.
    #[must_use]
    pub fn into_split_view(self) -> adw::OverlaySplitView {
        self.split_view
    }
}

fn finite_or_default(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_workspace_sidebar_policy() {
        let resolved = BigWorkspaceSidebarSplitSpec::new().resolve();

        assert_eq!(resolved.min_sidebar_width, 220.0);
        assert_eq!(resolved.max_sidebar_width, 360.0);
        assert!(!resolved.shows_sidebar_initially);
        assert!(!resolved.pins_sidebar_initially);
        assert!(resolved.is_collapsed_initially);
        assert_eq!(resolved.accessible_name, "Workspace sidebar split");
    }

    #[test]
    fn width_bounds_are_sanitized() {
        let resolved = BigWorkspaceSidebarSplitSpec::new()
            .min_sidebar_width(f64::NAN)
            .max_sidebar_width(100.0)
            .resolve();

        assert_eq!(resolved.min_sidebar_width, 220.0);
        assert_eq!(resolved.max_sidebar_width, 220.0);
    }

    #[test]
    fn css_classes_drop_empty_entries() {
        let resolved = BigWorkspaceSidebarSplitSpec::new()
            .css_class("")
            .css_class("workspace-sidebar")
            .css_class("   ")
            .resolve();

        assert_eq!(resolved.css_classes, vec!["workspace-sidebar"]);
    }
}
