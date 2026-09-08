// SPDX-License-Identifier: MIT

//! Tab workspace spec — lifecycle, groups, split layout.
//!
//! Combines the visual contract of `tab_strip` with explicit lifecycle
//! semantics: open, close, reorder, move-to-window, and tab groups.
//! Apps describe the workspace as data; the widget builder turns it
//! into `AdwTabView` + group decorations.

/// Tab identity is a string id so the workspace is serde-friendly.
pub type BigTabId = String;

/// Group identity (string id). `None` means ungrouped.
pub type BigTabGroupId = String;

/// Close-button policy (mirrors `tab_strip`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigTabCloseMode {
    /// Close button always visible on every tab.
    AlwaysVisible,
    /// Close button revealed on hover only.
    HoverOnly,
    /// Close button only on the active tab.
    ActiveOnly,
}

/// One row in the tab strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTab {
    /// Stable identifier used to route input messages to this tab.
    pub id: BigTabId,
    /// Translated label rendered in the tab header.
    pub title: String,
    /// Optional symbolic icon name preceding the label.
    pub icon_name: Option<String>,
    /// Optional group this tab belongs to (used for tab grouping UI).
    pub group: Option<BigTabGroupId>,
    /// Mark tab as "needs attention" (pulsing indicator).
    pub needs_attention: bool,
    /// Mark tab as "pinned" (cannot be auto-closed; smaller width).
    pub pinned: bool,
}

impl BigTab {
    /// Creates a new instance.
    #[must_use]
    pub fn new(id: impl Into<BigTabId>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon_name: None,
            group: None,
            needs_attention: false,
            pinned: false,
        }
    }

    /// Builder: sets icon.
    #[must_use]
    pub fn with_icon(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    /// Builder: sets group.
    #[must_use]
    pub fn with_group(mut self, group: impl Into<BigTabGroupId>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Mark this tab as pinned (smaller width, immune to bulk close).
    #[must_use]
    pub fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }
}

/// Group descriptor that ties together related tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTabGroup {
    /// Stable identifier referenced by member tabs.
    pub id: BigTabGroupId,
    /// Translated label shown on group dividers.
    pub title: String,
    /// Optional CSS color hint (hex string) for the group accent.
    pub color_hint: Option<String>,
}

impl BigTabGroup {
    /// Creates a new instance.
    #[must_use]
    pub fn new(id: impl Into<BigTabGroupId>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            color_hint: None,
        }
    }

    /// Builder: sets color.
    #[must_use]
    pub fn with_color(mut self, hex: impl Into<String>) -> Self {
        self.color_hint = Some(hex.into());
        self
    }
}

/// Display-free specification describing big tab workspace behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTabWorkspaceSpec {
    /// Ordered tab list rendered left-to-right.
    pub tabs: Vec<BigTab>,
    /// Tab groups referenced by `BigTab::group`.
    pub groups: Vec<BigTabGroup>,
    /// Currently active tab id (`None` defaults to the first tab on mount).
    pub active: Option<BigTabId>,
    /// Policy controlling close-button availability and confirmation prompts.
    pub close_mode: BigTabCloseMode,
}

impl Default for BigTabWorkspaceSpec {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            groups: Vec::new(),
            active: None,
            close_mode: BigTabCloseMode::HoverOnly,
        }
    }
}

