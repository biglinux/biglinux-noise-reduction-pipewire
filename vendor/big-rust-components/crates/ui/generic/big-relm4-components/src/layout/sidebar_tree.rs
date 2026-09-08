// SPDX-License-Identifier: MIT

//! Sidebar tree spec for hierarchical navigation.
//!
//! Used by file manager, package manager, container manager, system
//! monitor, IDE project tree, terminal session tree. Captures items,
//! parent/child relationships, expand state, search filter, and
//! selection.

use std::collections::{HashMap, HashSet};

/// Stable tree node id.
pub type BigNodeId = String;

/// One node in the sidebar tree; links to its parent by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSidebarNode {
    /// Id.
    pub id: BigNodeId,
    /// Label.
    pub label: String,
    /// Icon name.
    pub icon_name: Option<String>,
    /// Parent.
    pub parent: Option<BigNodeId>,
    /// Optional category badge.
    pub badge: Option<String>,
}

impl BigSidebarNode {
    /// Creates a new instance.
    #[must_use]
    pub fn new(id: impl Into<BigNodeId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon_name: None,
            parent: None,
            badge: None,
        }
    }

    /// Builder: sets parent.
    #[must_use]
    pub fn with_parent(mut self, parent: impl Into<BigNodeId>) -> Self {
        self.parent = Some(parent.into());
        self
    }

    /// Builder: sets icon.
    #[must_use]
    pub fn with_icon(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    /// Builder: sets badge.
    #[must_use]
    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }
}

/// Display-free specification describing big sidebar tree behaviour.
#[derive(Debug, Clone)]
pub struct BigSidebarTreeSpec {
    nodes: Vec<BigSidebarNode>,
    expanded: HashSet<BigNodeId>,
    selected: Option<BigNodeId>,
    reorderable: bool,
    show_context_menu: bool,
}

impl Default for BigSidebarTreeSpec {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            expanded: HashSet::new(),
            selected: None,
            reorderable: false,
            show_context_menu: true,
        }
    }
}

