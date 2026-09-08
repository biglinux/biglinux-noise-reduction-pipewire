// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Plain BigLinux detail row.

use adw::prelude::*;
use relm4::gtk;

use super::row_core::row_text;

/// Data used to build a non-action detail row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDetailRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Allow markup.
    pub allow_markup: bool,
}

impl BigDetailRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            prefix_icon_name: None,
            allow_markup: false,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigDetailRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `prefix_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigDetailRowSpec`].
    #[must_use]
    pub fn prefix_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.prefix_icon_name = Some(icon_name.into());
        self
    }

    /// Allow libadwaita markup in title/subtitle.
    ///
    /// Default is plain text; strings are escaped before they reach the row.
    #[must_use]
    pub fn allow_markup(mut self) -> Self {
        self.allow_markup = true;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigDetailRowResolved {
        BigDetailRowResolved {
            title: row_text(&self.title, self.allow_markup),
            subtitle: self
                .subtitle
                .as_ref()
                .map(|subtitle| row_text(subtitle, self.allow_markup)),
            prefix_icon_name: self.prefix_icon_name.clone(),
        }
    }
}

/// Pure resolved row contract. Safe to test without GTK/display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDetailRowResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
}

/// Built non-action row.
#[derive(Debug, Clone)]
pub struct BigDetailRow {
    root: adw::ActionRow,
}

impl BigDetailRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDetailRowSpec) -> Self {
        let resolved = spec.resolved();
        let mut builder = adw::ActionRow::builder().title(resolved.title.as_str());

        if let Some(subtitle) = resolved.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }

        let root = builder.build();

        if let Some(icon_name) = resolved.prefix_icon_name.as_deref() {
            let icon = gtk::Image::from_icon_name(icon_name);
            icon.set_accessible_role(gtk::AccessibleRole::Presentation);
            root.add_prefix(&icon);
        }

        Self { root }
    }

    /// Return a reference to the `root` exposed by this [`BigDetailRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::ActionRow {
        &self.root
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
    fn spec_defaults_to_plain_detail_row() {
        let spec = BigDetailRowSpec::new("Codec");

        assert_eq!(spec.title, "Codec");
        assert_eq!(spec.subtitle, None);
        assert_eq!(spec.prefix_icon_name, None);
        assert!(!spec.allow_markup);
    }

    #[test]
    fn resolved_escapes_markup_by_default() {
        let resolved = BigDetailRowSpec::new("<b>Codec</b>")
            .subtitle("<i>H.264</i>")
            .resolved();

        assert_eq!(resolved.title, "&lt;b&gt;Codec&lt;/b&gt;");
        assert_eq!(
            resolved.subtitle.as_deref(),
            Some("&lt;i&gt;H.264&lt;/i&gt;")
        );
    }

    #[test]
    fn resolved_can_allow_markup_explicitly() {
        let resolved = BigDetailRowSpec::new("<b>Codec</b>")
            .subtitle("<i>H.264</i>")
            .allow_markup()
            .resolved();

        assert_eq!(resolved.title, "<b>Codec</b>");
        assert_eq!(resolved.subtitle.as_deref(), Some("<i>H.264</i>"));
    }

    #[test]
    fn spec_builder_records_prefix_icon() {
        let spec =
            BigDetailRowSpec::new("Missing shaders").prefix_icon_name("dialog-warning-symbolic");

        assert_eq!(
            spec.prefix_icon_name.as_deref(),
            Some("dialog-warning-symbolic")
        );
    }
}
