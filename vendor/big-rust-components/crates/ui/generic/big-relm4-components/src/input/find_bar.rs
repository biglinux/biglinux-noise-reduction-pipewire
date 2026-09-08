// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Inline find bar with previous/next and search mode toggles.

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::feedback::tooltip;
use crate::input::switch::{BigSwitchControl, BigSwitchControlSpec};

const DEFAULT_SPACING: i32 = 6;
const PREVIOUS_ICON: &str = "go-up-symbolic";
const NEXT_ICON: &str = "go-down-symbolic";

/// Data used to build a find bar.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFindBarSpec {
    /// Search entry placeholder.
    pub placeholder: String,
    /// Search entry accessible label.
    pub search_accessible_label: String,
    /// Previous match button label and tooltip.
    pub previous_label: String,
    /// Next match button label and tooltip.
    pub next_label: String,
    /// Visible label for the case-sensitive switch.
    pub case_sensitive_label: String,
    /// Case-sensitive switch accessible label and tooltip.
    pub case_sensitive_accessible_label: String,
    /// Visible label for the regex switch.
    pub regex_label: String,
    /// Regex switch accessible label and tooltip.
    pub regex_accessible_label: String,
    /// Occurrence counter accessible label and tooltip.
    pub occurrence_accessible_label: String,
    /// Initial case-sensitive state.
    pub is_case_sensitive: bool,
    /// Initial regex state.
    pub uses_regex: bool,
    /// Space between controls.
    pub spacing: i32,
}

/// Labels for find bar navigation actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFindBarActionLabels {
    /// Previous match button label and tooltip.
    pub previous: String,
    /// Next match button label and tooltip.
    pub next: String,
}

impl BigFindBarActionLabels {
    /// Create navigation action labels.
    #[must_use]
    pub fn new(previous: impl Into<String>, next: impl Into<String>) -> Self {
        Self {
            previous: previous.into(),
            next: next.into(),
        }
    }
}

/// Visible and accessible labels for a find bar toggle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFindBarToggleLabels {
    /// Visible label.
    pub label: String,
    /// Accessible label and tooltip.
    pub accessible_label: String,
}

impl BigFindBarToggleLabels {
    /// Create toggle labels.
    #[must_use]
    pub fn new(label: impl Into<String>, accessible_label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            accessible_label: accessible_label.into(),
        }
    }
}

impl BigFindBarSpec {
    /// Create a find bar spec with all user-facing labels.
    #[must_use]
    pub fn new(
        placeholder: impl Into<String>,
        search_accessible_label: impl Into<String>,
        actions: BigFindBarActionLabels,
        case_sensitive: BigFindBarToggleLabels,
        regex: BigFindBarToggleLabels,
        occurrence_accessible_label: impl Into<String>,
    ) -> Self {
        Self {
            placeholder: placeholder.into(),
            search_accessible_label: search_accessible_label.into(),
            previous_label: actions.previous,
            next_label: actions.next,
            case_sensitive_label: case_sensitive.label,
            case_sensitive_accessible_label: case_sensitive.accessible_label,
            regex_label: regex.label,
            regex_accessible_label: regex.accessible_label,
            occurrence_accessible_label: occurrence_accessible_label.into(),
            is_case_sensitive: false,
            uses_regex: false,
            spacing: DEFAULT_SPACING,
        }
    }

    /// Set initial search options.
    #[must_use]
    pub fn options(mut self, is_case_sensitive: bool, uses_regex: bool) -> Self {
        self.is_case_sensitive = is_case_sensitive;
        self.uses_regex = uses_regex;
        self
    }

    /// Configure spacing between controls.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigFindBarResolved {
        BigFindBarResolved {
            placeholder: self.placeholder.clone(),
            search_accessible_label: self.search_accessible_label.clone(),
            previous_label: self.previous_label.clone(),
            next_label: self.next_label.clone(),
            case_sensitive_label: self.case_sensitive_label.clone(),
            case_sensitive_accessible_label: self.case_sensitive_accessible_label.clone(),
            regex_label: self.regex_label.clone(),
            regex_accessible_label: self.regex_accessible_label.clone(),
            occurrence_accessible_label: self.occurrence_accessible_label.clone(),
            is_case_sensitive: self.is_case_sensitive,
            uses_regex: self.uses_regex,
            spacing: self.spacing.max(0),
        }
    }
}

/// Pure resolved find bar contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFindBarResolved {
    /// Search entry placeholder.
    pub placeholder: String,
    /// Search entry accessible label.
    pub search_accessible_label: String,
    /// Previous match button label and tooltip.
    pub previous_label: String,
    /// Next match button label and tooltip.
    pub next_label: String,
    /// Visible label for the case-sensitive switch.
    pub case_sensitive_label: String,
    /// Case-sensitive switch accessible label and tooltip.
    pub case_sensitive_accessible_label: String,
    /// Visible label for the regex switch.
    pub regex_label: String,
    /// Regex switch accessible label and tooltip.
    pub regex_accessible_label: String,
    /// Occurrence counter accessible label and tooltip.
    pub occurrence_accessible_label: String,
    /// Initial case-sensitive state.
    pub is_case_sensitive: bool,
    /// Initial regex state.
    pub uses_regex: bool,
    /// Space between controls.
    pub spacing: i32,
}

/// Built find bar.
#[derive(Debug, Clone)]
pub struct BigFindBar {
    root: gtk::SearchBar,
    search_entry: gtk::SearchEntry,
    previous_button: gtk::Button,
    next_button: gtk::Button,
    case_sensitive_switch: gtk::Switch,
    regex_switch: gtk::Switch,
    occurrence_label: gtk::Label,
}

