// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Entry row revealed by a dropdown selection.

use adw::prelude::*;
use relm4::gtk;

const DEFAULT_SPACING: i32 = 8;
const DEFAULT_MARGIN_START: i32 = 16;
const DEFAULT_MARGIN_END: i32 = 16;
const DEFAULT_MARGIN_BOTTOM: i32 = 12;

/// Display-free specification describing big revealed entry row behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigRevealedEntryRowSpec {
    /// Label.
    pub label: String,
    /// Visible index.
    pub visible_index: u32,
    /// Spacing.
    pub spacing: i32,
    /// Margin start.
    pub margin_start: i32,
    /// Margin end.
    pub margin_end: i32,
    /// Margin bottom.
    pub margin_bottom: i32,
}

impl BigRevealedEntryRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(label: impl Into<String>, visible_index: u32) -> Self {
        Self {
            label: label.into(),
            visible_index,
            spacing: DEFAULT_SPACING,
            margin_start: DEFAULT_MARGIN_START,
            margin_end: DEFAULT_MARGIN_END,
            margin_bottom: DEFAULT_MARGIN_BOTTOM,
        }
    }

    /// Construct a [`BigRevealedEntryRowSpec`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn last_option(label: impl Into<String>, option_count: usize) -> Self {
        Self::new(label, option_count.saturating_sub(1) as u32)
    }

    /// Configure the `spacing` setting and return the updated builder.
    ///
    /// The supplied `spacing` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigRevealedEntryRowSpec`].
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing.max(0);
        self
    }

    /// Configure the `margins` setting and return the updated builder.
    ///
    /// The supplied `start` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigRevealedEntryRowSpec`].
    #[must_use]
    pub fn margins(mut self, start: i32, end: i32, bottom: i32) -> Self {
        self.margin_start = start.max(0);
        self.margin_end = end.max(0);
        self.margin_bottom = bottom.max(0);
        self
    }

    /// Returns `true` if visible.
    #[must_use]
    pub fn is_visible(&self, selected: u32) -> bool {
        selected == self.visible_index
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self, selected: u32) -> BigRevealedEntryRowResolved {
        BigRevealedEntryRowResolved {
            visible: self.is_visible(selected),
            spacing: self.spacing.max(0),
            margin_start: self.margin_start.max(0),
            margin_end: self.margin_end.max(0),
            margin_bottom: self.margin_bottom.max(0),
        }
    }
}

/// Resolved counterpart of `BigRevealedEntryRowSpec` ready for the widget adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigRevealedEntryRowResolved {
    /// Visible.
    pub visible: bool,
    /// Spacing.
    pub spacing: i32,
    /// Margin start.
    pub margin_start: i32,
    /// Margin end.
    pub margin_end: i32,
    /// Margin bottom.
    pub margin_bottom: i32,
}

/// Entry row whose actual `gtk::Entry` is hidden behind a reveal
/// animation; used when the row should look static until interacted
/// with.
#[derive(Debug, Clone)]
pub struct BigRevealedEntryRow {
    root: gtk::Box,
    spec: BigRevealedEntryRowSpec,
}

impl BigRevealedEntryRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        spec: BigRevealedEntryRowSpec,
        dropdown: &gtk::DropDown,
        entry: &gtk::Entry,
    ) -> Self {
        let resolved = spec.resolved(dropdown.selected());
        let root = gtk::Box::new(gtk::Orientation::Horizontal, resolved.spacing);
        root.set_margin_start(resolved.margin_start);
        root.set_margin_end(resolved.margin_end);
        root.set_margin_bottom(resolved.margin_bottom);

        root.append(&gtk::Label::new(Some(&spec.label)));
        entry.set_hexpand(true);
        root.append(entry);
        root.set_visible(resolved.visible);

        {
            let row = root.clone();
            let spec = spec.clone();
            dropdown.connect_selected_notify(move |dropdown| {
                row.set_visible(spec.is_visible(dropdown.selected()));
            });
        }

        Self { root, spec }
    }

    /// Return a reference to the `root` exposed by this [`BigRevealedEntryRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Consume `self` and yield the underlying root.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }

    /// Return a reference to the `spec` exposed by this [`BigRevealedEntryRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn spec(&self) -> &BigRevealedEntryRowSpec {
        &self.spec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_only_on_configured_index() {
        let spec = BigRevealedEntryRowSpec::new("Custom:", 3);
        assert!(!spec.is_visible(2));
        assert!(spec.is_visible(3));
    }

    #[test]
    fn last_option_handles_empty_lists() {
        let spec = BigRevealedEntryRowSpec::last_option("Custom:", 0);
        assert_eq!(spec.visible_index, 0);
    }

    #[test]
    fn resolved_clamps_negative_layout_values() {
        let resolved = BigRevealedEntryRowSpec::new("Custom:", 1)
            .spacing(-1)
            .margins(-2, -3, -4)
            .resolved(1);

        assert!(resolved.visible);
        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.margin_start, 0);
        assert_eq!(resolved.margin_end, 0);
        assert_eq!(resolved.margin_bottom, 0);
    }
}
