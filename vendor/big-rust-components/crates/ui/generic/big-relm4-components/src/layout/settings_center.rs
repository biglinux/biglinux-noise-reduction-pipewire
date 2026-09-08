// SPDX-License-Identifier: MIT

//! Settings center spec (typed bindings, dirty tracking, search).
//!
//! Sits on top of the existing `layout::control_center` chrome and
//! `input::preference_cards` widgets. Apps describe their pages and
//! bindings here; the widget builder turns the spec into a real Adwaita
//! preferences-window tree.
//!
//! Key invariants pinned by tests:
//!
//! - Each binding has a unique id.
//! - Saving only commits bindings that are dirty.
//! - Reverting clears dirty state and restores `committed` values.
//! - Search filters by visible label, help, and category id.

use std::collections::{BTreeMap, HashSet};

/// Binding payload kind. Drives widget choice in the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigSettingsKind {
    /// Boolean toggle (`AdwSwitchRow`).
    Bool,
    /// Free-form single-line text (`AdwEntryRow`).
    Text,
    /// Filesystem path with picker button.
    Path,
    /// Integer spin row.
    Integer,
    /// Floating-point spin row.
    Decimal,
    /// Drop-down picking one of a fixed set.
    Enum,
    /// Colour picker row.
    Color,
    /// Keyboard shortcut capture row.
    Shortcut,
}

/// Typed binding between a settings key and its UI representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSettingsBinding {
    /// Stable key (`"player.gapless"`). Used by persistence layer.
    pub key: String,
    /// User-facing label (i18n upstream).
    pub label: String,
    /// One-line help text below the row.
    pub help: Option<String>,
    /// Widget kind.
    pub kind: BigSettingsKind,
    /// Category id (used for grouping into pages).
    pub category: String,
    /// Default ("factory") value as a stringified blob.
    pub default: String,
    /// Last-committed value (loaded from persistence at startup).
    pub committed: String,
    /// Pending UI value (may differ from `committed`).
    pub pending: String,
}

impl BigSettingsBinding {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        kind: BigSettingsKind,
        category: impl Into<String>,
        default: impl Into<String>,
    ) -> Self {
        let default = default.into();
        Self {
            key: key.into(),
            label: label.into(),
            help: None,
            kind,
            category: category.into(),
            committed: default.clone(),
            pending: default.clone(),
            default,
        }
    }

    /// Builder: sets help.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Initialise both `committed` and `pending` from persisted storage.
    pub fn load(&mut self, value: impl Into<String>) {
        let v = value.into();
        self.committed = v.clone();
        self.pending = v;
    }

    /// Stage a UI value (does not commit).
    pub fn stage(&mut self, value: impl Into<String>) {
        self.pending = value.into();
    }

    /// Commit `pending` → `committed`. Returns true when state changed.
    pub fn commit(&mut self) -> bool {
        if self.pending == self.committed {
            return false;
        }
        self.committed = self.pending.clone();
        true
    }

    /// Discard staged change.
    pub fn revert(&mut self) {
        self.pending = self.committed.clone();
    }

    /// Restore the factory default (pending only — caller must commit).
    pub fn reset_to_default(&mut self) {
        self.pending = self.default.clone();
    }

    /// Returns `true` if dirty.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.pending != self.committed
    }

    /// Returns `true` if default.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.pending == self.default
    }
}

/// A page collects bindings under a single category id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSettingsPage {
    /// Id.
    pub id: String,
    /// Title.
    pub title: String,
    /// Icon name.
    pub icon_name: Option<String>,
    /// Help summary.
    pub help_summary: Option<String>,
}

impl BigSettingsPage {
    /// Creates a new instance.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon_name: None,
            help_summary: None,
        }
    }

    /// Builder: sets icon.
    #[must_use]
    pub fn with_icon(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    /// Builder: sets help summary.
    #[must_use]
    pub fn with_help_summary(mut self, summary: impl Into<String>) -> Self {
        self.help_summary = Some(summary.into());
        self
    }
}

