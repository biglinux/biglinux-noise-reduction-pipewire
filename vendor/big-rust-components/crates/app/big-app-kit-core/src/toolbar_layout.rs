// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! GTK-free toolbar layout model for apps with user-customizable tool strips.
//!
//! Apps provide string tool ids and a catalog describing canonical order,
//! default placement, non-user-configurable controls, and legacy migrations.
//! The model owns JSON normalization and ordering rules while UI crates keep
//! labels, widgets, and orientation-specific rendering.

use serde_json::{Value, json};

/// Current BigLinux toolbar-layout schema version.
///
/// This matches the file-manager layout schema that introduced the compact
/// single filter control and native view-mode menu.
pub const BIG_TOOLBAR_LAYOUT_VERSION: u64 = 7;

/// One toolbar edge in the in-memory model.
///
/// JSON compatibility keeps the historic field names `left` and `right`; the
/// Rust API uses `Start` and `End` so apps can map them to localized UI terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigToolbarEdge {
    /// Top toolbar.
    Top,
    /// Bottom toolbar.
    Bottom,
    /// Leading side toolbar, serialized as `left`.
    Start,
    /// Trailing side toolbar, serialized as `right`.
    End,
    /// Hidden controls.
    Hidden,
}

impl BigToolbarEdge {
    /// All edges in serialized/display order.
    pub const ALL: [BigToolbarEdge; 5] = [
        BigToolbarEdge::Top,
        BigToolbarEdge::Bottom,
        BigToolbarEdge::Start,
        BigToolbarEdge::End,
        BigToolbarEdge::Hidden,
    ];

    /// Whether this edge is horizontal.
    #[must_use]
    pub fn is_horizontal(self) -> bool {
        matches!(self, BigToolbarEdge::Top | BigToolbarEdge::Bottom)
    }

    fn json_key(self) -> &'static str {
        match self {
            BigToolbarEdge::Top => "top",
            BigToolbarEdge::Bottom => "bottom",
            BigToolbarEdge::Start => "left",
            BigToolbarEdge::End => "right",
            BigToolbarEdge::Hidden => "hidden",
        }
    }
}

/// Versioned migration ids for the shared toolbar-layout model.
///
/// The fields are generic string ids, but the version thresholds intentionally
/// mirror the file-manager migration chain so existing `version: 1..7` layouts
/// normalize exactly as before when the file-manager catalog is supplied.
#[derive(Debug, Clone, Copy)]
pub struct BigToolbarMigrations<'a> {
    /// Controls hidden when loading pre-v2 layouts.
    pub pre_v2_hide: &'a [&'a str],
    /// Control moved to the top edge when loading pre-v3 layouts.
    pub pre_v3_top: Option<&'a str>,
    /// Controls restored to the top edge from hidden on pre-v4 layouts.
    pub pre_v4_restore_top_if_hidden: &'a [&'a str],
    /// Controls hidden when loading pre-v5 layouts.
    pub pre_v5_hide: &'a [&'a str],
    /// Control restored to the top edge when loading pre-v2 layouts that hid it.
    pub pre_v2_restore_top_if_hidden: Option<&'a str>,
    /// Controls that must be visible on the top edge when normalization hid or
    /// omitted them.
    pub required_top_if_hidden: &'a [&'a str],
}

/// Static app catalog used to normalize a toolbar layout.
#[derive(Debug, Clone, Copy)]
pub struct BigToolbarCatalog<'a> {
    /// Schema version written by [`BigToolbarLayout::to_value`].
    pub version: u64,
    /// Known toolbar ids in canonical order.
    pub known: &'a [&'a str],
    /// Controls that users may not place in visible toolbars.
    pub locked_hidden: &'a [&'a str],
    /// Default top-edge ids for a fresh layout.
    pub default_top: &'a [&'a str],
    /// Default bottom-edge ids for a fresh layout.
    pub default_bottom: &'a [&'a str],
    /// Default leading-side ids for a fresh layout.
    pub default_start: &'a [&'a str],
    /// Default trailing-side ids for a fresh layout.
    pub default_end: &'a [&'a str],
    /// Default hidden ids for a fresh layout.
    pub default_hidden: &'a [&'a str],
    /// Edge that receives known ids missing from a saved layout.
    ///
    /// File manager uses [`BigToolbarEdge::Bottom`] for byte-compatible legacy
    /// normalization. New consumers can choose the edge that matches their
    /// own saved-layout contract.
    pub append_missing_to: BigToolbarEdge,
    /// Versioned migration rules.
    pub migrations: BigToolbarMigrations<'a>,
}

