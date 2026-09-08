// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Machine-readable feature catalog constants.

/// Catalog entry describing one reusable BigLinux UI feature.
///
/// Used by docs, the readiness check, and `bigagents` tooling to map a
/// user-facing capability ("drop-target", "settings-page") back to the
/// crate/module that owns it. Lookups go through [`feature_by_id`] and
/// [`features_by_category`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigFeature {
    /// Dotted identifier (`"forms.settings-page"`) referenced from docs
    /// and tests.
    pub id: &'static str,
    /// Coarse grouping (`"app-shell"`, `"dialogs"`, ...) used to filter
    /// the catalog.
    pub category: &'static str,
    /// Cargo crate that provides the feature.
    pub crate_name: &'static str,
    /// Rust module path inside `crate_name` where the relevant types
    /// live.
    pub module: &'static str,
    /// Short prose explaining the situation in which app authors should
    /// reach for this feature.
    pub when: &'static str,
}

/// Authoritative list of catalog entries. Insertion order is the order
/// the docs site renders them in.
pub const FEATURES: &[BigFeature] = &[
    BigFeature {
        id: "shell.single-window",
        category: "app-shell",
        crate_name: "big-app-kit",
        module: "shell",
        when: "standard one-window desktop app",
    },
    BigFeature {
        id: "files.drop-target",
        category: "file-io",
        crate_name: "big-app-kit",
        module: "files",
        when: "app accepts dragged files, folders, or URLs",
    },
    BigFeature {
        id: "collections.preview-grid",
        category: "collections",
        crate_name: "big-app-kit",
        module: "collections",
        when: "files or media need thumbnail grid browsing",
    },
    BigFeature {
        id: "forms.settings-page",
        category: "forms-settings",
        crate_name: "big-app-kit",
        module: "forms",
        when: "screen edits persistent settings",
    },
    BigFeature {
        id: "dialogs.error",
        category: "dialogs",
        crate_name: "big-app-kit",
        module: "dialogs",
        when: "failure needs copyable diagnostics",
    },
    BigFeature {
        id: "previews.video",
        category: "preview-editors",
        crate_name: "big-media-components",
        module: "video_transport",
        when: "app previews or edits video",
    },
    BigFeature {
        id: "tasks.runner",
        category: "long-tasks",
        crate_name: "big-app-kit",
        module: "tasks",
        when: "work can block the UI thread",
    },
    BigFeature {
        id: "desktop.actions",
        category: "desktop-integration",
        crate_name: "big-app-kit",
        module: "desktop",
        when: "app exposes shortcuts, menus, notifications, or packaging metadata",
    },
    BigFeature {
        id: "desktop.system-power",
        category: "desktop-integration",
        crate_name: "big-app-kit",
        module: "desktop",
        when: "app needs suspend or poweroff actions from UI flow",
    },
];

/// Find a single feature by its dotted identifier. Returns `None` when no
/// catalog entry matches `id`; this is the path the `bigagents` readiness
/// audit takes when it complains about an unknown feature reference.
#[must_use]
pub fn feature_by_id(id: &str) -> Option<&'static BigFeature> {
    FEATURES.iter().find(|feature| feature.id == id)
}

/// Return every catalog entry whose `category` field matches the
/// argument, in catalog order. An unknown category yields an empty
/// vector; docs and tooling rely on that to enumerate categories without
/// hard-coding them.
#[must_use]
pub fn features_by_category(category: &str) -> Vec<&'static BigFeature> {
    FEATURES
        .iter()
        .filter(|feature| feature.category == category)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_all_initial_categories() {
        let categories = [
            "app-shell",
            "file-io",
            "collections",
            "forms-settings",
            "dialogs",
            "preview-editors",
            "long-tasks",
            "desktop-integration",
        ];
        for category in categories {
            assert!(
                FEATURES.iter().any(|feature| feature.category == category),
                "missing category {category}"
            );
        }
    }

    #[test]
    fn feature_lookup_uses_stable_ids() {
        let feature = feature_by_id("tasks.runner").expect("tasks runner feature");
        assert_eq!(feature.module, "tasks");
    }

    #[test]
    fn category_lookup_filters_and_preserves_catalog_order() {
        let desktop_features = features_by_category("desktop-integration");

        assert_eq!(
            desktop_features
                .iter()
                .map(|feature| feature.id)
                .collect::<Vec<_>>(),
            vec!["desktop.actions", "desktop.system-power"]
        );
        assert!(
            desktop_features
                .iter()
                .all(|feature| feature.category == "desktop-integration")
        );
        assert!(features_by_category("missing-category").is_empty());
    }
}