/// Settings center spec.
#[derive(Debug, Clone)]
pub struct BigSettingsCenter {
    pages: Vec<BigSettingsPage>,
    bindings: Vec<BigSettingsBinding>,
}

impl BigSettingsCenter {
    /// Build with declared pages.
    #[must_use]
    pub fn new(pages: Vec<BigSettingsPage>) -> Self {
        Self {
            pages,
            bindings: Vec::new(),
        }
    }

    /// Register a binding. Rejects duplicates by `key` and bindings
    /// whose `category` does not match any declared page.
    pub fn add_binding(&mut self, binding: BigSettingsBinding) -> Result<(), String> {
        if self.bindings.iter().any(|b| b.key == binding.key) {
            return Err(format!("duplicate binding key: {}", binding.key));
        }
        if !self.pages.iter().any(|p| p.id == binding.category) {
            return Err(format!(
                "binding {} references unknown category {}",
                binding.key, binding.category
            ));
        }
        self.bindings.push(binding);
        Ok(())
    }

    /// Borrow the declared pages.
    #[must_use]
    pub fn pages(&self) -> &[BigSettingsPage] {
        &self.pages
    }

    /// Borrow every registered binding in registration order.
    #[must_use]
    pub fn bindings(&self) -> &[BigSettingsBinding] {
        &self.bindings
    }

    /// Find a binding by `key` for mutation. `None` when no binding
    /// uses that key.
    pub fn binding_mut(&mut self, key: &str) -> Option<&mut BigSettingsBinding> {
        self.bindings.iter_mut().find(|b| b.key == key)
    }

    /// Group bindings by page id, preserving declared order.
    #[must_use]
    pub fn bindings_by_page(&self) -> BTreeMap<&str, Vec<&BigSettingsBinding>> {
        let mut map: BTreeMap<&str, Vec<&BigSettingsBinding>> = BTreeMap::new();
        for page in &self.pages {
            map.entry(page.id.as_str()).or_default();
        }
        for b in &self.bindings {
            map.entry(b.category.as_str()).or_default().push(b);
        }
        map
    }

