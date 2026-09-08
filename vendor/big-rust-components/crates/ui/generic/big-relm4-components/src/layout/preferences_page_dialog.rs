// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Multi-page preferences dialog scaffold.
//!
//! Use this wrapper for app settings surfaces that need one or more
//! `adw::PreferencesPage`s and optional built-in search. Use
//! [`super::preferences_dialog::BigPreferencesDialog`] for explicit
//! Cancel/Save form dialogs.

use adw::prelude::*;
use relm4::gtk;

const DEFAULT_CONTENT_WIDTH: i32 = 640;
const DEFAULT_CONTENT_HEIGHT: i32 = 480;

/// Build-time configuration for [`BigPreferencesPageDialog`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPreferencesPageDialogSpec {
    /// Dialog title.
    pub title: String,
    /// Content width.
    pub content_width: i32,
    /// Content height.
    pub content_height: i32,
    /// Whether the libadwaita search UI is enabled.
    pub search_enabled: bool,
}

impl BigPreferencesPageDialogSpec {
    /// Create a spec with BigLinux defaults for a settings/preferences surface.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            content_width: DEFAULT_CONTENT_WIDTH,
            content_height: DEFAULT_CONTENT_HEIGHT,
            search_enabled: false,
        }
    }

    /// Override the content size.
    #[must_use]
    pub fn size(mut self, width: i32, height: i32) -> Self {
        self.content_width = width;
        self.content_height = height;
        self
    }

    /// Enable or disable the built-in preferences search UI.
    #[must_use]
    pub fn search_enabled(mut self, enabled: bool) -> Self {
        self.search_enabled = enabled;
        self
    }
}

/// Built multi-page preferences dialog.
#[derive(Debug, Clone)]
pub struct BigPreferencesPageDialog {
    dialog: adw::PreferencesDialog,
}

impl BigPreferencesPageDialog {
    /// Build a preferences dialog from `spec`.
    #[must_use]
    pub fn new(spec: &BigPreferencesPageDialogSpec) -> Self {
        let dialog = adw::PreferencesDialog::builder()
            .title(spec.title.as_str())
            .content_width(spec.content_width)
            .content_height(spec.content_height)
            .search_enabled(spec.search_enabled)
            .build();
        Self { dialog }
    }

    /// Return the underlying libadwaita dialog.
    #[must_use]
    pub fn dialog(&self) -> &adw::PreferencesDialog {
        &self.dialog
    }

    /// Add a preferences page.
    pub fn add_page(&self, page: &adw::PreferencesPage) {
        self.dialog.add(page);
    }

    /// Select a page by its `name`.
    pub fn set_visible_page_name(&self, name: &str) {
        self.dialog.set_visible_page_name(name);
    }

    /// Present the dialog, optionally transient for `parent`.
    pub fn present(&self, parent: Option<&gtk::Window>) {
        self.dialog.present(parent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_match_multi_page_preferences_surface() {
        let spec = BigPreferencesPageDialogSpec::new("Preferences");
        assert_eq!(spec.title, "Preferences");
        assert_eq!(spec.content_width, DEFAULT_CONTENT_WIDTH);
        assert_eq!(spec.content_height, DEFAULT_CONTENT_HEIGHT);
        assert!(!spec.search_enabled);
    }

    #[test]
    fn spec_size_overrides_defaults() {
        let spec = BigPreferencesPageDialogSpec::new("Preferences").size(960, 680);
        assert_eq!(spec.content_width, 960);
        assert_eq!(spec.content_height, 680);
    }

    #[test]
    fn spec_search_flag_is_explicit() {
        let spec = BigPreferencesPageDialogSpec::new("Preferences").search_enabled(true);
        assert!(spec.search_enabled);
    }
}
