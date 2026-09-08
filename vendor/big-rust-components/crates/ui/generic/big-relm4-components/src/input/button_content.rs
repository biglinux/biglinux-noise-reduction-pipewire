// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Icon+label button content with BigLinux accessibility defaults.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

/// Data used to build an icon+label Adwaita button content widget.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigButtonContentSpec {
    /// Symbolic icon name shown before the label.
    pub icon_name: String,
    /// Visible label.
    pub label: String,
    /// Accessible label for the parent button. Defaults to [`Self::label`].
    pub accessible_label: Option<String>,
    /// Accessible description for the parent button.
    pub accessible_description: Option<String>,
    /// BigLinux tooltip text for the parent button.
    pub tooltip: Option<String>,
    /// CSS classes applied to the parent button.
    pub css_classes: Vec<String>,
    /// Whether the Adwaita content may shrink.
    pub can_shrink: bool,
    /// Whether mnemonic underlines are used.
    pub use_underline: bool,
}

impl BigButtonContentSpec {
    /// Create a new button content spec from a symbolic icon and visible label.
    #[must_use]
    pub fn new(icon_name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            icon_name: icon_name.into(),
            label: label.into(),
            accessible_label: None,
            accessible_description: None,
            tooltip: None,
            css_classes: Vec::new(),
            can_shrink: false,
            use_underline: false,
        }
    }

    /// Configure the accessible label.
    #[must_use]
    pub fn accessible_label(mut self, label: impl Into<String>) -> Self {
        self.accessible_label = Some(label.into());
        self
    }

    /// Configure the accessible description.
    #[must_use]
    pub fn accessible_description(mut self, description: impl Into<String>) -> Self {
        self.accessible_description = Some(description.into());
        self
    }

    /// Configure the BigLinux tooltip text.
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Add one CSS class for the parent button.
    #[must_use]
    pub fn css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_classes.push(css_class.into());
        self
    }

    /// Add several CSS classes for the parent button.
    #[must_use]
    pub fn css_classes<I, S>(mut self, css_classes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.css_classes
            .extend(css_classes.into_iter().map(Into::into));
        self
    }

    /// Let the Adwaita button content shrink when space is tight.
    #[must_use]
    pub fn can_shrink(mut self) -> Self {
        self.can_shrink = true;
        self
    }

    /// Enable mnemonic underlines in the visible label.
    #[must_use]
    pub fn use_underline(mut self) -> Self {
        self.use_underline = true;
        self
    }

    /// Resolve defaults into a display-free contract.
    #[must_use]
    pub fn resolved(&self) -> BigButtonContentResolved {
        BigButtonContentResolved {
            icon_name: self.icon_name.clone(),
            label: self.label.clone(),
            accessible_label: self
                .accessible_label
                .clone()
                .unwrap_or_else(|| self.label.clone()),
            accessible_description: self.accessible_description.clone(),
            tooltip: self.tooltip.clone(),
            css_classes: self.css_classes.clone(),
            can_shrink: self.can_shrink,
            use_underline: self.use_underline,
        }
    }
}

/// Pure resolved button content contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigButtonContentResolved {
    /// Symbolic icon name shown before the label.
    pub icon_name: String,
    /// Visible label.
    pub label: String,
    /// Accessible label for the parent button.
    pub accessible_label: String,
    /// Accessible description for the parent button.
    pub accessible_description: Option<String>,
    /// BigLinux tooltip text for the parent button.
    pub tooltip: Option<String>,
    /// CSS classes applied to the parent button.
    pub css_classes: Vec<String>,
    /// Whether the Adwaita content may shrink.
    pub can_shrink: bool,
    /// Whether mnemonic underlines are used.
    pub use_underline: bool,
}

/// Build the underlying Adwaita content widget.
#[must_use]
pub fn build_button_content(spec: &BigButtonContentSpec) -> adw::ButtonContent {
    let resolved = spec.resolved();
    adw::ButtonContent::builder()
        .icon_name(resolved.icon_name.as_str())
        .label(resolved.label.as_str())
        .can_shrink(resolved.can_shrink)
        .use_underline(resolved.use_underline)
        .build()
}

/// Build a GTK button using [`BigButtonContentSpec`] for content, a11y, and tooltip.
#[must_use]
pub fn build_button(spec: &BigButtonContentSpec) -> gtk::Button {
    let resolved = spec.resolved();
    let content = build_button_content(spec);
    let button = gtk::Button::builder().child(&content).build();

    for css_class in &resolved.css_classes {
        button.add_css_class(css_class);
    }

    if let Some(tooltip_text) = resolved.tooltip.as_deref() {
        tooltip::set(&button, tooltip_text);
    }

    content.update_property(&[gtk::accessible::Property::Label(
        resolved.accessible_label.as_str(),
    )]);

    let accessible_description = resolved
        .accessible_description
        .as_deref()
        .or(resolved.tooltip.as_deref())
        .filter(|description| *description != resolved.accessible_label);
    let mut properties = vec![gtk::accessible::Property::Label(
        resolved.accessible_label.as_str(),
    )];
    if let Some(description) = accessible_description {
        properties.push(gtk::accessible::Property::Description(description));
    }
    button.update_property(&properties);

    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_defaults_accessible_label_to_visible_label() {
        let resolved = BigButtonContentSpec::new("document-open-symbolic", "Open").resolved();

        assert_eq!(resolved.icon_name, "document-open-symbolic");
        assert_eq!(resolved.label, "Open");
        assert_eq!(resolved.accessible_label, "Open");
        assert_eq!(resolved.accessible_description, None);
        assert_eq!(resolved.tooltip, None);
        assert!(!resolved.can_shrink);
        assert!(!resolved.use_underline);
    }

    #[test]
    fn builder_records_a11y_tooltip_and_classes() {
        let resolved = BigButtonContentSpec::new("folder-new-symbolic", "Add Folder")
            .accessible_label("Add music folder")
            .accessible_description("Choose a folder to monitor")
            .tooltip("Add a monitored music folder")
            .css_classes(["suggested-action", "pill"])
            .can_shrink()
            .use_underline()
            .resolved();

        assert_eq!(resolved.accessible_label, "Add music folder");
        assert_eq!(
            resolved.accessible_description.as_deref(),
            Some("Choose a folder to monitor")
        );
        assert_eq!(
            resolved.tooltip.as_deref(),
            Some("Add a monitored music folder")
        );
        assert_eq!(resolved.css_classes, ["suggested-action", "pill"]);
        assert!(resolved.can_shrink);
        assert!(resolved.use_underline);
    }
}