    /// Search across labels, help text, and category ids.
    #[must_use]
    pub fn search<'a>(&'a self, needle: &str) -> Vec<&'a BigSettingsBinding> {
        if needle.is_empty() {
            return self.bindings.iter().collect();
        }
        let lower = needle.to_lowercase();
        self.bindings
            .iter()
            .filter(|b| {
                b.label.to_lowercase().contains(&lower)
                    || b.help
                        .as_deref()
                        .is_some_and(|h| h.to_lowercase().contains(&lower))
                    || b.category.to_lowercase().contains(&lower)
                    || b.key.to_lowercase().contains(&lower)
            })
            .collect()
    }

    /// Ids of dirty bindings.
    #[must_use]
    pub fn dirty_keys(&self) -> Vec<&str> {
        self.bindings
            .iter()
            .filter(|b| b.is_dirty())
            .map(|b| b.key.as_str())
            .collect()
    }

    /// Commit every dirty binding. Returns the keys that were actually
    /// committed.
    pub fn commit_all(&mut self) -> Vec<String> {
        let mut committed = Vec::new();
        for b in self.bindings.iter_mut() {
            if b.commit() {
                committed.push(b.key.clone());
            }
        }
        committed
    }

    /// Discard every staged change.
    pub fn revert_all(&mut self) {
        for b in self.bindings.iter_mut() {
            b.revert();
        }
    }

    /// Audit: ensure no duplicate keys exist (mutation invariant).
    #[must_use]
    pub fn has_unique_keys(&self) -> bool {
        let mut seen = HashSet::new();
        self.bindings.iter().all(|b| seen.insert(b.key.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn center() -> BigSettingsCenter {
        let mut c = BigSettingsCenter::new(vec![
            BigSettingsPage::new("playback", "Playback"),
            BigSettingsPage::new("library", "Library"),
        ]);
        c.add_binding(BigSettingsBinding::new(
            "player.gapless",
            "Gapless playback",
            BigSettingsKind::Bool,
            "playback",
            "true",
        ))
        .unwrap();
        c.add_binding(
            BigSettingsBinding::new(
                "player.volume",
                "Default volume",
                BigSettingsKind::Integer,
                "playback",
                "80",
            )
            .with_help("Initial playback volume on startup."),
        )
        .unwrap();
        c.add_binding(BigSettingsBinding::new(
            "library.folder",
            "Library folder",
            BigSettingsKind::Path,
            "library",
            "~/Music",
        ))
        .unwrap();
        c
    }

    #[test]
    fn add_binding_rejects_duplicate_key() {
        let mut c = center();
        let err = c
            .add_binding(BigSettingsBinding::new(
                "player.gapless",
                "Dup",
                BigSettingsKind::Bool,
                "playback",
                "false",
            ))
            .unwrap_err();
        assert!(err.contains("duplicate"));
    }

    #[test]
    fn add_binding_rejects_unknown_category() {
        let mut c = center();
        let err = c
            .add_binding(BigSettingsBinding::new(
                "x.y",
                "X",
                BigSettingsKind::Bool,
                "nope",
                "false",
            ))
            .unwrap_err();
        assert!(err.contains("unknown category"));
    }

    #[test]
    fn dirty_keys_track_pending_changes() {
        let mut c = center();
        c.binding_mut("player.volume").unwrap().stage("90");
        assert_eq!(c.dirty_keys(), vec!["player.volume"]);
    }

    #[test]
    fn commit_all_clears_dirty_and_reports_changed_keys() {
        let mut c = center();
        c.binding_mut("player.volume").unwrap().stage("90");
        c.binding_mut("library.folder").unwrap().stage("~/Tracks");
        let committed = c.commit_all();
        assert_eq!(committed.len(), 2);
        assert!(c.dirty_keys().is_empty());
    }

    #[test]
    fn revert_all_restores_committed_values() {
        let mut c = center();
        c.binding_mut("player.volume").unwrap().stage("90");
        c.revert_all();
        assert_eq!(c.binding_mut("player.volume").unwrap().pending, "80");
    }

    #[test]
    fn search_matches_label_help_category_and_key() {
        let c = center();
        assert_eq!(c.search("gap").len(), 1);
        assert_eq!(c.search("Initial playback volume").len(), 1);
        assert_eq!(c.search("library").len(), 1);
        assert_eq!(c.search("player.").len(), 2);
    }

    #[test]
    fn bindings_by_page_groups_correctly() {
        let c = center();
        let map = c.bindings_by_page();
        assert_eq!(map.get("playback").unwrap().len(), 2);
        assert_eq!(map.get("library").unwrap().len(), 1);
    }

    #[test]
    fn binding_default_round_trip() {
        let mut b = BigSettingsBinding::new("x", "X", BigSettingsKind::Integer, "cat", "42");
        b.stage("100");
        assert!(b.is_dirty());
        b.reset_to_default();
        // Construction set committed = default = "42"; resetting pending
        // back to default reaches committed, so dirty clears.
        assert_eq!(b.pending, "42");
        assert!(!b.is_dirty());
    }

    #[test]
    fn reset_to_default_after_load_marks_dirty() {
        let mut b = BigSettingsBinding::new("x", "X", BigSettingsKind::Integer, "cat", "42");
        b.load("100"); // simulate persisted value differing from default
        assert!(!b.is_dirty());
        b.reset_to_default();
        assert!(b.is_dirty());
        assert_eq!(b.pending, "42");
        assert_eq!(b.committed, "100");
    }

    #[test]
    fn unique_keys_invariant_holds() {
        let c = center();
        assert!(c.has_unique_keys());
    }
}
