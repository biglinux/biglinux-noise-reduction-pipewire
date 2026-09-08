// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Copyable key/value detail rows.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

use super::row_core::escaped_text;

/// Shared accumulator for "Copy all" buttons.
#[derive(Debug, Clone, Default)]
pub struct BigDetailLines {
    lines: Rc<RefCell<Vec<String>>>,
}

impl BigDetailLines {
    /// Creates a new instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `line` to the buffer; later flushed by
    /// [`Self::joined`] when the user requests a copy.
    pub fn push(&self, line: impl Into<String>) {
        self.lines.borrow_mut().push(line.into());
    }

    /// Return a reference to the `joined` exposed by this [`BigDetailLines`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn joined(&self) -> String {
        self.lines.borrow().join("\n")
    }
}

/// Data needed to build a copyable detail row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigCopyableDetailRowSpec {
    /// Title.
    pub title: String,
    /// Value.
    pub value: String,
    /// Copy text.
    pub copy_text: Option<String>,
    /// Copy label.
    pub copy_label: String,
    /// Escape markup.
    pub escape_markup: bool,
    /// Selectable subtitle.
    pub selectable_subtitle: bool,
}

impl BigCopyableDetailRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        value: impl Into<String>,
        copy_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            value: value.into(),
            copy_text: None,
            copy_label: copy_label.into(),
            escape_markup: true,
            selectable_subtitle: true,
        }
    }

    /// Configure the `copy_text` setting and return the updated builder.
    ///
    /// The supplied `copy_text` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigCopyableDetailRowSpec`].
    #[must_use]
    pub fn copy_text(mut self, copy_text: impl Into<String>) -> Self {
        self.copy_text = Some(copy_text.into());
        self
    }

    /// Configure the `escape_markup` setting and return the updated builder.
    ///
    /// The supplied `escape_markup` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigCopyableDetailRowSpec`].
    #[must_use]
    pub fn escape_markup(mut self, escape_markup: bool) -> Self {
        self.escape_markup = escape_markup;
        self
    }

    /// Configure the `selectable_subtitle` setting and return the updated builder.
    ///
    /// The supplied `selectable` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigCopyableDetailRowSpec`].
    #[must_use]
    pub fn selectable_subtitle(mut self, selectable: bool) -> Self {
        self.selectable_subtitle = selectable;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigCopyableDetailRowResolved {
        BigCopyableDetailRowResolved {
            title: row_text(&self.title, self.escape_markup),
            value: row_text(&self.value, self.escape_markup),
            line: format!("{}: {}", self.title, self.value),
            copy_text: self.copy_text.clone().unwrap_or_else(|| self.value.clone()),
            copy_label: self.copy_label.clone(),
            selectable_subtitle: self.selectable_subtitle,
        }
    }
}

/// Pure row contract. Safe for no-display tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigCopyableDetailRowResolved {
    /// Title.
    pub title: String,
    /// Value.
    pub value: String,
    /// Line.
    pub line: String,
    /// Copy text.
    pub copy_text: String,
    /// Copy label.
    pub copy_label: String,
    /// Selectable subtitle.
    pub selectable_subtitle: bool,
}

/// Built detail row plus copy button.
#[derive(Debug, Clone)]
pub struct BigCopyableDetailRow {
    row: adw::ActionRow,
    copy_button: gtk::Button,
}

impl BigCopyableDetailRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigCopyableDetailRowSpec, lines: Option<&BigDetailLines>) -> Self {
        let resolved = spec.resolved();
        if let Some(lines) = lines {
            lines.push(resolved.line.clone());
        }

        let row = adw::ActionRow::builder()
            .title(&resolved.title)
            .subtitle(&resolved.value)
            .subtitle_selectable(resolved.selectable_subtitle)
            .build();
        let copy_button =
            tooltip::icon_button("edit-copy-symbolic", &resolved.copy_label, &["flat"]);
        copy_button.set_valign(gtk::Align::Center);
        let value = resolved.copy_text;
        copy_button.connect_clicked(move |button| {
            button.clipboard().set_text(&value);
        });
        row.add_suffix(&copy_button);

        Self { row, copy_button }
    }

    /// Return a reference to the `row` exposed by this [`BigCopyableDetailRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn row(&self) -> &adw::ActionRow {
        &self.row
    }

    /// Return a reference to the `copy button` exposed by this [`BigCopyableDetailRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn copy_button(&self) -> &gtk::Button {
        &self.copy_button
    }

    /// Consume `self` and yield the underlying row.
    #[must_use]
    pub fn into_row(self) -> adw::ActionRow {
        self.row
    }
}

/// Build a copy-all button for accumulated detail lines.
#[must_use]
pub fn copy_all_button(copy_all_label: &str, lines: BigDetailLines) -> gtk::Button {
    let button = tooltip::icon_button("edit-copy-symbolic", copy_all_label, &[]);
    button.connect_clicked(move |button| {
        button.clipboard().set_text(&lines.joined());
    });
    button
}

fn row_text(text: &str, escape_markup: bool) -> String {
    if escape_markup {
        escaped_text(text)
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_escapes_markup_but_keeps_raw_copy_line() {
        let resolved = BigCopyableDetailRowSpec::new("<Title>", "A&B", "Copy").resolved();

        assert_eq!(resolved.title, "&lt;Title&gt;");
        assert_eq!(resolved.value, "A&amp;B");
        assert_eq!(resolved.line, "<Title>: A&B");
        assert_eq!(resolved.copy_text, "A&B");
    }

    #[test]
    fn resolved_can_copy_text_distinct_from_display_value() {
        let resolved = BigCopyableDetailRowSpec::new("Tunnel", "127.0.0.1:8080 -> host:80", "Copy")
            .copy_text("127.0.0.1:8080")
            .resolved();

        assert_eq!(resolved.value, "127.0.0.1:8080 -&gt; host:80");
        assert_eq!(resolved.copy_text, "127.0.0.1:8080");
    }

    #[test]
    fn detail_lines_join_in_order() {
        let lines = BigDetailLines::new();
        lines.push("A: 1");
        lines.push("B: 2");

        assert_eq!(lines.joined(), "A: 1\nB: 2");
    }
}
