// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Reusable collection/list/grid contracts.
//!
//! [`crate::collections::BigCollectionSpec`] captures the user-visible behaviour of a
//! collection view (selection, search, reorder, paging) independent of any
//! widget toolkit. The resolved form ([`crate::collections::BigCollectionResolved`]) is consumed
//! by the Relm4/GTK adapter to decide which sub-widgets to materialise.

/// Visual layout used to render a collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigCollectionMode {
    /// One row per item, stacked vertically.
    List,
    /// Items laid out in a uniform grid.
    Grid,
    /// Grid optimised for thumbnail previews.
    PreviewGrid,
    /// Ordered processing queue (e.g. converter jobs).
    Queue,
    /// Master list paired with a detail pane.
    ListDetail,
}

/// Behavioural contract for a collection view.
///
/// Build with [`BigCollectionSpec::new`] (or a preset like
/// [`BigCollectionSpec::file_list`]) and chain the builder methods.
///
/// # Examples
///
/// ```
/// use big_app_kit_core::collections::{BigCollectionMode, BigCollectionSpec};
///
/// let spec = BigCollectionSpec::file_grid("No files")
///     .searchable(true)
///     .page_size(50);
/// let resolved = spec.resolved();
/// assert!(resolved.requires_search_entry);
/// assert_eq!(resolved.layout_role, "grid");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigCollectionSpec {
    /// Visual layout used by the view.
    pub mode: BigCollectionMode,
    /// Mounts a headerbar search entry; leave `false` for fixed-size
    /// collections where filtering would only add chrome noise.
    pub searchable: bool,
    /// Enables drag handles and row reordering; only meaningful when the
    /// backing model is mutable and preserves order across sessions.
    pub reorderable: bool,
    /// Allows selection gestures to mark rows; disable for purely
    /// informational lists that have no follow-up action.
    pub selectable: bool,
    /// Renders section headers around grouped items; pair with a model
    /// that yields stable group keys to avoid header churn on refresh.
    pub grouped: bool,
    /// Title shown when the collection is empty.
    pub empty_title: String,
    /// Optional page size; `None` means render every item.
    pub page_size: Option<usize>,
}

impl BigCollectionSpec {
    /// Build a spec with the given mode and empty-state title.
    #[must_use]
    pub fn new(mode: BigCollectionMode, empty_title: impl Into<String>) -> Self {
        Self {
            mode,
            searchable: false,
            reorderable: false,
            selectable: true,
            grouped: false,
            empty_title: empty_title.into(),
            page_size: None,
        }
    }

    /// Preset for a flat file list.
    #[must_use]
    pub fn file_list(empty_title: impl Into<String>) -> Self {
        Self::new(BigCollectionMode::List, empty_title)
    }

    /// Preset for a uniform file grid.
    #[must_use]
    pub fn file_grid(empty_title: impl Into<String>) -> Self {
        Self::new(BigCollectionMode::Grid, empty_title)
    }

    /// Preset for a thumbnail preview grid.
    #[must_use]
    pub fn preview_grid(empty_title: impl Into<String>) -> Self {
        Self::new(BigCollectionMode::PreviewGrid, empty_title)
    }

    /// Preset for an ordered, reorderable queue.
    #[must_use]
    pub fn file_queue(empty_title: impl Into<String>) -> Self {
        Self::new(BigCollectionMode::Queue, empty_title).reorderable(true)
    }

    /// Preset for a master/detail split layout.
    #[must_use]
    pub fn list_detail(empty_title: impl Into<String>) -> Self {
        Self::new(BigCollectionMode::ListDetail, empty_title)
    }

    /// Toggle the search entry.
    #[must_use]
    pub fn searchable(mut self, enabled: bool) -> Self {
        self.searchable = enabled;
        self
    }

    /// Toggle drag-to-reorder.
    #[must_use]
    pub fn reorderable(mut self, enabled: bool) -> Self {
        self.reorderable = enabled;
        self
    }

    /// Toggle section grouping.
    #[must_use]
    pub fn grouped(mut self, enabled: bool) -> Self {
        self.grouped = enabled;
        self
    }

    /// Set the page size (number of items rendered per page).
    #[must_use]
    pub fn page_size(mut self, value: usize) -> Self {
        self.page_size = Some(value);
        self
    }

    /// Resolve the spec into widget-facing requirements.
    ///
    /// The returned [`BigCollectionResolved`] is what a Relm4 factory or GTK
    /// view should inspect when deciding which sub-widgets to instantiate.
    #[must_use]
    pub fn resolved(&self) -> BigCollectionResolved {
        BigCollectionResolved {
            requires_factory: true,
            requires_virtualization: matches!(
                self.mode,
                BigCollectionMode::List | BigCollectionMode::Grid | BigCollectionMode::PreviewGrid
            ),
            requires_drag_handles: self.reorderable,
            requires_search_entry: self.searchable,
            layout_role: match self.mode {
                BigCollectionMode::List => "list",
                BigCollectionMode::Grid | BigCollectionMode::PreviewGrid => "grid",
                BigCollectionMode::Queue => "queue",
                BigCollectionMode::ListDetail => "list-detail",
            },
        }
    }
}

/// Widget-facing requirements derived from a [`BigCollectionSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigCollectionResolved {
    /// Whether items must be rendered through a Relm4/GTK factory.
    pub requires_factory: bool,
    /// Whether the view should virtualise rows for performance.
    pub requires_virtualization: bool,
    /// Whether reorder drag handles should be shown.
    pub requires_drag_handles: bool,
    /// Whether a search entry should be mounted in the headerbar.
    pub requires_search_entry: bool,
    /// Stable identifier of the layout role (`list`, `grid`, `queue`,
    /// `list-detail`).
    pub layout_role: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_grid_uses_virtualized_grid_role() {
        let resolved =
            BigCollectionSpec::new(BigCollectionMode::PreviewGrid, "No files").resolved();
        assert!(resolved.requires_factory);
        assert!(resolved.requires_virtualization);
        assert_eq!(resolved.layout_role, "grid");
    }

    #[test]
    fn reorderable_queue_requires_drag_handles() {
        let resolved = BigCollectionSpec::new(BigCollectionMode::Queue, "Empty")
            .reorderable(true)
            .resolved();
        assert!(resolved.requires_drag_handles);
        assert_eq!(resolved.layout_role, "queue");
    }
}
