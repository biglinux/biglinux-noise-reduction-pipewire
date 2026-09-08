// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux switch row.

use relm4::adw;

use crate::text::escape_markup_text;

/// Data used to build a switch row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSwitchRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Active.
    pub active: bool,
    /// Allow markup.
    pub allow_markup: bool,
}

impl BigSwitchRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>, active: bool) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            active,
            allow_markup: false,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSwitchRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
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
    pub fn resolved(&self) -> BigSwitchRowResolved {
        BigSwitchRowResolved {
            title: row_text(&self.title, self.allow_markup),
            subtitle: self
                .subtitle
                .as_ref()
                .map(|subtitle| row_text(subtitle, self.allow_markup)),
            active: self.active,
        }
    }
}

/// Pure resolved row contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSwitchRowResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Active.
    pub active: bool,
}

/// Built switch row.
#[derive(Debug, Clone)]
pub struct BigSwitchRow {
    root: adw::SwitchRow,
}

impl BigSwitchRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigSwitchRowSpec) -> Self {
        let resolved = spec.resolved();
        let mut builder = adw::SwitchRow::builder()
            .title(resolved.title.as_str())
            .active(resolved.active);

        if let Some(subtitle) = resolved.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }

        Self {
            root: builder.build(),
        }
    }

    /// Return a reference to the `root` exposed by this [`BigSwitchRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::SwitchRow {
        &self.root
    }

    /// Consume `self` and yield the underlying row.
    #[must_use]
    pub fn into_root(self) -> adw::SwitchRow {
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