impl<'a> BigToolbarCatalog<'a> {
    /// File-manager-compatible catalog constructor for the current schema.
    #[must_use]
    pub const fn file_manager(
        known: &'a [&'a str],
        locked_hidden: &'a [&'a str],
        default_top: &'a [&'a str],
        default_bottom: &'a [&'a str],
        default_start: &'a [&'a str],
        default_end: &'a [&'a str],
        default_hidden: &'a [&'a str],
        migrations: BigToolbarMigrations<'a>,
    ) -> Self {
        Self {
            version: BIG_TOOLBAR_LAYOUT_VERSION,
            known,
            locked_hidden,
            default_top,
            default_bottom,
            default_start,
            default_end,
            default_hidden,
            append_missing_to: BigToolbarEdge::Bottom,
            migrations,
        }
    }

    fn is_known(self, id: &str) -> bool {
        self.known.contains(&id)
    }

    fn is_locked_hidden(self, id: &str) -> bool {
        self.locked_hidden.contains(&id)
    }
}

/// Per-edge ordered toolbar ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigToolbarLayout {
    top: Vec<String>,
    bottom: Vec<String>,
    start: Vec<String>,
    end: Vec<String>,
    hidden: Vec<String>,
}

impl BigToolbarLayout {
    /// Build the catalog's fresh default layout.
    #[must_use]
    pub fn default_for(catalog: BigToolbarCatalog<'_>) -> Self {
        let mut layout = Self {
            top: clone_ids(catalog.default_top),
            bottom: clone_ids(catalog.default_bottom),
            start: clone_ids(catalog.default_start),
            end: clone_ids(catalog.default_end),
            hidden: clone_ids(catalog.default_hidden),
        };
        layout.normalize(catalog);
        layout
    }

    /// Decode a saved JSON value and normalize it against `catalog`.
    #[must_use]
    pub fn from_value(value: &Value, catalog: BigToolbarCatalog<'_>) -> Self {
        let mut layout = Self {
            top: read_edge(value, catalog, BigToolbarEdge::Top),
            bottom: read_edge(value, catalog, BigToolbarEdge::Bottom),
            start: read_edge(value, catalog, BigToolbarEdge::Start),
            end: read_edge(value, catalog, BigToolbarEdge::End),
            hidden: read_edge(value, catalog, BigToolbarEdge::Hidden),
        };
        layout.normalize(catalog);
        layout.apply_migrations(
            value.get("version").and_then(Value::as_u64).unwrap_or(0),
            catalog,
        );
        layout
    }

    /// Decode a saved JSON string and normalize it against `catalog`.
    ///
    /// Invalid JSON falls back to the catalog default.
    #[must_use]
    pub fn from_json_str(raw: &str, catalog: BigToolbarCatalog<'_>) -> Self {
        serde_json::from_str::<Value>(raw)
            .map(|value| Self::from_value(&value, catalog))
            .unwrap_or_else(|_| Self::default_for(catalog))
    }

    /// Serialize with the catalog schema version and historic JSON field names.
    #[must_use]
    pub fn to_value(&self, catalog: BigToolbarCatalog<'_>) -> Value {
        json!({
            "version": catalog.version,
            "top": self.top,
            "bottom": self.bottom,
            "left": self.start,
            "right": self.end,
            "hidden": self.hidden,
        })
    }

