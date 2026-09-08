// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Small dropdown helpers for dialog controls.

use adw::prelude::*;
use relm4::gtk;

/// Data used to build a standalone inline dropdown control.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigInlineDropdownSpec {
    /// User-visible labels exposed by the dropdown model.
    pub labels: Vec<String>,
    /// Selected row index.
    pub selected: u32,
}

impl BigInlineDropdownSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            labels: labels.into_iter().map(Into::into).collect(),
            selected: 0,
        }
    }

    /// Configure the selected row index.
    #[must_use]
    pub fn selected(mut self, selected: u32) -> Self {
        self.selected = selected;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigInlineDropdownResolved {
        let max_index = self.labels.len().saturating_sub(1) as u32;

        BigInlineDropdownResolved {
            labels: self.labels.clone(),
            selected: self.selected.min(max_index),
        }
    }
}

/// Pure resolved inline dropdown contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigInlineDropdownResolved {
    /// User-visible labels exposed by the dropdown model.
    pub labels: Vec<String>,
    /// Selected row index after clamping.
    pub selected: u32,
}

/// Built standalone inline dropdown control.
#[derive(Debug, Clone)]
pub struct BigInlineDropdown {
    root: gtk::DropDown,
}

impl BigInlineDropdown {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigInlineDropdownSpec) -> Self {
        let resolved = spec.resolved();
        let label_refs: Vec<&str> = resolved.labels.iter().map(String::as_str).collect();
        let root = gtk::DropDown::from_strings(&label_refs);
        root.set_selected(resolved.selected);
        root.set_valign(gtk::Align::Center);
        Self { root }
    }

    /// Return a reference to the root dropdown.
    #[must_use]
    pub fn root(&self) -> &gtk::DropDown {
        &self.root
    }

    /// Consume `self` and yield the underlying dropdown.
    #[must_use]
    pub fn into_root(self) -> gtk::DropDown {
        self.root
    }
}

/// Build a `DropDown` from static translated labels.
#[must_use]
pub fn dropdown_from_strings(labels: &[&str]) -> gtk::DropDown {
    gtk::DropDown::from_strings(labels)
}

/// Keep two `DropDown`s' selected index in sync (e.g. a dialog copy and a source
/// control). Both directions hold the *other* dropdown WEAK to avoid a mutual
/// reference cycle that would leak both widgets; each handler upgrades-or-skips
/// and guards against notify ping-pong.
pub fn sync_dropdowns(dialog_dropdown: &gtk::DropDown, source_dropdown: &gtk::DropDown) {
    dialog_dropdown.set_selected(source_dropdown.selected());

    {
        let source_weak = source_dropdown.downgrade();
        dialog_dropdown.connect_selected_notify(move |dropdown| {
            if let Some(source) = source_weak.upgrade()
                && source.selected() != dropdown.selected()
            {
                source.set_selected(dropdown.selected());
            }
        });
    }

    {
        let dialog_weak = dialog_dropdown.downgrade();
        source_dropdown.connect_selected_notify(move |source| {
            if let Some(dialog) = dialog_weak.upgrade()
                && dialog.selected() != source.selected()
            {
                dialog.set_selected(source.selected());
            }
        });
    }
}

/// Keep a dialog `DropDown` and a source `ComboRow` selected index in sync.
///
/// Both directions hold the *other* widget WEAK: a two-way strong bind makes the
/// two widgets pin each other (mutual reference cycle) so neither finalizes when
/// the dialog closes. Each handler upgrades-or-skips, so syncing works while both
/// are alive and silently stops once one is dropped.
pub fn sync_dropdown_with_combo_row(dialog_dropdown: &gtk::DropDown, source_combo: &adw::ComboRow) {
    dialog_dropdown.set_selected(source_combo.selected());

    {
        let source_weak = source_combo.downgrade();
        dialog_dropdown.connect_selected_notify(move |dropdown| {
            if let Some(source_combo) = source_weak.upgrade() {
                source_combo.set_selected(dropdown.selected());
            }
        });
    }

    {
        let dialog_weak = dialog_dropdown.downgrade();
        source_combo.connect_selected_notify(move |combo| {
            if let Some(dialog_dropdown) = dialog_weak.upgrade() {
                dialog_dropdown.set_selected(combo.selected());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_dropdown_defaults_to_first_option() {
        let resolved = BigInlineDropdownSpec::new(["Light", "Dark"]).resolved();

        assert_eq!(resolved.labels, ["Light", "Dark"]);
        assert_eq!(resolved.selected, 0);
    }

    #[test]
    fn inline_dropdown_clamps_selected_index() {
        let resolved = BigInlineDropdownSpec::new(["Light", "Dark"])
            .selected(99)
            .resolved();

        assert_eq!(resolved.selected, 1);
    }
}
