// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux navigation/action row helper.

use relm4::gtk;

use super::navigation_row::{BigNavigationRow, BigNavigationRowSpec};
use super::row_core::{action_label_or_title, row_text};

const DEFAULT_ACTION_ICON: &str = "go-next-symbolic";

/// Data used to build a navigation/action row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigActionNavRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Action icon name.
    pub action_icon_name: String,
    /// Action label.
    pub action_label: Option<String>,
    /// Allow markup.
    pub allow_markup: bool,
}

impl BigActionNavRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            prefix_icon_name: None,
            action_icon_name: DEFAULT_ACTION_ICON.to_string(),
            action_label: None,
            allow_markup: false,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigActionNavRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `prefix_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigActionNavRowSpec`].
    #[must_use]
    pub fn prefix_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.prefix_icon_name = Some(icon_name.into());
        self
    }

    /// Configure the `action_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigActionNavRowSpec`].
    #[must_use]
    pub fn action_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.action_icon_name = icon_name.into();
        self
    }

    /// Configure the `action_label` setting and return the updated builder.
    ///
    /// The supplied `label` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigActionNavRowSpec`].
    #[must_use]
    pub fn action_label(mut self, label: impl Into<String>) -> Self {
        self.action_label = Some(label.into());
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
    pub fn resolved(&self) -> BigActionNavRowResolved {
        BigActionNavRowResolved {
            title: row_text(&self.title, self.allow_markup),
            subtitle: self
                .subtitle
                .as_ref()
                .map(|subtitle| row_text(subtitle, self.allow_markup)),
            prefix_icon_name: self.prefix_icon_name.clone(),
            action_icon_name: self.action_icon_name.clone(),
            action_label: action_label_or_title(&self.action_label, &self.title),
        }
    }
}

/// Pure resolved row contract. Safe to test without GTK/display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigActionNavRowResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Action icon name.
    pub action_icon_name: String,
    /// Action label.
    pub action_label: String,
}

/// Built row plus its explicit action button.
#[derive(Debug, Clone)]
pub struct BigActionNavRow {
    inner: BigNavigationRow,
}

impl BigActionNavRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigActionNavRowSpec) -> Self {
        let resolved = spec.resolved();
        let mut spec = BigNavigationRowSpec::new(resolved.title)
            .trailing_icon_name(resolved.action_icon_name)
            .action_label(resolved.action_label)
            .allow_markup();

        if let Some(subtitle) = resolved.subtitle {
            spec = spec.subtitle(subtitle);
        }

        if let Some(prefix_icon_name) = resolved.prefix_icon_name {
            spec = spec.prefix_icon_name(prefix_icon_name);
        }

        Self {
            inner: BigNavigationRow::new(spec),
        }
    }

    /// Return a reference to the `root` exposed by this [`BigActionNavRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::ActionRow {
        self.inner.root()
    }

    /// Return a reference to the `action button` exposed by this [`BigActionNavRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn action_button(&self) -> &gtk::Button {
        self.inner.action_button()
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (adw::ActionRow, gtk::Button) {
        self.inner.into_parts()
    }

    /// Consume `self` and yield the underlying root.
    #[must_use]
    pub fn into_root(self) -> adw::ActionRow {
        self.inner.into_root()
    }
}

/// Construct a [`BigActionNavRow`] populated from the caller-supplied fields.
///
/// All setters/builder methods can still adjust the result before it is
/// passed to the GTK layer.
#[must_use]
pub fn action_nav_row(title: impl Into<String>, subtitle: impl Into<String>) -> BigActionNavRow {
    BigActionNavRow::new(BigActionNavRowSpec::new(title).subtitle(subtitle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_to_named_next_button() {
        let spec = BigActionNavRowSpec::new("Open settings");
        assert_eq!(spec.action_icon_name, "go-next-symbolic");
        assert_eq!(spec.action_label, None);
        assert!(!spec.allow_markup);
    }

    #[test]
    fn spec_builder_records_optional_parts() {
        let spec = BigActionNavRowSpec::new("Title")
            .subtitle("Subtitle")
            .prefix_icon_name("folder-symbolic")
            .action_icon_name("document-edit-symbolic")
            .action_label("Edit")
            .allow_markup();

        assert_eq!(spec.subtitle.as_deref(), Some("Subtitle"));
        assert_eq!(spec.prefix_icon_name.as_deref(), Some("folder-symbolic"));
        assert_eq!(spec.action_icon_name, "document-edit-symbolic");
        assert_eq!(spec.action_label.as_deref(), Some("Edit"));
        assert!(spec.allow_markup);
    }

    #[test]
    fn row_text_escapes_markup_by_default() {
        assert_eq!(row_text("<b>Audio</b>", false), "&lt;b&gt;Audio&lt;/b&gt;");
    }

    #[test]
    fn row_text_can_allow_markup_explicitly() {
        assert_eq!(row_text("<b>Audio</b>", true), "<b>Audio</b>");
    }
}
