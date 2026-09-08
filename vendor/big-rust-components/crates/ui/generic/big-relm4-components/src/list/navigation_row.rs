// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux row that opens another view/dialog.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

use super::row_core::{action_label_or_title, row_text};

const DEFAULT_TRAILING_ICON: &str = "go-next-symbolic";

/// Data used to build a navigation row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigNavigationRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Prefix icon pixel size.
    pub prefix_icon_pixel_size: Option<i32>,
    /// Trailing icon name.
    pub trailing_icon_name: String,
    /// Action label.
    pub action_label: Option<String>,
    /// Allow markup.
    pub allow_markup: bool,
}

impl BigNavigationRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            prefix_icon_name: None,
            prefix_icon_pixel_size: None,
            trailing_icon_name: DEFAULT_TRAILING_ICON.to_string(),
            action_label: None,
            allow_markup: false,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigNavigationRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `prefix_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigNavigationRowSpec`].
    #[must_use]
    pub fn prefix_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.prefix_icon_name = Some(icon_name.into());
        self
    }

    /// Configure the `prefix_icon_pixel_size` setting and return the updated builder.
    ///
    /// The supplied `pixel_size` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigNavigationRowSpec`].
    #[must_use]
    pub fn prefix_icon_pixel_size(mut self, pixel_size: i32) -> Self {
        self.prefix_icon_pixel_size = Some(pixel_size.max(1));
        self
    }

    /// Configure the `trailing_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigNavigationRowSpec`].
    #[must_use]
    pub fn trailing_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.trailing_icon_name = icon_name.into();
        self
    }

    /// Configure the `action_label` setting and return the updated builder.
    ///
    /// The supplied `label` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigNavigationRowSpec`].
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
    pub fn resolved(&self) -> BigNavigationRowResolved {
        BigNavigationRowResolved {
            title: row_text(&self.title, self.allow_markup),
            subtitle: self
                .subtitle
                .as_ref()
                .map(|subtitle| row_text(subtitle, self.allow_markup)),
            prefix_icon_name: self.prefix_icon_name.clone(),
            prefix_icon_pixel_size: self.prefix_icon_pixel_size,
            trailing_icon_name: self.trailing_icon_name.clone(),
            action_label: action_label_or_title(&self.action_label, &self.title),
        }
    }
}

/// Pure resolved row contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigNavigationRowResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Prefix icon pixel size.
    pub prefix_icon_pixel_size: Option<i32>,
    /// Trailing icon name.
    pub trailing_icon_name: String,
    /// Action label.
    pub action_label: String,
}

/// Built navigation row plus explicit action button.
#[derive(Debug, Clone)]
pub struct BigNavigationRow {
    root: adw::ActionRow,
    action_button: gtk::Button,
}

impl BigNavigationRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigNavigationRowSpec) -> Self {
        let resolved = spec.resolved();
        let mut builder = adw::ActionRow::builder()
            .title(resolved.title.as_str())
            .activatable(true);

        if let Some(subtitle) = resolved.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }

        let root = builder.build();

        if let Some(icon_name) = resolved.prefix_icon_name.as_deref() {
            let icon = gtk::Image::from_icon_name(icon_name);
            if let Some(pixel_size) = resolved.prefix_icon_pixel_size {
                icon.set_pixel_size(pixel_size);
            }
            root.add_prefix(&icon);
        }

        let action_button = tooltip::icon_button(
            resolved.trailing_icon_name.as_str(),
            &resolved.action_label,
            &["flat", "circular"],
        );
        let root_weak = root.downgrade();
        action_button.connect_clicked(move |_| {
            if let Some(root) = root_weak.upgrade() {
                ActionRowExt::activate(&root);
            }
        });
        root.add_suffix(&action_button);

        Self {
            root,
            action_button,
        }
    }

    /// Return a reference to the `root` exposed by this [`BigNavigationRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::ActionRow {
        &self.root
    }

    /// Return a reference to the `action button` exposed by this [`BigNavigationRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn action_button(&self) -> &gtk::Button {
        &self.action_button
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (adw::ActionRow, gtk::Button) {
        (self.root, self.action_button)
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
    fn spec_defaults_to_go_next_row() {
        let spec = BigNavigationRowSpec::new("Audio Settings");

        assert_eq!(spec.title, "Audio Settings");
        assert_eq!(spec.subtitle, None);
        assert_eq!(spec.prefix_icon_name, None);
        assert_eq!(spec.prefix_icon_pixel_size, None);
        assert_eq!(spec.trailing_icon_name, "go-next-symbolic");
        assert_eq!(spec.action_label, None);
        assert!(!spec.allow_markup);
    }

    #[test]
    fn spec_builder_records_optional_parts() {
        let spec = BigNavigationRowSpec::new("Audio Settings")
            .subtitle("Noise removal")
            .prefix_icon_name("audio-volume-high-symbolic")
            .prefix_icon_pixel_size(24)
            .trailing_icon_name("document-open-symbolic")
            .action_label("Open audio settings")
            .allow_markup();

        assert_eq!(spec.subtitle.as_deref(), Some("Noise removal"));
        assert_eq!(
            spec.prefix_icon_name.as_deref(),
            Some("audio-volume-high-symbolic")
        );
        assert_eq!(spec.prefix_icon_pixel_size, Some(24));
        assert_eq!(spec.trailing_icon_name, "document-open-symbolic");
        assert_eq!(spec.action_label.as_deref(), Some("Open audio settings"));
        assert!(spec.allow_markup);
    }

    #[test]
    fn spec_clamps_prefix_pixel_size() {
        let spec = BigNavigationRowSpec::new("Audio Settings").prefix_icon_pixel_size(0);

        assert_eq!(spec.prefix_icon_pixel_size, Some(1));
    }

    #[test]
    fn resolved_defaults_action_label_to_raw_title() {
        let resolved = BigNavigationRowSpec::new("<b>Audio</b>").resolved();

        assert_eq!(resolved.title, "&lt;b&gt;Audio&lt;/b&gt;");
        assert_eq!(resolved.action_label, "<b>Audio</b>");
    }

    #[test]
    fn resolved_escapes_markup_by_default() {
        let resolved = BigNavigationRowSpec::new("<b>Audio</b>")
            .subtitle("<i>Noise</i>")
            .resolved();

        assert_eq!(resolved.title, "&lt;b&gt;Audio&lt;/b&gt;");
        assert_eq!(
            resolved.subtitle.as_deref(),
            Some("&lt;i&gt;Noise&lt;/i&gt;")
        );
    }

    #[test]
    fn resolved_can_allow_markup_explicitly() {
        let resolved = BigNavigationRowSpec::new("<b>Audio</b>")
            .subtitle("<i>Noise</i>")
            .allow_markup()
            .resolved();

        assert_eq!(resolved.title, "<b>Audio</b>");
        assert_eq!(resolved.subtitle.as_deref(), Some("<i>Noise</i>"));
    }
}