impl BigFindBar {
    /// Build a find bar from a semantic spec.
    #[must_use]
    pub fn new(spec: BigFindBarSpec) -> Self {
        let resolved = spec.resolved();
        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text(&resolved.placeholder)
            .hexpand(true)
            .build();
        search_entry.update_property(&[gtk::accessible::Property::Label(
            &resolved.search_accessible_label,
        )]);

        let previous_button = tooltip::icon_button(PREVIOUS_ICON, &resolved.previous_label, &[]);
        let next_button = tooltip::icon_button(NEXT_ICON, &resolved.next_label, &[]);

        let case_sensitive_switch = BigSwitchControl::new(
            BigSwitchControlSpec::new(
                &resolved.case_sensitive_accessible_label,
                resolved.is_case_sensitive,
            )
            .tooltip(&resolved.case_sensitive_accessible_label),
        )
        .into_root();
        let regex_switch = BigSwitchControl::new(
            BigSwitchControlSpec::new(&resolved.regex_accessible_label, resolved.uses_regex)
                .tooltip(&resolved.regex_accessible_label),
        )
        .into_root();

        let occurrence_label = gtk::Label::new(None);
        occurrence_label.add_css_class("dim-label");
        tooltip::set(&occurrence_label, &resolved.occurrence_accessible_label);
        occurrence_label.update_property(&[gtk::accessible::Property::Label(
            &resolved.occurrence_accessible_label,
        )]);

        let case_box = labeled_switch_row(
            &resolved.case_sensitive_label,
            &case_sensitive_switch,
            resolved.spacing,
        );
        let regex_box = labeled_switch_row(&resolved.regex_label, &regex_switch, resolved.spacing);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, resolved.spacing);
        content.append(&search_entry);
        content.append(&occurrence_label);
        content.append(&case_box);
        content.append(&regex_box);
        content.append(&previous_button);
        content.append(&next_button);

        let root = gtk::SearchBar::builder().child(&content).build();
        root.connect_entry(&search_entry);

        Self {
            root,
            search_entry,
            previous_button,
            next_button,
            case_sensitive_switch,
            regex_switch,
            occurrence_label,
        }
    }

    /// Return the root search bar.
    #[must_use]
    pub fn root(&self) -> &gtk::SearchBar {
        &self.root
    }

    /// Return the search entry.
    #[must_use]
    pub fn search_entry(&self) -> &gtk::SearchEntry {
        &self.search_entry
    }

    /// Return the previous match button.
    #[must_use]
    pub fn previous_button(&self) -> &gtk::Button {
        &self.previous_button
    }

    /// Return the next match button.
    #[must_use]
    pub fn next_button(&self) -> &gtk::Button {
        &self.next_button
    }

    /// Return the case-sensitive switch.
    #[must_use]
    pub fn case_sensitive_switch(&self) -> &gtk::Switch {
        &self.case_sensitive_switch
    }

    /// Return the regex switch.
    #[must_use]
    pub fn regex_switch(&self) -> &gtk::Switch {
        &self.regex_switch
    }

    /// Return the occurrence label.
    #[must_use]
    pub fn occurrence_label(&self) -> &gtk::Label {
        &self.occurrence_label
    }

    /// Consume `self` and return all app-wired parts.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        gtk::SearchBar,
        gtk::SearchEntry,
        gtk::Button,
        gtk::Button,
        gtk::Switch,
        gtk::Switch,
        gtk::Label,
    ) {
        (
            self.root,
            self.search_entry,
            self.previous_button,
            self.next_button,
            self.case_sensitive_switch,
            self.regex_switch,
            self.occurrence_label,
        )
    }
}

fn labeled_switch_row(label: &str, switch: &gtk::Switch, spacing: i32) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, spacing);
    row.append(&gtk::Label::new(Some(label)));
    row.append(switch);
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_bar_spec_resolves_labels_and_options() {
        let resolved = BigFindBarSpec::new(
            "Find",
            "Search",
            BigFindBarActionLabels::new("Previous", "Next"),
            BigFindBarToggleLabels::new("Case", "Case sensitive search"),
            BigFindBarToggleLabels::new("Regex", "Use regular expressions"),
            "Current occurrence",
        )
        .options(true, true)
        .spacing(4)
        .resolved();

        assert_eq!(resolved.placeholder, "Find");
        assert_eq!(resolved.search_accessible_label, "Search");
        assert_eq!(resolved.previous_label, "Previous");
        assert_eq!(resolved.next_label, "Next");
        assert_eq!(resolved.case_sensitive_label, "Case");
        assert_eq!(
            resolved.case_sensitive_accessible_label,
            "Case sensitive search"
        );
        assert_eq!(resolved.regex_label, "Regex");
        assert_eq!(resolved.regex_accessible_label, "Use regular expressions");
        assert_eq!(resolved.occurrence_accessible_label, "Current occurrence");
        assert!(resolved.is_case_sensitive);
        assert!(resolved.uses_regex);
        assert_eq!(resolved.spacing, 4);
    }

    #[test]
    fn find_bar_spec_clamps_negative_spacing() {
        let resolved = BigFindBarSpec::new(
            "Find",
            "Search",
            BigFindBarActionLabels::new("Previous", "Next"),
            BigFindBarToggleLabels::new("Case", "Case"),
            BigFindBarToggleLabels::new("Regex", "Regex"),
            "Count",
        )
        .spacing(-12)
        .resolved();

        assert_eq!(resolved.spacing, 0);
    }
}
