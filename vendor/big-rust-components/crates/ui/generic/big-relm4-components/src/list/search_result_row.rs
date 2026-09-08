// SPDX-License-Identifier: MIT

//! Compact list row for searchable result lists.

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::feedback::tooltip;

const DEFAULT_SPACING: i32 = 4;
const DEFAULT_HORIZONTAL_MARGIN: i32 = 12;
const DEFAULT_VERTICAL_MARGIN: i32 = 8;
const DEFAULT_TITLE_SPACING: i32 = 8;
const DEFAULT_BADGE_CLASSES: &[&str] = &["success", "caption"];
const DEFAULT_TITLE_CLASSES: &[&str] = &["heading"];
const DEFAULT_SUBTITLE_CLASSES: &[&str] = &["dim-label", "caption"];

/// Data used to build a searchable result row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchResultRowSpec {
    /// Main title.
    pub title: String,
    /// Optional subtitle.
    pub subtitle: Option<String>,
    /// Optional badge text.
    pub badge: Option<String>,
    /// Optional badge tooltip.
    pub badge_tooltip: Option<String>,
    /// Badge CSS classes.
    pub badge_css_classes: Vec<String>,
    /// Title CSS classes.
    pub title_css_classes: Vec<String>,
    /// Row accessible label.
    pub accessible_label: Option<String>,
    /// Whether the row is selectable.
    pub selectable: bool,
    /// Whether the subtitle label is selectable text.
    pub subtitle_selectable: bool,
    /// Space between row lines.
    pub spacing: i32,
    /// Space between title and badge.
    pub title_spacing: i32,
    /// Horizontal row margin.
    pub horizontal_margin: i32,
    /// Vertical row margin.
    pub vertical_margin: i32,
}

impl BigSearchResultRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            badge: None,
            badge_tooltip: None,
            badge_css_classes: DEFAULT_BADGE_CLASSES
                .iter()
                .map(ToString::to_string)
                .collect(),
            title_css_classes: DEFAULT_TITLE_CLASSES
                .iter()
                .map(ToString::to_string)
                .collect(),
            accessible_label: None,
            selectable: true,
            subtitle_selectable: false,
            spacing: DEFAULT_SPACING,
            title_spacing: DEFAULT_TITLE_SPACING,
            horizontal_margin: DEFAULT_HORIZONTAL_MARGIN,
            vertical_margin: DEFAULT_VERTICAL_MARGIN,
        }
    }

    /// Configure subtitle text.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure badge text.
    #[must_use]
    pub fn badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// Configure badge tooltip text.
    #[must_use]
    pub fn badge_tooltip(mut self, badge_tooltip: impl Into<String>) -> Self {
        self.badge_tooltip = Some(badge_tooltip.into());
        self
    }

    /// Configure badge CSS classes.
    #[must_use]
    pub fn badge_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.badge_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Configure title CSS classes.
    #[must_use]
    pub fn title_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.title_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Configure the row accessible label.
    #[must_use]
    pub fn accessible_label(mut self, accessible_label: impl Into<String>) -> Self {
        self.accessible_label = Some(accessible_label.into());
        self
    }

    /// Configure whether the row is selectable.
    #[must_use]
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Configure whether subtitle text can be selected/copied.
    #[must_use]
    pub fn subtitle_selectable(mut self, subtitle_selectable: bool) -> Self {
        self.subtitle_selectable = subtitle_selectable;
        self
    }

    /// Configure row spacing.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Configure title-to-badge spacing.
    #[must_use]
    pub fn title_spacing(mut self, title_spacing: i32) -> Self {
        self.title_spacing = title_spacing;
        self
    }

    /// Configure horizontal row margin.
    #[must_use]
    pub fn horizontal_margin(mut self, horizontal_margin: i32) -> Self {
        self.horizontal_margin = horizontal_margin;
        self
    }

    /// Configure vertical row margin.
    #[must_use]
    pub fn vertical_margin(mut self, vertical_margin: i32) -> Self {
        self.vertical_margin = vertical_margin;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigSearchResultRowResolved {
        BigSearchResultRowResolved {
            title: self.title.clone(),
            subtitle: self.subtitle.clone(),
            badge: self.badge.clone(),
            badge_tooltip: self.badge_tooltip.clone(),
            badge_css_classes: self.badge_css_classes.clone(),
            title_css_classes: self.title_css_classes.clone(),
            accessible_label: self
                .accessible_label
                .clone()
                .unwrap_or_else(|| self.title.clone()),
            selectable: self.selectable,
            subtitle_selectable: self.subtitle_selectable,
            spacing: self.spacing.max(0),
            title_spacing: self.title_spacing.max(0),
            horizontal_margin: self.horizontal_margin.max(0),
            vertical_margin: self.vertical_margin.max(0),
        }
    }
}

