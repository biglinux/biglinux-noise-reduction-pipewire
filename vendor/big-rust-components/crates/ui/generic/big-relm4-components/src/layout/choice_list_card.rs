// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Horizontal choice cards with optional leading media and trailing affordance.

use relm4::gtk;
use relm4::gtk::prelude::*;

/// Data used to build a horizontal choice card.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigChoiceListCardSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: String,
    /// Spacing between leading media, text and trailing icon.
    pub spacing: i32,
    /// Spacing between title and subtitle.
    pub text_spacing: i32,
    /// Vertical inner margin.
    pub vertical_margin: i32,
    /// Horizontal inner margin.
    pub horizontal_margin: i32,
    /// Optional trailing icon name.
    pub trailing_icon_name: Option<String>,
    /// CSS classes applied to the trailing icon.
    pub trailing_icon_css_classes: Vec<String>,
    /// CSS classes applied to the root button.
    pub button_css_classes: Vec<String>,
    /// CSS classes applied to the content row.
    pub content_css_classes: Vec<String>,
    /// CSS classes applied to the title label.
    pub title_css_classes: Vec<String>,
    /// CSS classes applied to the subtitle label.
    pub subtitle_css_classes: Vec<String>,
}

impl BigChoiceListCardSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            spacing: 18,
            text_spacing: 6,
            vertical_margin: 10,
            horizontal_margin: 16,
            trailing_icon_name: Some("go-next-symbolic".to_owned()),
            trailing_icon_css_classes: Vec::new(),
            button_css_classes: vec!["flat".to_owned(), "activatable".to_owned()],
            content_css_classes: Vec::new(),
            title_css_classes: vec!["heading".to_owned()],
            subtitle_css_classes: vec!["dim-label".to_owned(), "caption".to_owned()],
        }
    }

    /// Configure spacing between main row elements.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Configure spacing between title and subtitle.
    #[must_use]
    pub fn text_spacing(mut self, text_spacing: i32) -> Self {
        self.text_spacing = text_spacing;
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

    /// Configure trailing icon name.
    #[must_use]
    pub fn trailing_icon_name(mut self, trailing_icon_name: impl Into<String>) -> Self {
        self.trailing_icon_name = Some(trailing_icon_name.into());
        self
    }

    /// Remove the trailing icon.
    #[must_use]
    pub fn without_trailing_icon(mut self) -> Self {
        self.trailing_icon_name = None;
        self
    }

    /// Configure trailing icon CSS classes.
    #[must_use]
    pub fn trailing_icon_css_classes(
        mut self,
        trailing_icon_css_classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.trailing_icon_css_classes = trailing_icon_css_classes
            .into_iter()
            .map(Into::into)
            .collect();
        self
    }

    /// Configure root button CSS classes.
    #[must_use]
    pub fn button_css_classes(
        mut self,
        button_css_classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.button_css_classes = button_css_classes.into_iter().map(Into::into).collect();
        self
    }

    /// Configure content row CSS classes.
    #[must_use]
    pub fn content_css_classes(
        mut self,
        content_css_classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.content_css_classes = content_css_classes.into_iter().map(Into::into).collect();
        self
    }

    /// Configure title CSS classes.
    #[must_use]
    pub fn title_css_classes(
        mut self,
        title_css_classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.title_css_classes = title_css_classes.into_iter().map(Into::into).collect();
        self
    }

    /// Configure subtitle CSS classes.
    #[must_use]
    pub fn subtitle_css_classes(
        mut self,
        subtitle_css_classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.subtitle_css_classes = subtitle_css_classes.into_iter().map(Into::into).collect();
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigChoiceListCardResolved {
        BigChoiceListCardResolved {
            title: self.title.clone(),
            subtitle: self.subtitle.clone(),
            accessible_label: format!("{}: {}", self.title, self.subtitle),
            spacing: self.spacing.max(0),
            text_spacing: self.text_spacing.max(0),
            vertical_margin: self.vertical_margin.max(0),
            horizontal_margin: self.horizontal_margin.max(0),
            trailing_icon_name: self.trailing_icon_name.clone(),
            trailing_icon_css_classes: self.trailing_icon_css_classes.clone(),
            button_css_classes: self.button_css_classes.clone(),
            content_css_classes: self.content_css_classes.clone(),
            title_css_classes: self.title_css_classes.clone(),
            subtitle_css_classes: self.subtitle_css_classes.clone(),
        }
    }
}

/// Pure resolved choice-list-card contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigChoiceListCardResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: String,
    /// Accessible label.
    pub accessible_label: String,
    /// Spacing between main row elements after clamping.
    pub spacing: i32,
    /// Spacing between title and subtitle after clamping.
    pub text_spacing: i32,
    /// Vertical inner margin after clamping.
    pub vertical_margin: i32,
    /// Horizontal inner margin after clamping.
    pub horizontal_margin: i32,
    /// Optional trailing icon name.
    pub trailing_icon_name: Option<String>,
    /// CSS classes applied to the trailing icon.
    pub trailing_icon_css_classes: Vec<String>,
    /// CSS classes applied to the root button.
    pub button_css_classes: Vec<String>,
    /// CSS classes applied to the content row.
    pub content_css_classes: Vec<String>,
    /// CSS classes applied to the title label.
    pub title_css_classes: Vec<String>,
    /// CSS classes applied to the subtitle label.
    pub subtitle_css_classes: Vec<String>,
}

