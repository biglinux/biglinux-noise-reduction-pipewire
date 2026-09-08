// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux policy wrapper for `AdwComboRow` state-holder rows.

use adw::prelude::*;
use relm4::gtk;

use crate::text::escape_markup_text;

/// Data used to build an `AdwComboRow`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigComboRowSpec {
    /// Row title.
    pub title: String,
    /// Optional row subtitle.
    pub subtitle: Option<String>,
    /// User-visible labels exposed by the row model.
    pub labels: Vec<String>,
    /// Selected row index.
    pub selected: u32,
    /// Allow libadwaita markup in title/subtitle.
    pub allow_markup: bool,
}

impl BigComboRowSpec {
    /// Create a combo row spec with the supplied labels.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        labels: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            labels: labels.into_iter().map(Into::into).collect(),
            selected: 0,
            allow_markup: false,
        }
    }

    /// Create a combo row spec without an initial model.
    #[must_use]
    pub fn empty(title: impl Into<String>) -> Self {
        Self::new(title, std::iter::empty::<String>())
    }

    /// Configure the row subtitle.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the selected row index.
    #[must_use]
    pub fn selected(mut self, selected: u32) -> Self {
        self.selected = selected;
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
    pub fn resolved(&self) -> BigComboRowResolved {
        let max_index = self.labels.len().saturating_sub(1) as u32;

        BigComboRowResolved {
            title: row_text(&self.title, self.allow_markup),
            subtitle: self
                .subtitle
                .as_ref()
                .map(|subtitle| row_text(subtitle, self.allow_markup)),
            labels: self.labels.clone(),
            selected: self.selected.min(max_index),
        }
    }
}

/// Pure resolved row contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigComboRowResolved {
    /// Row title.
    pub title: String,
    /// Optional row subtitle.
    pub subtitle: Option<String>,
    /// User-visible labels exposed by the row model.
    pub labels: Vec<String>,
    /// Selected row index after clamping.
    pub selected: u32,
}

/// Built combo row.
#[derive(Debug, Clone)]
pub struct BigComboRow {
    root: adw::ComboRow,
}

impl BigComboRow {
    /// Build an `AdwComboRow` from the shared spec.
    #[must_use]
    pub fn new(spec: BigComboRowSpec) -> Self {
        let resolved = spec.resolved();
        let mut builder = adw::ComboRow::builder().title(resolved.title.as_str());

        if let Some(subtitle) = resolved.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }

        let root = builder.build();
        root.update_property(&[gtk::accessible::Property::Label(&resolved.title)]);

        if !resolved.labels.is_empty() {
            let label_refs: Vec<&str> = resolved.labels.iter().map(String::as_str).collect();
            let model = gtk::StringList::new(&label_refs);
            root.set_model(Some(&model));
            root.set_selected(resolved.selected);
        }

        Self { root }
    }

    /// Borrow the root combo row.
    #[must_use]
    pub fn root(&self) -> &adw::ComboRow {
        &self.root
    }

    /// Consume `self` and yield the underlying combo row.
    #[must_use]
    pub fn into_root(self) -> adw::ComboRow {
        self.root
    }
}

fn row_text(text: &str, allow_markup: bool) -> String {
    if allow_markup {
        text.to_string()
    } else {
        escape_markup_text(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_to_first_option_plain_text() {
        let spec = BigComboRowSpec::new("GPU", ["Automatic", "VA-API"]);

        assert_eq!(spec.title, "GPU");
        assert_eq!(spec.subtitle, None);
        assert_eq!(spec.labels, ["Automatic", "VA-API"]);
        assert_eq!(spec.selected, 0);
        assert!(!spec.allow_markup);
    }

    #[test]
    fn resolved_clamps_selected_index() {
        let resolved = BigComboRowSpec::new("GPU", ["Automatic", "VA-API"])
            .selected(99)
            .resolved();

        assert_eq!(resolved.selected, 1);
    }

    #[test]
    fn resolved_supports_empty_state_holder_rows() {
        let resolved = BigComboRowSpec::empty("GPU Device").selected(3).resolved();

        assert_eq!(resolved.labels, Vec::<String>::new());
        assert_eq!(resolved.selected, 0);
    }

    #[test]
    fn resolved_escapes_markup_by_default() {
        let resolved = BigComboRowSpec::new("<b>GPU</b>", ["Automatic"])
            .subtitle("<i>Device</i>")
            .resolved();

        assert_eq!(resolved.title, "&lt;b&gt;GPU&lt;/b&gt;");
        assert_eq!(
            resolved.subtitle.as_deref(),
            Some("&lt;i&gt;Device&lt;/i&gt;")
        );
    }

    #[test]
    fn resolved_can_allow_markup_explicitly() {
        let resolved = BigComboRowSpec::new("<b>GPU</b>", ["Automatic"])
            .subtitle("<i>Device</i>")
            .allow_markup()
            .resolved();

        assert_eq!(resolved.title, "<b>GPU</b>");
        assert_eq!(resolved.subtitle.as_deref(), Some("<i>Device</i>"));
    }
}