impl BigTabWorkspaceSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a new tab and make it active. Returns `Err` when the id
    /// already exists.
    ///
    /// # Errors
    /// Returns the duplicate id string.
    pub fn open(&mut self, tab: BigTab) -> Result<(), String> {
        if self.tabs.iter().any(|t| t.id == tab.id) {
            return Err(format!("duplicate tab id: {}", tab.id));
        }
        let id = tab.id.clone();
        self.tabs.push(tab);
        self.active = Some(id);
        Ok(())
    }

    /// Close the tab and, when it was active, fall back to the
    /// previous tab.
    ///
    /// # Errors
    /// Returns the missing id when the workspace has no such tab.
    pub fn close(&mut self, id: &str) -> Result<(), String> {
        let Some(idx) = self.tabs.iter().position(|t| t.id == id) else {
            return Err(format!("missing tab id: {id}"));
        };
        self.tabs.remove(idx);
        if self.active.as_deref() == Some(id) {
            self.active = self
                .tabs
                .get(idx.saturating_sub(1))
                .or_else(|| self.tabs.first())
                .map(|t| t.id.clone());
        }
        Ok(())
    }

    /// Reorder by moving `from` to position `to` (drop-before semantics).
    ///
    /// # Errors
    /// Returns descriptive errors for out-of-range positions.
    pub fn reorder(&mut self, from: usize, to: usize) -> Result<(), String> {
        if from >= self.tabs.len() {
            return Err(format!("from {from} out of range"));
        }
        if to > self.tabs.len() {
            return Err(format!("to {to} out of range"));
        }
        if from == to {
            return Ok(());
        }
        let tab = self.tabs.remove(from);
        let adjusted = if to > from { to - 1 } else { to };
        self.tabs.insert(adjusted.min(self.tabs.len()), tab);
        Ok(())
    }

    /// Make `id` active.
    ///
    /// # Errors
    /// Returns the missing id when not found.
    pub fn set_active(&mut self, id: &str) -> Result<(), String> {
        if !self.tabs.iter().any(|t| t.id == id) {
            return Err(format!("missing tab id: {id}"));
        }
        self.active = Some(id.to_string());
        Ok(())
    }

    /// Return the current `active tab` value held by this [`BigTabWorkspaceSpec`].
    #[must_use]
    pub fn active_tab(&self) -> Option<&BigTab> {
        let id = self.active.as_ref()?;
        self.tabs.iter().find(|t| &t.id == id)
    }

    /// Tabs grouped by group id. Ungrouped tabs use the empty string.
    #[must_use]
    pub fn tabs_by_group(&self) -> Vec<(String, Vec<&BigTab>)> {
        let mut order: Vec<String> = Vec::new();
        let mut groups: std::collections::BTreeMap<String, Vec<&BigTab>> =
            std::collections::BTreeMap::new();
        for t in &self.tabs {
            let key = t.group.clone().unwrap_or_default();
            if !groups.contains_key(&key) {
                order.push(key.clone());
            }
            groups.entry(key).or_default().push(t);
        }
        order
            .into_iter()
            .map(|k| {
                let v = groups.remove(&k).unwrap_or_default();
                (k, v)
            })
            .collect()
    }

    /// Return the current `len` value held by this [`BigTabWorkspaceSpec`].
    #[must_use]
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Returns `true` if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

/// Index-keyed selection math for a tab strip whose tabs are tracked by
/// position (e.g. a `FactoryVecDeque`-backed custom strip), as opposed to the
/// id-keyed [`BigTabWorkspaceSpec`]. These pure helpers let a widget host keep
/// its selected-index correct across a remove or a reorder without re-deriving
/// the edge cases each time.
///
/// Given the index just removed, the previously-selected index, and the strip
/// length *after* the removal, return the index that should become selected
/// (or `None` when the strip is now empty or nothing was selected).
///
/// When the selected tab itself was removed, selection falls back to the tab
/// now occupying that slot, clamped to the last tab.
#[must_use]
pub fn selection_index_after_remove(
    removed_idx: usize,
    prev_selected: Option<usize>,
    new_len: usize,
) -> Option<usize> {
    if new_len == 0 {
        return None;
    }
    let target = match prev_selected {
        None => return None,
        Some(sel) if sel == removed_idx => removed_idx.min(new_len - 1),
        Some(sel) if sel > removed_idx => sel - 1,
        Some(sel) => sel,
    };
    Some(target)
}

