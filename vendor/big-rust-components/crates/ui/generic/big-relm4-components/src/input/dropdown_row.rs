// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux action row with a dropdown suffix.

use adw::prelude::*;
use relm4::gtk;

use crate::text::escape_markup_text;

/// Data used to build a dropdown row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDropdownRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Labels.
    pub labels: Vec<String>,
    /// Selected.
    pub selected: u32,
    /// Allow markup.
    pub allow_markup: bool,
}

impl BigDropdownRowSpec {
    /// Creates a new instance.
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

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigDropdownRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `selected` setting and return the updated builder.
    ///
    /// The supplied `selected` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigDropdownRowSpec`].
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
    pub fn resolved(&self) -> BigDropdownRowResolved {
        let max_index = self.labels.len().saturating_sub(1) as u32;

        BigDropdownRowResolved {
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
pub struct BigDropdownRowResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Labels.
    pub labels: Vec<String>,
    /// Selected.
    pub selected: u32,
}

/// Built row plus its dropdown.
#[derive(Debug, Clone)]
pub struct BigDropdownRow {
    root: adw::ActionRow,
    dropdown: gtk::DropDown,
}

impl BigDropdownRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDropdownRowSpec) -> Self {
        let resolved = spec.resolved();
        let mut builder = adw::ActionRow::builder().title(resolved.title.as_str());

        if let Some(subtitle) = resolved.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }

        let root = builder.build();
        let label_refs: Vec<&str> = resolved.labels.iter().map(String::as_str).collect();
        let dropdown = gtk::DropDown::from_strings(&label_refs);
        dropdown.set_selected(resolved.selected);
        dropdown.set_valign(gtk::Align::Center);
        root.add_suffix(&dropdown);
        root.set_activatable_widget(Some(&dropdown));

        Self { root, dropdown }
    }

    /// Return a reference to the `root` exposed by this [`BigDropdownRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::ActionRow {
        &self.root
    }

    /// Return a reference to the `dropdown` exposed by this [`BigDropdownRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn dropdown(&self) -> &gtk::DropDown {
        &self.dropdown
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (adw::ActionRow, gtk::DropDown) {
        (self.root, self.dropdown)
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
        let spec = BigDropdownRowSpec::new("Output", ["Join", "Separate"]);

        assert_eq!(spec.title, "Output");
        assert_eq!(spec.subtitle, None);
        assert_eq!(spec.labels, ["Join", "Separate"]);
        assert_eq!(spec.selected, 0);
        assert!(!spec.allow_markup);
    }

    #[test]
    fn resolved_clamps_selected_index() {
        let resolved = BigDropdownRowSpec::new("Output", ["Join", "Separate"])
            .selected(99)
            .resolved();

        assert_eq!(resolved.selected, 1);
    }

    #[test]
    fn resolved_escapes_markup_by_default() {
        let resolved = BigDropdownRowSpec::new("<b>Output</b>", ["Join"])
            .subtitle("<i>Mode</i>")
            .resolved();

        assert_eq!(resolved.title, "&lt;b&gt;Output&lt;/b&gt;");
        assert_eq!(
            resolved.subtitle.as_deref(),
            Some("&lt;i&gt;Mode&lt;/i&gt;")
        );
    }

    #[test]
    fn resolved_can_allow_markup_explicitly() {
        let resolved = BigDropdownRowSpec::new("<b>Output</b>", ["Join"])
            .subtitle("<i>Mode</i>")
            .allow_markup()
            .resolved();

        assert_eq!(resolved.title, "<b>Output</b>");
        assert_eq!(resolved.subtitle.as_deref(), Some("<i>Mode</i>"));
    }
}
