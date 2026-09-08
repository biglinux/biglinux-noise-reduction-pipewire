// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux action row with a compact suffix button strip.

use adw::prelude::*;
use relm4::gtk;

use super::row_core::row_text;

/// Data used to build a compact button-strip row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigButtonStripRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Spacing.
    pub spacing: i32,
    /// Activatable.
    pub activatable: bool,
    /// Allow markup.
    pub allow_markup: bool,
}

impl BigButtonStripRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            prefix_icon_name: None,
            spacing: 4,
            activatable: false,
            allow_markup: false,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigButtonStripRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `prefix_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigButtonStripRowSpec`].
    #[must_use]
    pub fn prefix_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.prefix_icon_name = Some(icon_name.into());
        self
    }

    /// Configure the `spacing` setting and return the updated builder.
    ///
    /// The supplied `spacing` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigButtonStripRowSpec`].
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing.max(0);
        self
    }

    /// Configure the `activatable` setting and return the updated builder.
    ///
    /// The supplied `activatable` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigButtonStripRowSpec`].
    #[must_use]
    pub fn activatable(mut self, activatable: bool) -> Self {
        self.activatable = activatable;
        self
    }

    /// Allow libadwaita markup in title.
    ///
    /// Default is plain text; strings are escaped before they reach the row.
    #[must_use]
    pub fn allow_markup(mut self) -> Self {
        self.allow_markup = true;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigButtonStripRowResolved {
        BigButtonStripRowResolved {
            title: row_text(&self.title, self.allow_markup),
            subtitle: self
                .subtitle
                .as_ref()
                .map(|subtitle| row_text(subtitle, self.allow_markup)),
            prefix_icon_name: self.prefix_icon_name.clone(),
            spacing: self.spacing.max(0),
            activatable: self.activatable,
        }
    }
}

/// Pure resolved row contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigButtonStripRowResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Spacing.
    pub spacing: i32,
    /// Activatable.
    pub activatable: bool,
}

/// Built row plus its button strip.
#[derive(Debug, Clone)]
pub struct BigButtonStripRow {
    root: adw::ActionRow,
    strip: gtk::Box,
}

impl BigButtonStripRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigButtonStripRowSpec) -> Self {
        let resolved = spec.resolved();
        let mut builder = adw::ActionRow::builder()
            .title(resolved.title.as_str())
            .activatable(resolved.activatable);

        if let Some(subtitle) = resolved.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }

        let root = builder.build();

        if let Some(icon_name) = resolved.prefix_icon_name.as_deref() {
            root.add_prefix(&gtk::Image::from_icon_name(icon_name));
        }

        let strip = gtk::Box::builder()
            .spacing(resolved.spacing)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        root.add_suffix(&strip);

        Self { root, strip }
    }

    /// Append `widget` to the horizontal button strip.
    pub fn append(&self, widget: &impl IsA<gtk::Widget>) {
        self.strip.append(widget);
    }

    /// Return a reference to the `root` exposed by this [`BigButtonStripRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::ActionRow {
        &self.root
    }

    /// Return a reference to the `strip` exposed by this [`BigButtonStripRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn strip(&self) -> &gtk::Box {
        &self.strip
    }

    /// Consume `self` and yield the underlying root.
    #[must_use]
    pub fn into_root(self) -> adw::ActionRow {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_to_plain_title_and_compact_spacing() {
        let spec = BigButtonStripRowSpec::new("Transform");

        assert_eq!(spec.title, "Transform");
        assert_eq!(spec.subtitle, None);
        assert_eq!(spec.prefix_icon_name, None);
        assert_eq!(spec.spacing, 4);
        assert!(!spec.activatable);
        assert!(!spec.allow_markup);
    }

    #[test]
    fn spec_builder_records_optional_parts() {
        let spec = BigButtonStripRowSpec::new("Segment")
            .subtitle("00:03")
            .prefix_icon_name("media-playback-start-symbolic")
            .activatable(true);

        assert_eq!(spec.subtitle.as_deref(), Some("00:03"));
        assert_eq!(
            spec.prefix_icon_name.as_deref(),
            Some("media-playback-start-symbolic")
        );
        assert!(spec.activatable);
    }

    #[test]
    fn resolved_clamps_negative_spacing() {
        let resolved = BigButtonStripRowSpec::new("Transform")
            .spacing(-10)
            .resolved();

        assert_eq!(resolved.spacing, 0);
    }

    #[test]
    fn resolved_escapes_markup_by_default() {
        let resolved = BigButtonStripRowSpec::new("<b>Transform</b>")
            .subtitle("<i>Duration</i>")
            .resolved();

        assert_eq!(resolved.title, "&lt;b&gt;Transform&lt;/b&gt;");
        assert_eq!(
            resolved.subtitle.as_deref(),
            Some("&lt;i&gt;Duration&lt;/i&gt;")
        );
    }

    #[test]
    fn resolved_can_allow_markup_explicitly() {
        let resolved = BigButtonStripRowSpec::new("<b>Transform</b>")
            .subtitle("<i>Duration</i>")
            .allow_markup()
            .resolved();

        assert_eq!(resolved.title, "<b>Transform</b>");
        assert_eq!(resolved.subtitle.as_deref(), Some("<i>Duration</i>"));
    }
}