/// Remap a selected index after a tab moves from `from` to `to` (drop-before
/// semantics, matching [`BigTabWorkspaceSpec::reorder`]). Returns the selected
/// tab's new index so the host can keep highlighting the same tab.
#[must_use]
pub const fn selection_index_after_move(selected: usize, from: usize, to: usize) -> usize {
    if from < to {
        if selected > from && selected <= to {
            selected - 1
        } else {
            selected
        }
    } else if selected >= to && selected < from {
        selected + 1
    } else {
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_after_remove_empty_strip_is_none() {
        assert_eq!(selection_index_after_remove(0, Some(0), 0), None);
    }

    #[test]
    fn selection_after_remove_nothing_selected_is_none() {
        assert_eq!(selection_index_after_remove(1, None, 3), None);
    }

    #[test]
    fn selection_after_remove_selected_tab_falls_back_to_same_slot() {
        // Removed the selected tab at idx 1; new len 2 → slot 1 still exists.
        assert_eq!(selection_index_after_remove(1, Some(1), 2), Some(1));
    }

    #[test]
    fn selection_after_remove_selected_last_tab_clamps() {
        // Removed the selected last tab (was idx 2); new len 2 → clamp to 1.
        assert_eq!(selection_index_after_remove(2, Some(2), 2), Some(1));
    }

    #[test]
    fn selection_after_remove_before_selected_shifts_down() {
        // Removed idx 0, selection was idx 2 → now idx 1.
        assert_eq!(selection_index_after_remove(0, Some(2), 3), Some(1));
    }

    #[test]
    fn selection_after_remove_after_selected_unchanged() {
        // Removed idx 2, selection was idx 0 → unchanged.
        assert_eq!(selection_index_after_remove(2, Some(0), 3), Some(0));
    }

    #[test]
    fn selection_after_move_forward_within_span_shifts_down() {
        assert_eq!(selection_index_after_move(2, 0, 3), 1);
    }

    #[test]
    fn selection_after_move_backward_within_span_shifts_up() {
        assert_eq!(selection_index_after_move(1, 3, 0), 2);
    }

    #[test]
    fn selection_after_move_outside_span_unchanged() {
        assert_eq!(selection_index_after_move(5, 0, 3), 5);
        assert_eq!(selection_index_after_move(0, 1, 3), 0);
    }

    #[test]
    fn open_first_tab_sets_active() {
        let mut w = BigTabWorkspaceSpec::new();
        w.open(BigTab::new("t1", "Tab 1")).unwrap();
        assert_eq!(w.active.as_deref(), Some("t1"));
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn open_duplicate_id_rejected() {
        let mut w = BigTabWorkspaceSpec::new();
        w.open(BigTab::new("t1", "Tab 1")).unwrap();
        let err = w.open(BigTab::new("t1", "Dup")).unwrap_err();
        assert!(err.contains("duplicate"));
    }

    #[test]
    fn close_active_falls_back_to_previous() {
        let mut w = BigTabWorkspaceSpec::new();
        w.open(BigTab::new("t1", "1")).unwrap();
        w.open(BigTab::new("t2", "2")).unwrap();
        w.open(BigTab::new("t3", "3")).unwrap();
        w.set_active("t2").unwrap();
        w.close("t2").unwrap();
        assert_eq!(w.active.as_deref(), Some("t1"));
    }

    #[test]
    fn close_last_clears_active() {
        let mut w = BigTabWorkspaceSpec::new();
        w.open(BigTab::new("t1", "1")).unwrap();
        w.close("t1").unwrap();
        assert!(w.active.is_none());
        assert!(w.is_empty());
    }

    #[test]
    fn reorder_moves_drop_before_semantics() {
        let mut w = BigTabWorkspaceSpec::new();
        w.open(BigTab::new("a", "A")).unwrap();
        w.open(BigTab::new("b", "B")).unwrap();
        w.open(BigTab::new("c", "C")).unwrap();
        w.reorder(0, 2).unwrap();
        let ids: Vec<&str> = w.tabs.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a", "c"]);
    }

    #[test]
    fn tabs_by_group_preserves_first_seen_order() {
        let mut w = BigTabWorkspaceSpec::new();
        w.open(BigTab::new("t1", "1").with_group("g1")).unwrap();
        w.open(BigTab::new("t2", "2").with_group("g2")).unwrap();
        w.open(BigTab::new("t3", "3").with_group("g1")).unwrap();
        let groups = w.tabs_by_group();
        let order: Vec<&str> = groups.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(order, vec!["g1", "g2"]);
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[1].1.len(), 1);
    }

    #[test]
    fn ungrouped_tabs_use_empty_group_key() {
        let mut w = BigTabWorkspaceSpec::new();
        w.open(BigTab::new("t1", "1")).unwrap();
        let groups = w.tabs_by_group();
        assert_eq!(groups[0].0, "");
    }

    #[test]
    fn set_active_unknown_id_rejected() {
        let mut w = BigTabWorkspaceSpec::new();
        let err = w.set_active("nope").unwrap_err();
        assert!(err.contains("missing"));
    }

    #[test]
    fn pinned_tab_preserves_flag() {
        let mut w = BigTabWorkspaceSpec::new();
        w.open(BigTab::new("pinned", "P").pinned()).unwrap();
        assert!(w.tabs[0].pinned);
    }
}