impl BigSidebarTreeSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node. Returns `Err` when the id is duplicated or the
    /// parent is missing.
    ///
    /// # Errors
    /// Returns a description string.
    pub fn add(&mut self, node: BigSidebarNode) -> Result<(), String> {
        if self.nodes.iter().any(|n| n.id == node.id) {
            return Err(format!("duplicate id: {}", node.id));
        }
        if let Some(parent) = &node.parent
            && !self.nodes.iter().any(|n| &n.id == parent)
        {
            return Err(format!("unknown parent for {}: {}", node.id, parent));
        }
        self.nodes.push(node);
        Ok(())
    }

    /// Expand the node (no-op when missing).
    pub fn expand(&mut self, id: &str) {
        if self.nodes.iter().any(|n| n.id == id) {
            self.expanded.insert(id.to_string());
        }
    }

    /// Update the `collapse` state stored in this [`BigSidebarTreeSpec`] in place.
    pub fn collapse(&mut self, id: &str) {
        self.expanded.remove(id);
    }

    /// Returns true when the node id is currently expanded.
    #[must_use]
    pub fn is_expanded(&self, id: &str) -> bool {
        self.expanded.contains(id)
    }

    /// Toggle a node's expand state. Returns the new state, or `None`
    /// when the id is missing.
    pub fn toggle(&mut self, id: &str) -> Option<bool> {
        if !self.nodes.iter().any(|n| n.id == id) {
            return None;
        }
        if self.expanded.remove(id) {
            Some(false)
        } else {
            self.expanded.insert(id.to_string());
            Some(true)
        }
    }

    /// Sets selected.
    pub fn set_selected(&mut self, id: Option<&str>) {
        self.selected = id.map(ToString::to_string);
    }

    /// Return the current `selected` value held by this [`BigSidebarTreeSpec`].
    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Return the current `reorderable` value held by this [`BigSidebarTreeSpec`].
    #[must_use]
    pub fn reorderable(&self) -> bool {
        self.reorderable
    }

    /// Sets reorderable.
    pub fn set_reorderable(&mut self, on: bool) {
        self.reorderable = on;
    }

    /// Present the context menu surface.
    #[must_use]
    pub fn show_context_menu(&self) -> bool {
        self.show_context_menu
    }

    /// Sets show context menu.
    pub fn set_show_context_menu(&mut self, on: bool) {
        self.show_context_menu = on;
    }

    /// Return a reference to the `nodes` exposed by this [`BigSidebarTreeSpec`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn nodes(&self) -> &[BigSidebarNode] {
        &self.nodes
    }

    /// Direct children of `id` (or root when `id == None`).
    #[must_use]
    pub fn children_of(&self, id: Option<&str>) -> Vec<&BigSidebarNode> {
        self.nodes
            .iter()
            .filter(|n| n.parent.as_deref() == id)
            .collect()
    }

    /// Build a `child → parent` map. Used by widget builders to render
    /// the tree without re-scanning the slice.
    #[must_use]
    pub fn parent_index(&self) -> HashMap<&str, Option<&str>> {
        self.nodes
            .iter()
            .map(|n| (n.id.as_str(), n.parent.as_deref()))
            .collect()
    }

    /// Filter nodes by `needle` (case-insensitive substring on label).
    /// Returns the matched ids plus every ancestor id (so the renderer
    /// can keep the path visible).
    #[must_use]
    pub fn search(&self, needle: &str) -> HashSet<String> {
        if needle.is_empty() {
            return self.nodes.iter().map(|n| n.id.clone()).collect();
        }
        let lower = needle.to_lowercase();
        let parent_map = self.parent_index();
        let mut hits: HashSet<String> = HashSet::new();
        for n in &self.nodes {
            if n.label.to_lowercase().contains(&lower)
                || n.id.to_lowercase().contains(&lower)
                || n.badge
                    .as_deref()
                    .is_some_and(|b| b.to_lowercase().contains(&lower))
            {
                hits.insert(n.id.clone());
                let mut current = n.parent.clone();
                while let Some(p) = current {
                    hits.insert(p.clone());
                    current = parent_map
                        .get(p.as_str())
                        .copied()
                        .flatten()
                        .map(str::to_string);
                }
            }
        }
        hits
    }

    /// Depth-first traversal for rendering.
    #[must_use]
    pub fn dfs_ids(&self) -> Vec<&str> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack: Vec<&str> = self
            .children_of(None)
            .into_iter()
            .map(|n| n.id.as_str())
            .rev()
            .collect();
        while let Some(id) = stack.pop() {
            out.push(id);
            for child in self.children_of(Some(id)).into_iter().rev() {
                stack.push(child.id.as_str());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> BigSidebarTreeSpec {
        let mut t = BigSidebarTreeSpec::new();
        t.add(BigSidebarNode::new("library", "Library")).unwrap();
        t.add(BigSidebarNode::new("albums", "Albums").with_parent("library"))
            .unwrap();
        t.add(BigSidebarNode::new("artists", "Artists").with_parent("library"))
            .unwrap();
        t.add(BigSidebarNode::new("playlists", "Playlists"))
            .unwrap();
        t.add(BigSidebarNode::new("favourites", "Favourites").with_parent("playlists"))
            .unwrap();
        t
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut t = tree();
        let err = t.add(BigSidebarNode::new("library", "X")).unwrap_err();
        assert!(err.contains("duplicate"));
    }

    #[test]
    fn unknown_parent_rejected() {
        let mut t = tree();
        let err = t
            .add(BigSidebarNode::new("orphan", "X").with_parent("ghost"))
            .unwrap_err();
        assert!(err.contains("unknown parent"));
    }

    #[test]
    fn children_of_root_lists_top_level() {
        let t = tree();
        let roots: Vec<&str> = t.children_of(None).iter().map(|n| n.id.as_str()).collect();
        assert_eq!(roots, vec!["library", "playlists"]);
    }

    #[test]
    fn children_of_parent_lists_descendants() {
        let t = tree();
        let kids: Vec<&str> = t
            .children_of(Some("library"))
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(kids, vec!["albums", "artists"]);
    }

    #[test]
    fn toggle_expands_then_collapses() {
        let mut t = tree();
        assert_eq!(t.toggle("library"), Some(true));
        assert!(t.is_expanded("library"));
        assert_eq!(t.toggle("library"), Some(false));
        assert!(!t.is_expanded("library"));
    }

    #[test]
    fn toggle_unknown_returns_none() {
        let mut t = tree();
        assert!(t.toggle("nope").is_none());
    }

    #[test]
    fn search_includes_ancestors() {
        let t = tree();
        let hits = t.search("albums");
        assert!(hits.contains("albums"));
        assert!(hits.contains("library"));
    }

    #[test]
    fn empty_search_returns_all_ids() {
        let t = tree();
        let all = t.search("");
        assert_eq!(all.len(), t.nodes().len());
    }

    #[test]
    fn dfs_visits_in_declared_order() {
        let t = tree();
        let ids = t.dfs_ids();
        assert_eq!(
            ids,
            vec!["library", "albums", "artists", "playlists", "favourites"]
        );
    }

    #[test]
    fn selection_tracks_set_selected() {
        let mut t = tree();
        t.set_selected(Some("albums"));
        assert_eq!(t.selected(), Some("albums"));
        t.set_selected(None);
        assert!(t.selected().is_none());
    }
}
