// SPDX-License-Identifier: MIT

//! Search entry with an optional inline filter toggle.

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::feedback::tooltip;

const DEFAULT_SPACING: i32 = 8;

/// Optional toggle shown next to a search entry.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchFilterToggleSpec {
    /// Visible label.
    pub label: String,
    /// Tooltip text.
    pub tooltip: String,
    /// Accessible label.
    pub accessible_label: String,
    /// Whether the toggle is visible.
    pub visible: bool,
}

impl BigSearchFilterToggleSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(label: impl Into<String>, tooltip: impl Into<String>) -> Self {
        let label = label.into();
        let tooltip = tooltip.into();
        Self {
            accessible_label: tooltip.clone(),
            label,
            tooltip,
            visible: true,
        }
    }

    /// Configure the accessible label independently from the tooltip.
    #[must_use]
    pub fn accessible_label(mut self, accessible_label: impl Into<String>) -> Self {
        self.accessible_label = accessible_label.into();
        self
    }

    /// Configure whether the toggle starts visible.
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

/// Data used to build a search/filter row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchFilterBarSpec {
    /// Search placeholder text.
    pub placeholder: String,
    /// Search accessible label.
    pub search_accessible_label: String,
    /// Optional filter toggle.
    pub toggle: Option<BigSearchFilterToggleSpec>,
    /// Space between controls.
    pub spacing: i32,
}

impl BigSearchFilterBarSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(placeholder: impl Into<String>, search_accessible_label: impl Into<String>) -> Self {
        Self {
            placeholder: placeholder.into(),
            search_accessible_label: search_accessible_label.into(),
            toggle: None,
            spacing: DEFAULT_SPACING,
        }
    }

    /// Add an optional toggle next to the search entry.
    #[must_use]
    pub fn toggle(mut self, toggle: BigSearchFilterToggleSpec) -> Self {
        self.toggle = Some(toggle);
        self
    }

    /// Configure spacing between controls.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigSearchFilterBarResolved {
        BigSearchFilterBarResolved {
            placeholder: self.placeholder.clone(),
            search_accessible_label: self.search_accessible_label.clone(),
            toggle: self.toggle.clone(),
            spacing: self.spacing.max(0),
        }
    }
}

/// Pure resolved search/filter contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchFilterBarResolved {
    /// Search placeholder text.
    pub placeholder: String,
    /// Search accessible label.
    pub search_accessible_label: String,
    /// Optional filter toggle.
    pub toggle: Option<BigSearchFilterToggleSpec>,
    /// Space between controls.
    pub spacing: i32,
}

/// Built search/filter row.
#[derive(Debug, Clone)]
pub struct BigSearchFilterBar {
    root: gtk::Box,
    search: gtk::SearchEntry,
    toggle: Option<gtk::ToggleButton>,
}

impl BigSearchFilterBar {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigSearchFilterBarSpec) -> Self {
        let resolved = spec.resolved();
        let root = gtk::Box::new(gtk::Orientation::Horizontal, resolved.spacing);
        let search = gtk::SearchEntry::builder()
            .placeholder_text(resolved.placeholder.as_str())
            .hexpand(true)
            .build();
        search.update_property(&[gtk::accessible::Property::Label(
            resolved.search_accessible_label.as_str(),
        )]);
        root.append(&search);

        let toggle = resolved.toggle.map(|toggle_spec| {
            let toggle = gtk::ToggleButton::builder()
                .label(toggle_spec.label.as_str())
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .visible(toggle_spec.visible)
                .build();
            tooltip::set(&toggle, toggle_spec.tooltip.as_str());
            toggle.update_property(&[gtk::accessible::Property::Label(
                toggle_spec.accessible_label.as_str(),
            )]);
            root.append(&toggle);
            toggle
        });

        Self {
            root,
            search,
            toggle,
        }
    }

    /// Return the root row.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return the search entry.
    #[must_use]
    pub fn search(&self) -> &gtk::SearchEntry {
        &self.search
    }

    /// Return the optional filter toggle.
    #[must_use]
    pub fn toggle(&self) -> Option<&gtk::ToggleButton> {
        self.toggle.as_ref()
    }

    /// Consume `self` and yield the root row.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_filter_bar_clamps_spacing_and_keeps_labels() {
        let resolved = BigSearchFilterBarSpec::new("Search models", "Search")
            .toggle(
                BigSearchFilterToggleSpec::new("Free", "Only free models")
                    .accessible_label("Show only free models")
                    .visible(false),
            )
            .spacing(-4)
            .resolved();

        assert_eq!(resolved.placeholder, "Search models");
        assert_eq!(resolved.search_accessible_label, "Search");
        assert_eq!(resolved.spacing, 0);
        let toggle = resolved.toggle.as_ref().expect("toggle present");
        assert_eq!(toggle.label, "Free");
        assert_eq!(toggle.tooltip, "Only free models");
        assert_eq!(toggle.accessible_label, "Show only free models");
        assert!(!toggle.visible);
    }
}
