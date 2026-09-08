// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Entry controls paired with a trailing action widget.

use adw::prelude::*;
use relm4::gtk;

use crate::text::escape_markup_text;

const DEFAULT_WIDTH_CHARS: i32 = 24;
const DEFAULT_SPACING: i32 = 8;

/// Data used to build an inline entry plus trailing action control.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEntryActionControlSpec {
    /// Optional initial entry text.
    pub text: Option<String>,
    /// Optional entry placeholder text.
    pub placeholder: Option<String>,
    /// Entry width in characters.
    pub width_chars: i32,
    /// Space between the entry and trailing action.
    pub spacing: i32,
    /// Whether the entry text can be edited.
    pub editable: bool,
    /// Whether the entry can receive focus.
    pub can_focus: bool,
}

impl BigEntryActionControlSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: None,
            placeholder: None,
            width_chars: DEFAULT_WIDTH_CHARS,
            spacing: DEFAULT_SPACING,
            editable: true,
            can_focus: true,
        }
    }

    /// Configure the entry text.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Configure the entry placeholder text.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Configure the entry width in characters.
    #[must_use]
    pub fn width_chars(mut self, width_chars: i32) -> Self {
        self.width_chars = width_chars;
        self
    }

    /// Configure the space between the entry and trailing action.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Configure the entry as read-only and non-focusable.
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.editable = false;
        self.can_focus = false;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigEntryActionControlResolved {
        BigEntryActionControlResolved {
            text: self.text.clone(),
            placeholder: self.placeholder.clone(),
            width_chars: self.width_chars.max(1),
            spacing: self.spacing.max(0),
            editable: self.editable,
            can_focus: self.can_focus,
        }
    }
}

impl Default for BigEntryActionControlSpec {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure resolved entry/action control contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEntryActionControlResolved {
    /// Optional initial entry text.
    pub text: Option<String>,
    /// Optional entry placeholder text.
    pub placeholder: Option<String>,
    /// Entry width in characters.
    pub width_chars: i32,
    /// Space between the entry and trailing action.
    pub spacing: i32,
    /// Whether the entry text can be edited.
    pub editable: bool,
    /// Whether the entry can receive focus.
    pub can_focus: bool,
}

/// Built inline entry plus trailing action control.
#[derive(Debug, Clone)]
pub struct BigEntryActionControl {
    root: gtk::Box,
    entry: gtk::Entry,
}

impl BigEntryActionControl {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigEntryActionControlSpec, action: &impl IsA<gtk::Widget>) -> Self {
        let resolved = spec.resolved();
        let entry = gtk::Entry::builder()
            .hexpand(true)
            .width_chars(resolved.width_chars)
            .editable(resolved.editable)
            .can_focus(resolved.can_focus)
            .valign(gtk::Align::Center)
            .build();

        if let Some(text) = resolved.text.as_deref() {
            entry.set_text(text);
        }

        if let Some(placeholder) = resolved.placeholder.as_deref() {
            entry.set_placeholder_text(Some(placeholder));
        }

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(resolved.spacing)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();
        root.append(&entry);
        root.append(action);

        Self { root, entry }
    }

    /// Return a reference to the root widget.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the entry widget.
    #[must_use]
    pub fn entry(&self) -> &gtk::Entry {
        &self.entry
    }

    /// Consume `self` and yield the underlying widgets.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, gtk::Entry) {
        (self.root, self.entry)
    }
}

/// Data used to build an `ActionRow` containing an entry/action control.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEntryActionRowSpec {
    /// Row title.
    pub title: String,
    /// Optional row subtitle.
    pub subtitle: Option<String>,
    /// Inline entry/action control spec.
    pub control: BigEntryActionControlSpec,
}

impl BigEntryActionRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            control: BigEntryActionControlSpec::new(),
        }
    }

    /// Configure the row subtitle.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the inline control.
    #[must_use]
    pub fn control(mut self, control: BigEntryActionControlSpec) -> Self {
        self.control = control;
        self
    }
}

/// Built row containing an entry/action control.
#[derive(Debug, Clone)]
pub struct BigEntryActionRow {
    root: adw::ActionRow,
    control: BigEntryActionControl,
}

impl BigEntryActionRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigEntryActionRowSpec, action: &impl IsA<gtk::Widget>) -> Self {
        let mut builder = adw::ActionRow::builder()
            .title(spec.title.as_str())
            .activatable(false);

