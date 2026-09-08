// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Card-like choice buttons for small selection pages.

use relm4::gtk;
use relm4::gtk::prelude::*;

/// Data used to build a card-like choice button.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigChoiceCardSpec {
    /// Icon name.
    pub icon_name: String,
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: String,
    /// Icon pixel size.
    pub icon_pixel_size: i32,
    /// Vertical inner margin.
    pub vertical_margin: i32,
    /// Horizontal inner margin.
    pub horizontal_margin: i32,
    /// Inner spacing.
    pub spacing: i32,
}

impl BigChoiceCardSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        icon_name: impl Into<String>,
        title: impl Into<String>,
        subtitle: impl Into<String>,
    ) -> Self {
        Self {
            icon_name: icon_name.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            icon_pixel_size: 48,
            vertical_margin: 16,
            horizontal_margin: 12,
            spacing: 8,
        }
    }

    /// Configure icon pixel size.
    #[must_use]
    pub fn icon_pixel_size(mut self, icon_pixel_size: i32) -> Self {
        self.icon_pixel_size = icon_pixel_size;
        self
    }

    /// Configure vertical inner margin.
    #[must_use]
    pub fn vertical_margin(mut self, vertical_margin: i32) -> Self {
        self.vertical_margin = vertical_margin;
        self
    }

    /// Configure horizontal inner margin.
    #[must_use]
    pub fn horizontal_margin(mut self, horizontal_margin: i32) -> Self {
        self.horizontal_margin = horizontal_margin;
        self
    }

    /// Configure inner spacing.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigChoiceCardResolved {
        BigChoiceCardResolved {
            icon_name: self.icon_name.clone(),
            title: self.title.clone(),
            subtitle: self.subtitle.clone(),
            icon_pixel_size: self.icon_pixel_size.max(1),
            vertical_margin: self.vertical_margin.max(0),
            horizontal_margin: self.horizontal_margin.max(0),
            spacing: self.spacing.max(0),
        }
    }
}

/// Pure resolved choice-card contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigChoiceCardResolved {
    /// Icon name.
    pub icon_name: String,
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: String,
    /// Icon pixel size after clamping.
    pub icon_pixel_size: i32,
    /// Vertical inner margin after clamping.
    pub vertical_margin: i32,
    /// Horizontal inner margin after clamping.
    pub horizontal_margin: i32,
    /// Inner spacing after clamping.
    pub spacing: i32,
}

/// Built card-like choice button.
#[derive(Debug, Clone)]
pub struct BigChoiceCard {
    root: gtk::Button,
}

impl BigChoiceCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigChoiceCardSpec) -> Self {
        let resolved = spec.resolved();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(resolved.spacing)
            .margin_top(resolved.vertical_margin)
            .margin_bottom(resolved.vertical_margin)
            .margin_start(resolved.horizontal_margin)
            .margin_end(resolved.horizontal_margin)
            .build();

        let image = gtk::Image::from_icon_name(&resolved.icon_name);
        image.set_pixel_size(resolved.icon_pixel_size);
        content.append(&image);
        content.append(
            &gtk::Label::builder()
                .label(&resolved.title)
                .css_classes(["title-3"])
                .build(),
        );
        content.append(
            &gtk::Label::builder()
                .label(&resolved.subtitle)
                .wrap(true)
                .justify(gtk::Justification::Center)
                .css_classes(["dim-label"])
                .build(),
        );

        let root = gtk::Button::builder()
            .css_classes(["card"])
            .child(&content)
            .hexpand(true)
            .build();
        root.update_property(&[gtk::accessible::Property::Label(&resolved.title)]);
        Self { root }
    }

    /// Return a reference to the root button.
    #[must_use]
    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    /// Consume `self` and yield the underlying button.
    #[must_use]
    pub fn into_root(self) -> gtk::Button {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_card_defaults_match_biglinux_card_style() {
        let spec = BigChoiceCardSpec::new("utilities-terminal-symbolic", "Simple", "Run it");

        assert_eq!(spec.icon_pixel_size, 48);
        assert_eq!(spec.vertical_margin, 16);
        assert_eq!(spec.horizontal_margin, 12);
        assert_eq!(spec.spacing, 8);
    }

    #[test]
    fn resolved_clamps_numeric_values() {
        let resolved = BigChoiceCardSpec::new("icon", "Title", "Subtitle")
            .icon_pixel_size(0)
            .vertical_margin(-1)
            .horizontal_margin(-2)
            .spacing(-8)
            .resolved();

        assert_eq!(resolved.icon_pixel_size, 1);
        assert_eq!(resolved.vertical_margin, 0);
        assert_eq!(resolved.horizontal_margin, 0);
        assert_eq!(resolved.spacing, 0);
    }
}