/// Pure resolved search-result row contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchResultRowResolved {
    /// Main title.
    pub title: String,
    /// Optional subtitle.
    pub subtitle: Option<String>,
    /// Optional badge text.
    pub badge: Option<String>,
    /// Optional badge tooltip.
    pub badge_tooltip: Option<String>,
    /// Badge CSS classes.
    pub badge_css_classes: Vec<String>,
    /// Title CSS classes.
    pub title_css_classes: Vec<String>,
    /// Row accessible label.
    pub accessible_label: String,
    /// Whether the row is selectable.
    pub selectable: bool,
    /// Whether the subtitle label is selectable text.
    pub subtitle_selectable: bool,
    /// Space between row lines.
    pub spacing: i32,
    /// Space between title and badge.
    pub title_spacing: i32,
    /// Horizontal row margin.
    pub horizontal_margin: i32,
    /// Vertical row margin.
    pub vertical_margin: i32,
}

/// Built search-result row.
#[derive(Debug, Clone)]
pub struct BigSearchResultRow {
    root: gtk::ListBoxRow,
}

impl BigSearchResultRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigSearchResultRowSpec) -> Self {
        let resolved = spec.resolved();
        let root = gtk::ListBoxRow::builder()
            .selectable(resolved.selectable)
            .build();
        root.update_property(&[gtk::accessible::Property::Label(
            resolved.accessible_label.as_str(),
        )]);

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(resolved.spacing)
            .margin_top(resolved.vertical_margin)
            .margin_bottom(resolved.vertical_margin)
            .margin_start(resolved.horizontal_margin)
            .margin_end(resolved.horizontal_margin)
            .build();
        body.append(&build_title_line(&resolved));

        if let Some(subtitle) = resolved.subtitle.as_deref() {
            body.append(&build_subtitle_label(
                subtitle,
                resolved.subtitle_selectable,
            ));
        }

        root.set_child(Some(&body));
        Self { root }
    }

    /// Return the root row.
    #[must_use]
    pub fn root(&self) -> &gtk::ListBoxRow {
        &self.root
    }

    /// Consume `self` and yield the root row.
    #[must_use]
    pub fn into_root(self) -> gtk::ListBoxRow {
        self.root
    }
}

fn build_title_line(resolved: &BigSearchResultRowResolved) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, resolved.title_spacing);
    let label = gtk::Label::builder()
        .label(resolved.title.as_str())
        .xalign(0.0)
        .wrap(true)
        .hexpand(true)
        .css_classes(resolved.title_css_classes.clone())
        .build();
    row.append(&label);
    if let Some(badge_text) = resolved.badge.as_deref() {
        let badge = gtk::Label::builder()
            .label(badge_text)
            .valign(gtk::Align::Center)
            .css_classes(resolved.badge_css_classes.clone())
            .build();
        if let Some(tooltip_text) = resolved.badge_tooltip.as_deref() {
            tooltip::set(&badge, tooltip_text);
        }
        row.append(&badge);
    }
    row
}

fn build_subtitle_label(subtitle: &str, selectable: bool) -> gtk::Label {
    gtk::Label::builder()
        .label(subtitle)
        .xalign(0.0)
        .selectable(selectable)
        .css_classes(DEFAULT_SUBTITLE_CLASSES)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_result_row_resolves_defaults() {
        let resolved = BigSearchResultRowSpec::new("Llama")
            .subtitle("provider/llama")
            .badge("Free")
            .subtitle_selectable(true)
            .resolved();

        assert_eq!(resolved.title, "Llama");
        assert_eq!(resolved.subtitle.as_deref(), Some("provider/llama"));
        assert_eq!(resolved.badge.as_deref(), Some("Free"));
        assert_eq!(resolved.badge_css_classes, ["success", "caption"]);
        assert_eq!(resolved.title_css_classes, ["heading"]);
        assert_eq!(resolved.accessible_label, "Llama");
        assert!(resolved.selectable);
        assert!(resolved.subtitle_selectable);
    }

    #[test]
    fn search_result_row_clamps_layout_values() {
        let resolved = BigSearchResultRowSpec::new("More")
            .selectable(false)
            .spacing(-1)
            .title_spacing(-2)
            .horizontal_margin(-3)
            .vertical_margin(-4)
            .resolved();

        assert!(!resolved.selectable);
        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.title_spacing, 0);
        assert_eq!(resolved.horizontal_margin, 0);
        assert_eq!(resolved.vertical_margin, 0);
    }

    #[test]
    fn search_result_row_can_override_title_classes() {
        let resolved = BigSearchResultRowSpec::new("Showing first 100")
            .title_css_classes(["dim-label"])
            .resolved();

        assert_eq!(resolved.title_css_classes, ["dim-label"]);
    }
}