/// Built horizontal choice card.
#[derive(Debug, Clone)]
pub struct BigChoiceListCard {
    root: gtk::Button,
}

impl BigChoiceListCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigChoiceListCardSpec, leading: Option<&gtk::Widget>) -> Self {
        let resolved = spec.resolved();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(resolved.spacing)
            .margin_top(resolved.vertical_margin)
            .margin_bottom(resolved.vertical_margin)
            .margin_start(resolved.horizontal_margin)
            .margin_end(resolved.horizontal_margin)
            .build();
        add_css_classes(&row, &resolved.content_css_classes);

        if let Some(leading) = leading {
            row.append(leading);
        }

        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(resolved.text_spacing)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        let title = gtk::Label::builder()
            .label(&resolved.title)
            .xalign(0.0)
            .build();
        add_css_classes(&title, &resolved.title_css_classes);
        let subtitle = gtk::Label::builder()
            .label(&resolved.subtitle)
            .xalign(0.0)
            .wrap(true)
            .build();
        add_css_classes(&subtitle, &resolved.subtitle_css_classes);
        text.append(&title);
        text.append(&subtitle);
        row.append(&text);

        if let Some(icon_name) = resolved.trailing_icon_name.as_deref() {
            let arrow = gtk::Image::from_icon_name(icon_name);
            arrow.set_valign(gtk::Align::Center);
            add_css_classes(&arrow, &resolved.trailing_icon_css_classes);
            row.append(&arrow);
        }

        let button = gtk::Button::builder().child(&row).build();
        add_css_classes(&button, &resolved.button_css_classes);
        button.update_property(&[gtk::accessible::Property::Label(&resolved.accessible_label)]);
        Self { root: button }
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

fn add_css_classes(widget: &impl IsA<gtk::Widget>, css_classes: &[String]) {
    for class in css_classes {
        widget.add_css_class(class);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_list_card_defaults_to_horizontal_action_card() {
        let resolved = BigChoiceListCardSpec::new("SSH", "Connect remotely").resolved();

        assert_eq!(resolved.accessible_label, "SSH: Connect remotely");
        assert_eq!(resolved.spacing, 18);
        assert_eq!(resolved.text_spacing, 6);
        assert_eq!(resolved.vertical_margin, 10);
        assert_eq!(resolved.horizontal_margin, 16);
        assert_eq!(
            resolved.trailing_icon_name.as_deref(),
            Some("go-next-symbolic")
        );
    }

    #[test]
    fn resolved_clamps_spacing_and_margins() {
        let resolved = BigChoiceListCardSpec::new("Local", "Run here")
            .spacing(-4)
            .text_spacing(-3)
            .vertical_margin(-2)
            .horizontal_margin(-1)
            .without_trailing_icon()
            .resolved();

        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.text_spacing, 0);
        assert_eq!(resolved.vertical_margin, 0);
        assert_eq!(resolved.horizontal_margin, 0);
        assert_eq!(resolved.trailing_icon_name, None);
    }
}