        if let Some(subtitle) = spec.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }

        let root = builder.build();
        let control = BigEntryActionControl::new(spec.control, action);
        root.add_suffix(control.root());

        Self { root, control }
    }

    /// Return a reference to the root row.
    #[must_use]
    pub fn root(&self) -> &adw::ActionRow {
        &self.root
    }

    /// Return a reference to the entry widget.
    #[must_use]
    pub fn entry(&self) -> &gtk::Entry {
        self.control.entry()
    }

    /// Consume `self` and yield the row.
    #[must_use]
    pub fn into_root(self) -> adw::ActionRow {
        self.root
    }
}

/// Data used to build an `EntryRow` with a trailing action widget.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEntryRowActionSpec {
    /// Row title.
    pub title: String,
    /// Optional initial row text.
    pub text: Option<String>,
    /// Allow libadwaita markup in the title.
    pub allow_markup: bool,
}

impl BigEntryRowActionSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            text: None,
            allow_markup: false,
        }
    }

    /// Configure the initial row text.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Allow libadwaita markup in the title.
    ///
    /// Default is plain text; strings are escaped before they reach the row.
    #[must_use]
    pub fn allow_markup(mut self) -> Self {
        self.allow_markup = true;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigEntryRowActionResolved {
        BigEntryRowActionResolved {
            title: if self.allow_markup {
                self.title.clone()
            } else {
                escape_markup_text(&self.title)
            },
            text: self.text.clone(),
        }
    }
}

/// Pure resolved entry-row/action contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEntryRowActionResolved {
    /// Row title.
    pub title: String,
    /// Optional initial row text.
    pub text: Option<String>,
}

/// Built `EntryRow` with a trailing action widget.
#[derive(Debug, Clone)]
pub struct BigEntryRowAction {
    root: adw::EntryRow,
}

impl BigEntryRowAction {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigEntryRowActionSpec, action: &impl IsA<gtk::Widget>) -> Self {
        let resolved = spec.resolved();
        let root = adw::EntryRow::builder()
            .title(resolved.title.as_str())
            .build();

        if let Some(text) = resolved.text.as_deref() {
            root.set_text(text);
        }

        root.add_suffix(action);

        Self { root }
    }

    /// Return a reference to the root row.
    #[must_use]
    pub fn root(&self) -> &adw::EntryRow {
        &self.root
    }

    /// Consume `self` and yield the row.
    #[must_use]
    pub fn into_root(self) -> adw::EntryRow {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_spec_defaults_to_editable_entry() {
        let resolved = BigEntryActionControlSpec::new().resolved();

        assert_eq!(resolved.width_chars, 24);
        assert_eq!(resolved.spacing, 8);
        assert!(resolved.editable);
        assert!(resolved.can_focus);
    }

    #[test]
    fn control_spec_clamps_layout_values() {
        let resolved = BigEntryActionControlSpec::new()
            .width_chars(-10)
            .spacing(-5)
            .resolved();

        assert_eq!(resolved.width_chars, 1);
        assert_eq!(resolved.spacing, 0);
    }

    #[test]
    fn read_only_control_is_not_focusable() {
        let resolved = BigEntryActionControlSpec::new().read_only().resolved();

        assert!(!resolved.editable);
        assert!(!resolved.can_focus);
    }

    #[test]
    fn row_spec_records_title_subtitle_and_control() {
        let spec = BigEntryActionRowSpec::new("Folder")
            .subtitle("Pick a folder")
            .control(BigEntryActionControlSpec::new().placeholder("Home"));

        assert_eq!(spec.title, "Folder");
        assert_eq!(spec.subtitle.as_deref(), Some("Pick a folder"));
        assert_eq!(spec.control.placeholder.as_deref(), Some("Home"));
    }

    #[test]
    fn entry_row_action_spec_escapes_title_by_default() {
        let resolved = BigEntryRowActionSpec::new("<b>Folder</b>")
            .text("/home")
            .resolved();

        assert_eq!(resolved.title, "&lt;b&gt;Folder&lt;/b&gt;");
        assert_eq!(resolved.text.as_deref(), Some("/home"));
    }

    #[test]
    fn entry_row_action_spec_can_allow_markup() {
        let resolved = BigEntryRowActionSpec::new("<b>Folder</b>")
            .allow_markup()
            .resolved();

        assert_eq!(resolved.title, "<b>Folder</b>");
    }
}
