// SPDX-License-Identifier: MIT

//! Flat action button for compact popover lists.

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::feedback::tooltip;

const DEFAULT_SPACING: i32 = 8;
const DEFAULT_ICON_PIXEL_SIZE: i32 = 16;
const DEFAULT_CLASSES: &[&str] = &["flat"];

/// Data used to build a flat popover/list action button.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPopoverActionButtonSpec {
    /// User-visible label.
    pub label: String,
    /// Optional symbolic icon name.
    pub icon_name: Option<String>,
    /// Optional tooltip text.
    pub tooltip: Option<String>,
    /// CSS classes applied to the button.
    pub css_classes: Vec<String>,
    /// Space between icon and label.
    pub spacing: i32,
    /// Icon pixel size.
    pub icon_pixel_size: i32,
}

impl BigPopoverActionButtonSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon_name: None,
            tooltip: None,
            css_classes: DEFAULT_CLASSES.iter().map(ToString::to_string).collect(),
            spacing: DEFAULT_SPACING,
            icon_pixel_size: DEFAULT_ICON_PIXEL_SIZE,
        }
    }

    /// Configure the optional symbolic icon name.
    #[must_use]
    pub fn icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    /// Configure tooltip text.
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Configure CSS classes.
    #[must_use]
    pub fn css_classes(mut self, classes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Configure icon-to-label spacing.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Configure icon pixel size.
    #[must_use]
    pub fn icon_pixel_size(mut self, icon_pixel_size: i32) -> Self {
        self.icon_pixel_size = icon_pixel_size;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigPopoverActionButtonResolved {
        BigPopoverActionButtonResolved {
            label: self.label.clone(),
            icon_name: self.icon_name.clone(),
            tooltip: self.tooltip.clone(),
            css_classes: self.css_classes.clone(),
            spacing: self.spacing.max(0),
            icon_pixel_size: self.icon_pixel_size.max(1),
        }
    }
}

/// Pure resolved popover action button contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPopoverActionButtonResolved {
    /// User-visible label.
    pub label: String,
    /// Optional symbolic icon name.
    pub icon_name: Option<String>,
    /// Optional tooltip text.
    pub tooltip: Option<String>,
    /// CSS classes applied to the button.
    pub css_classes: Vec<String>,
    /// Space between icon and label.
    pub spacing: i32,
    /// Icon pixel size.
    pub icon_pixel_size: i32,
}

/// Built flat popover/list action button.
#[derive(Debug, Clone)]
pub struct BigPopoverActionButton {
    root: gtk::Button,
}

impl BigPopoverActionButton {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigPopoverActionButtonSpec) -> Self {
        let resolved = spec.resolved();
        let root = gtk::Button::builder()
            .halign(gtk::Align::Fill)
            .hexpand(true)
            .css_classes(resolved.css_classes.clone())
            .build();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, resolved.spacing);
        row.set_halign(gtk::Align::Fill);
        row.set_hexpand(true);

        if let Some(icon_name) = resolved.icon_name.as_deref() {
            let image = gtk::Image::from_icon_name(icon_name);
            image.set_pixel_size(resolved.icon_pixel_size);
            row.append(&image);
        }

        let label = gtk::Label::builder()
            .label(resolved.label.as_str())
            .xalign(0.0)
            .hexpand(true)
            .build();
        row.append(&label);
        root.set_child(Some(&row));

        if let Some(tooltip_text) = resolved.tooltip.as_deref() {
            tooltip::set(&root, tooltip_text);
        }
        root.update_property(&[gtk::accessible::Property::Label(resolved.label.as_str())]);

        Self { root }
    }

    /// Return the root button.
    #[must_use]
    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    /// Consume `self` and yield the root button.
    #[must_use]
    pub fn into_root(self) -> gtk::Button {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popover_action_button_defaults_to_flat_text_row() {
        let resolved = BigPopoverActionButtonSpec::new("Open").resolved();

        assert_eq!(resolved.label, "Open");
        assert_eq!(resolved.icon_name, None);
        assert_eq!(resolved.tooltip, None);
        assert_eq!(resolved.css_classes, ["flat"]);
        assert_eq!(resolved.spacing, 8);
        assert_eq!(resolved.icon_pixel_size, 16);
    }

    #[test]
    fn popover_action_button_records_icon_tooltip_and_clamps_layout_values() {
        let resolved = BigPopoverActionButtonSpec::new("Reload")
            .icon_name("view-refresh-symbolic")
            .tooltip("Reload lesson")
            .css_classes(["flat", "lesson-action-row"])
            .spacing(-2)
            .icon_pixel_size(0)
            .resolved();

        assert_eq!(resolved.label, "Reload");
        assert_eq!(resolved.icon_name.as_deref(), Some("view-refresh-symbolic"));
        assert_eq!(resolved.tooltip.as_deref(), Some("Reload lesson"));
        assert_eq!(resolved.css_classes, ["flat", "lesson-action-row"]);
        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.icon_pixel_size, 1);
    }
}
