// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared header bar for sidebar and utility-pane surfaces.

use adw::prelude::*;
use relm4::gtk;

/// Display-free spec for [`BigSidebarHeader`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSidebarHeaderSpec {
    /// Header title.
    pub title: String,
    /// Whether libadwaita shows title controls.
    pub show_title: bool,
    /// CSS classes applied to the header bar.
    pub css_classes: Vec<String>,
}

impl BigSidebarHeaderSpec {
    /// Create a sidebar header spec with the standard title label.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            show_title: true,
            css_classes: Vec::new(),
        }
    }

    /// Set whether libadwaita should show title controls.
    #[must_use]
    pub fn show_title(mut self, show_title: bool) -> Self {
        self.show_title = show_title;
        self
    }

    /// Add one CSS class to the header.
    #[must_use]
    pub fn css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_classes.push(css_class.into());
        self
    }
}

/// Header bar with BigLinux sidebar title styling and action slots.
#[derive(Debug, Clone)]
pub struct BigSidebarHeader {
    root: adw::HeaderBar,
}

impl BigSidebarHeader {
    /// Build a sidebar header from `spec`.
    #[must_use]
    pub fn new(spec: BigSidebarHeaderSpec) -> Self {
        let title = gtk::Label::builder()
            .label(&spec.title)
            .css_classes(["heading"])
            .build();
        let root = adw::HeaderBar::builder()
            .show_title(spec.show_title)
            .title_widget(&title)
            .build();
        for css_class in spec.css_classes {
            root.add_css_class(&css_class);
        }
        Self { root }
    }

    /// Borrow the root header bar.
    #[must_use]
    pub fn root(&self) -> &adw::HeaderBar {
        &self.root
    }

    /// Add a widget to the start slot.
    pub fn pack_start(&self, widget: &impl IsA<gtk::Widget>) {
        self.root.pack_start(widget);
    }

    /// Add a widget to the end slot.
    pub fn pack_end(&self, widget: &impl IsA<gtk::Widget>) {
        self.root.pack_end(widget);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_to_visible_title_without_extra_classes() {
        let spec = BigSidebarHeaderSpec::new("Files");

        assert_eq!(spec.title, "Files");
        assert!(spec.show_title);
        assert!(spec.css_classes.is_empty());
    }

    #[test]
    fn spec_records_title_visibility_and_css_class() {
        let spec = BigSidebarHeaderSpec::new("Playlist")
            .show_title(false)
            .css_class("flat");

        assert_eq!(spec.title, "Playlist");
        assert!(!spec.show_title);
        assert_eq!(spec.css_classes, ["flat"]);
    }
}
