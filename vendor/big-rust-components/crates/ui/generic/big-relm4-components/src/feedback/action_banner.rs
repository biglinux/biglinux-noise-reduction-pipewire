// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Inline action banner for recoverable pane or workflow states.

use std::cell::RefCell;

use adw::prelude::*;
use relm4::gtk;

use crate::layout::widget_container::clear_box_children;

/// Display-free spec for [`BigActionBanner`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigActionBannerSpec {
    /// Initial banner title.
    pub title: String,
    /// CSS classes applied to the root row.
    pub css_classes: Vec<String>,
    /// Spacing between title and actions.
    pub spacing: i32,
    /// Spacing between action buttons.
    pub action_spacing: i32,
    /// Initial root visibility.
    pub is_visible: bool,
}

impl BigActionBannerSpec {
    /// Create an initially hidden action banner.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            css_classes: Vec::new(),
            spacing: 12,
            action_spacing: 6,
            is_visible: false,
        }
    }

    /// Replace root CSS classes.
    #[must_use]
    pub fn css_classes(mut self, classes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Replace root/action spacing.
    #[must_use]
    pub fn spacing(mut self, spacing: i32, action_spacing: i32) -> Self {
        self.spacing = spacing.max(0);
        self.action_spacing = action_spacing.max(0);
        self
    }

    /// Set initial visibility.
    #[must_use]
    pub fn visible(mut self, is_visible: bool) -> Self {
        self.is_visible = is_visible;
        self
    }
}

/// Inline banner with a single title and caller-wired action buttons.
#[derive(Debug)]
pub struct BigActionBanner {
    root: gtk::Box,
    title_label: RefCell<gtk::Label>,
    actions: gtk::Box,
}

impl BigActionBanner {
    /// Build a banner from a semantic spec.
    #[must_use]
    pub fn new(spec: BigActionBannerSpec) -> Self {
        let title_label = build_banner_title_label(&spec.title);
        let actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(spec.action_spacing)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Center)
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(spec.spacing)
            .visible(spec.is_visible)
            .build();
        for css_class in &spec.css_classes {
            root.add_css_class(css_class);
        }
        root.append(&title_label);
        root.append(&actions);

        Self {
            root,
            title_label: RefCell::new(title_label),
            actions,
        }
    }

    /// Return the root row.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return the action button box.
    #[must_use]
    pub fn actions(&self) -> &gtk::Box {
        &self.actions
    }

    /// Replace the title label with a fresh widget.
    pub fn set_title(&self, title: &str) {
        let fresh = build_banner_title_label(title);
        let old = self.title_label.replace(fresh.clone());
        self.root.remove(&old);
        self.root.insert_child_after(&fresh, None::<&gtk::Widget>);
        self.root.queue_draw();
        if let Some(parent) = self.root.parent() {
            parent.queue_draw();
        }
    }

    /// Remove all action buttons.
    pub fn clear_actions(&self) {
        clear_box_children(&self.actions);
    }

    /// Append a caller-wired action button.
    pub fn append_action_button(&self, label: &str) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        self.actions.append(&button);
        button
    }
}

fn build_banner_title_label(title: &str) -> gtk::Label {
    gtk::Label::builder()
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .single_line_mode(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .label(title)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_banner_spec_defaults_to_hidden_row() {
        let spec = BigActionBannerSpec::new("Disconnected");

        assert_eq!(spec.title, "Disconnected");
        assert!(spec.css_classes.is_empty());
        assert_eq!(spec.spacing, 12);
        assert_eq!(spec.action_spacing, 6);
        assert!(!spec.is_visible);
    }

    #[test]
    fn action_banner_spec_records_classes_spacing_and_visibility() {
        let spec = BigActionBannerSpec::new("Retrying")
            .css_classes(["error", "inline-banner"])
            .spacing(-4, 10)
            .visible(true);

        assert_eq!(spec.css_classes, ["error", "inline-banner"]);
        assert_eq!(spec.spacing, 0);
        assert_eq!(spec.action_spacing, 10);
        assert!(spec.is_visible);
    }
}