    /// Serialize as a compact JSON string for string-only preference stores.
    ///
    /// # Errors
    ///
    /// Returns a serde error only if the layout cannot be serialized.
    pub fn to_json_string(
        &self,
        catalog: BigToolbarCatalog<'_>,
    ) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.to_value(catalog))
    }

    /// Items on `edge`, in display order.
    #[must_use]
    pub fn items_on(&self, edge: BigToolbarEdge) -> &[String] {
        self.list(edge)
    }

    /// Which edge currently contains `id`.
    #[must_use]
    pub fn edge_of(&self, id: &str) -> BigToolbarEdge {
        BigToolbarEdge::ALL
            .into_iter()
            .find(|&edge| self.list(edge).iter().any(|item| item == id))
            .unwrap_or(BigToolbarEdge::Hidden)
    }

    /// Move `id` to the end of `edge`, honoring catalog locked-hidden controls.
    pub fn set_edge(&mut self, catalog: BigToolbarCatalog<'_>, id: &str, edge: BigToolbarEdge) {
        if !catalog.is_known(id) {
            return;
        }
        let edge = if catalog.is_locked_hidden(id) {
            BigToolbarEdge::Hidden
        } else {
            edge
        };
        if self.edge_of(id) == edge {
            return;
        }
        self.remove_id(id);
        self.list_mut(edge).push(id.to_owned());
    }

    /// Swap `id` with its previous (`up`) or next neighbor on the same edge.
    pub fn reorder(&mut self, id: &str, up: bool) {
        let edge = self.edge_of(id);
        let list = self.list_mut(edge);
        let Some(pos) = list.iter().position(|item| item == id) else {
            return;
        };
        let target = if up {
            pos.checked_sub(1)
        } else if pos + 1 < list.len() {
            Some(pos + 1)
        } else {
            None
        };
        if let Some(target) = target {
            list.swap(pos, target);
        }
    }

    /// Normalize a layout in place: drop unknown ids, dedupe by first
    /// occurrence, append missing known ids, and force locked controls hidden.
    pub fn normalize(&mut self, catalog: BigToolbarCatalog<'_>) {
        let mut seen: Vec<String> = Vec::new();
        for edge in BigToolbarEdge::ALL {
            self.list_mut(edge).retain(|id| {
                if !catalog.is_known(id) || seen.iter().any(|seen_id| seen_id == id) {
                    false
                } else {
                    seen.push(id.clone());
                    true
                }
            });
        }
        for id in catalog.known {
            if !seen.iter().any(|seen_id| seen_id == id) {
                self.list_mut(catalog.append_missing_to)
                    .push((*id).to_owned());
            }
        }
        for id in catalog.locked_hidden {
            self.set_edge(catalog, id, BigToolbarEdge::Hidden);
        }
    }

    fn apply_migrations(&mut self, version: u64, catalog: BigToolbarCatalog<'_>) {
        if version < 2 {
            for id in catalog.migrations.pre_v2_hide {
                self.set_edge(catalog, id, BigToolbarEdge::Hidden);
            }
            if let Some(id) = catalog.migrations.pre_v2_restore_top_if_hidden
                && self.edge_of(id) == BigToolbarEdge::Hidden
            {
                self.set_edge(catalog, id, BigToolbarEdge::Top);
            }
        }
        if version < 3
            && let Some(id) = catalog.migrations.pre_v3_top
        {
            self.set_edge(catalog, id, BigToolbarEdge::Top);
        }
        if version < 4 {
            for id in catalog.migrations.pre_v4_restore_top_if_hidden {
                if self.edge_of(id) == BigToolbarEdge::Hidden {
                    self.set_edge(catalog, id, BigToolbarEdge::Top);
                }
            }
        }
        if version < 5 {
            for id in catalog.migrations.pre_v5_hide {
                self.set_edge(catalog, id, BigToolbarEdge::Hidden);
            }
        }
        for id in catalog.locked_hidden {
            self.set_edge(catalog, id, BigToolbarEdge::Hidden);
        }
        for id in catalog.migrations.required_top_if_hidden {
            if self.edge_of(id) == BigToolbarEdge::Hidden {
                self.set_edge(catalog, id, BigToolbarEdge::Top);
            }
        }
    }

    fn list(&self, edge: BigToolbarEdge) -> &Vec<String> {
        match edge {
            BigToolbarEdge::Top => &self.top,
            BigToolbarEdge::Bottom => &self.bottom,
            BigToolbarEdge::Start => &self.start,
            BigToolbarEdge::End => &self.end,
            BigToolbarEdge::Hidden => &self.hidden,
        }
    }

    fn list_mut(&mut self, edge: BigToolbarEdge) -> &mut Vec<String> {
        match edge {
            BigToolbarEdge::Top => &mut self.top,
            BigToolbarEdge::Bottom => &mut self.bottom,
            BigToolbarEdge::Start => &mut self.start,
            BigToolbarEdge::End => &mut self.end,
            BigToolbarEdge::Hidden => &mut self.hidden,
        }
    }

    fn remove_id(&mut self, id: &str) {
        for edge in BigToolbarEdge::ALL {
            self.list_mut(edge).retain(|item| item != id);
        }
    }
}

fn clone_ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn read_edge(value: &Value, catalog: BigToolbarCatalog<'_>, edge: BigToolbarEdge) -> Vec<String> {
    value
        .get(edge.json_key())
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .filter(|id| catalog.is_known(id))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: &[&str] = &[
        "position",
        "refresh",
        "hidden",
        "actions",
        "view-mode",
        "sort",
        "preview",
        "breadcrumb",
        "upload",
        "history",
        "search",
        "recursive-search",
        "search-options",
        "count",
        "free-space",
        "disk",
        "zoom",
    ];
    const LOCKED: &[&str] = &["refresh", "recursive-search", "search-options"];
    const DEFAULT_TOP: &[&str] = &[
        "breadcrumb",
        "actions",
        "view-mode",
        "sort",
        "hidden",
        "preview",
        "search",
    ];
    const DEFAULT_BOTTOM: &[&str] = &["zoom", "disk", "free-space"];
    const DEFAULT_HIDDEN: &[&str] = &[
        "position",
        "count",
        "history",
        "upload",
        "refresh",
        "recursive-search",
        "search-options",
    ];
    const V2_HIDE: &[&str] = &[
        "refresh",
        "view-mode",
        "sort",
        "hidden",
        "preview",
        "history",
        "position",
        "count",
    ];
    const V4_RESTORE: &[&str] = &["view-mode", "sort", "hidden", "preview"];

    fn catalog() -> BigToolbarCatalog<'static> {
        BigToolbarCatalog::file_manager(
            KNOWN,
            LOCKED,
            DEFAULT_TOP,
            DEFAULT_BOTTOM,
            &[],
            &[],
            DEFAULT_HIDDEN,
            BigToolbarMigrations {
                pre_v2_hide: V2_HIDE,
                pre_v3_top: Some("actions"),
                pre_v4_restore_top_if_hidden: V4_RESTORE,
                pre_v5_hide: LOCKED,
                pre_v2_restore_top_if_hidden: Some("breadcrumb"),
                required_top_if_hidden: &["search", "view-mode"],
            },
        )
    }

    #[test]
    fn default_has_path_bar_on_top_and_status_footer_on_bottom() {
        let layout = BigToolbarLayout::default_for(catalog());

        assert_eq!(layout.edge_of("breadcrumb"), BigToolbarEdge::Top);
        assert_eq!(layout.items_on(BigToolbarEdge::Top), DEFAULT_TOP);
        assert_eq!(layout.items_on(BigToolbarEdge::Bottom), DEFAULT_BOTTOM);
        assert_eq!(layout.items_on(BigToolbarEdge::Hidden), DEFAULT_HIDDEN);
    }

    #[test]
    fn set_edge_moves_and_dedups() {
        let mut layout = BigToolbarLayout::default_for(catalog());

        assert_eq!(layout.edge_of("zoom"), BigToolbarEdge::Bottom);
        layout.set_edge(catalog(), "zoom", BigToolbarEdge::Start);

        assert_eq!(layout.edge_of("zoom"), BigToolbarEdge::Start);
        assert!(
            !layout
                .items_on(BigToolbarEdge::Bottom)
                .contains(&"zoom".to_owned())
        );
        assert_eq!(layout.items_on(BigToolbarEdge::Start), ["zoom"]);
    }

    #[test]
    fn reorder_swaps_within_edge() {
        let mut layout = BigToolbarLayout::default_for(catalog());

        layout.reorder("actions", true);

        assert_eq!(layout.items_on(BigToolbarEdge::Top)[0], "actions");
        assert_eq!(layout.items_on(BigToolbarEdge::Top)[1], "breadcrumb");
    }

    #[test]
    fn roundtrip_through_value_preserves_layout() {
        let mut layout = BigToolbarLayout::default_for(catalog());
        layout.set_edge(catalog(), "zoom", BigToolbarEdge::Start);
        layout.set_edge(catalog(), "disk", BigToolbarEdge::Hidden);

        let restored = BigToolbarLayout::from_value(&layout.to_value(catalog()), catalog());

        assert_eq!(restored.edge_of("zoom"), BigToolbarEdge::Start);
        assert_eq!(restored.edge_of("disk"), BigToolbarEdge::Hidden);
        assert_eq!(
            restored.items_on(BigToolbarEdge::Bottom),
            layout.items_on(BigToolbarEdge::Bottom)
        );
    }

    #[test]
    fn load_appends_items_missing_from_saved_layout() {
        let layout = BigToolbarLayout::from_value(&json!({ "bottom": ["search"] }), catalog());
        let mut all: Vec<String> = Vec::new();

        for edge in BigToolbarEdge::ALL {
            all.extend(layout.items_on(edge).iter().cloned());
        }

        for id in KNOWN {
            assert!(all.iter().any(|candidate| candidate == id), "missing {id}");
        }
    }

    #[test]
    fn legacy_layouts_do_not_reintroduce_toolbar_refresh() {
        let layout = BigToolbarLayout::from_value(
            &json!({
                "version": 5,
                "top": ["breadcrumb", "refresh", "recursive-search", "search-options", "search"],
                "bottom": ["zoom", "disk", "free-space"],
                "hidden": ["actions", "view-mode", "sort", "hidden", "preview"]
            }),
            catalog(),
        );

        assert_eq!(layout.edge_of("refresh"), BigToolbarEdge::Hidden);
        assert_eq!(layout.edge_of("recursive-search"), BigToolbarEdge::Hidden);
        assert_eq!(layout.edge_of("search-options"), BigToolbarEdge::Hidden);
        assert_eq!(layout.edge_of("search"), BigToolbarEdge::Top);
    }

    #[test]
    fn current_saved_layouts_still_obey_single_filter_contract() {
        let layout = BigToolbarLayout::from_value(
            &json!({
                "version": 6,
                "top": ["breadcrumb", "search-options", "recursive-search"],
                "bottom": ["zoom", "disk", "free-space"],
                "hidden": ["search", "refresh", "actions", "view-mode", "sort", "hidden", "preview"]
            }),
            catalog(),
        );

        assert_eq!(layout.edge_of("refresh"), BigToolbarEdge::Hidden);
        assert_eq!(layout.edge_of("recursive-search"), BigToolbarEdge::Hidden);
        assert_eq!(layout.edge_of("search-options"), BigToolbarEdge::Hidden);
        assert_eq!(layout.edge_of("search"), BigToolbarEdge::Top);
    }

    #[test]
    fn current_layout_keeps_one_view_menu_and_one_filter_entry() {
        let layout = BigToolbarLayout::from_value(
            &json!({
                "version": 7,
                "top": ["breadcrumb", "refresh", "search-options", "recursive-search"],
                "bottom": ["zoom", "disk", "free-space"],
                "hidden": ["search", "view-mode", "actions", "sort", "hidden", "preview"]
            }),
            catalog(),
        );

        assert_eq!(layout.edge_of("refresh"), BigToolbarEdge::Hidden);
        assert_eq!(layout.edge_of("recursive-search"), BigToolbarEdge::Hidden);
        assert_eq!(layout.edge_of("search-options"), BigToolbarEdge::Hidden);
        assert_eq!(layout.edge_of("search"), BigToolbarEdge::Top);
        assert_eq!(layout.edge_of("view-mode"), BigToolbarEdge::Top);
    }

    #[test]
    fn product_managed_controls_cannot_be_readded_to_toolbar() {
        let mut layout = BigToolbarLayout::default_for(catalog());

        for id in LOCKED {
            layout.set_edge(catalog(), id, BigToolbarEdge::Top);
            assert_eq!(layout.edge_of(id), BigToolbarEdge::Hidden);
        }
    }

    #[test]
    fn roundtrip_is_byte_compatible_with_captured_file_manager_json() {
        let value = json!({
            "version": 7,
            "top": ["breadcrumb", "actions", "view-mode", "sort", "hidden", "preview", "search"],
            "bottom": ["zoom", "disk", "free-space"],
            "left": [],
            "right": [],
            "hidden": ["position", "count", "history", "upload", "refresh", "recursive-search", "search-options"]
        });
        let layout = BigToolbarLayout::from_value(&value, catalog());

        assert_eq!(layout.to_value(catalog()), value);
    }
}
