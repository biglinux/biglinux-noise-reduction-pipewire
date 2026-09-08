// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Persisted sidebar geometry shell for workspace windows.
//!
//! Apps own sidebar rows, sections, selection, and actions. This shell owns
//! only side, visibility, width bounds, accessibility metadata, and persistence.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;

use crate::i18n;
use crate::input::preference_rows::BigPreferenceStore;
use crate::layout::workspace_sidebar_split::{
    BigWorkspaceSidebarSplit, BigWorkspaceSidebarSplitSpec,
};

/// Side where the sidebar is mounted relative to the main content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BigSidebarSide {
    /// Sidebar is on the leading edge.
    #[default]
    Start,
    /// Sidebar is on the trailing edge.
    End,
}

impl BigSidebarSide {
    /// Parse the stable persisted value, falling back to `default_side`.
    #[must_use]
    pub fn from_persisted_value(value: &str, default_side: Self) -> Self {
        match value {
            "start" => Self::Start,
            "end" => Self::End,
            _ => default_side,
        }
    }

    /// Stable lowercase value for settings.
    #[must_use]
    pub fn as_persisted_value(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }

    fn pack_type(self) -> gtk::PackType {
        match self {
            Self::Start => gtk::PackType::Start,
            Self::End => gtk::PackType::End,
        }
    }
}

/// Display and persistence contract for [`BigSidebarShell`].
#[derive(Debug, Clone)]
pub struct BigSidebarShellSpec {
    /// Settings key prefix. `sidebar` derives `sidebar_side`,
    /// `sidebar_visible`, and `sidebar_width`.
    pub setting_prefix: &'static str,
    /// Fallback side when the setting is unset or unknown.
    pub default_side: BigSidebarSide,
    /// Fallback visibility when the setting is unset.
    pub default_visible: bool,
    /// Minimum persisted sidebar width.
    pub min_width: i32,
    /// Maximum persisted sidebar width.
    pub max_width: i32,
    /// Component-owned accessible-name msgid for the split root.
    pub accessible_name: &'static str,
    /// Pre-translated accessible name from the app's own textdomain; a
    /// non-empty value overrides [`Self::accessible_name`].
    pub accessible_name_override: Option<String>,
    /// CSS classes added to the split root.
    pub css_classes: &'static [&'static str],
}

impl BigSidebarShellSpec {
    /// Derive the three persisted keys from [`Self::setting_prefix`].
    #[must_use]
    pub fn setting_keys(&self) -> BigSidebarShellSettingKeys {
        BigSidebarShellSettingKeys::from_prefix(self.setting_prefix)
    }

    fn resolved(self, store: &dyn BigPreferenceStore) -> BigSidebarShellResolved {
        let keys = self.setting_keys();
        let min_width = self.min_width.max(1);
        let max_width = self.max_width.max(min_width);
        let width = store
            .get_i64(keys.width.as_str())
            .and_then(|width| i32::try_from(width).ok())
            .map_or(min_width, |width| {
                clamp_sidebar_width(width, min_width, max_width)
            });
        let side = store
            .get_string(keys.side.as_str())
            .map_or(self.default_side, |value| {
                BigSidebarSide::from_persisted_value(&value, self.default_side)
            });
        let is_visible = store
            .get_bool(keys.visible.as_str())
            .unwrap_or(self.default_visible);
        BigSidebarShellResolved {
            keys: keys.into_static(),
            side,
            is_visible,
            width,
            min_width,
            max_width,
            accessible_name: self
                .accessible_name_override
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map_or_else(|| i18n::t(self.accessible_name), str::to_owned),
            css_classes: self.css_classes,
        }
    }
}

/// Derived persisted keys for a [`BigSidebarShellSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSidebarShellSettingKeys {
    /// Key storing `start` or `end`.
    pub side: String,
    /// Key storing sidebar visibility.
    pub visible: String,
    /// Key storing sidebar width in logical pixels.
    pub width: String,
}

impl BigSidebarShellSettingKeys {
    /// Derive `*_side`, `*_visible`, and `*_width` keys from `prefix`.
    #[must_use]
    pub fn from_prefix(prefix: &str) -> Self {
        Self {
            side: format!("{prefix}_side"),
            visible: format!("{prefix}_visible"),
            width: format!("{prefix}_width"),
        }
    }

    // Keys are `format!`-derived from `setting_prefix`, but the store's
    // `set_*(key: &'static str, ..)` parameter requires `'static`. Leak once
    // per shell (bounded by window count, not user interaction) to satisfy it.
    fn into_static(self) -> BigSidebarShellStaticKeys {
        BigSidebarShellStaticKeys {
            side: Box::leak(self.side.into_boxed_str()),
            visible: Box::leak(self.visible.into_boxed_str()),
            width: Box::leak(self.width.into_boxed_str()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BigSidebarShellStaticKeys {
    side: &'static str,
    visible: &'static str,
    width: &'static str,
}

struct BigSidebarShellResolved {
    keys: BigSidebarShellStaticKeys,
    side: BigSidebarSide,
    is_visible: bool,
    width: i32,
    min_width: i32,
    max_width: i32,
    accessible_name: String,
    css_classes: &'static [&'static str],
}

/// Persisted `AdwOverlaySplitView` shell for an app-owned sidebar.
#[derive(Clone)]
pub struct BigSidebarShell {
    split: BigWorkspaceSidebarSplit,
    sidebar_content_slot: gtk::Box,
    store: Rc<dyn BigPreferenceStore>,
    keys: BigSidebarShellStaticKeys,
    default_side: BigSidebarSide,
    default_visible: bool,
    default_width: i32,
    min_width: i32,
    max_width: i32,
    side: Cell<BigSidebarSide>,
    is_visible: Cell<bool>,
    width: Cell<i32>,
}

impl BigSidebarShell {
    /// Build the shell around `content` and an initially empty sidebar slot.
    #[must_use]
    pub fn new(
        content: &impl IsA<gtk::Widget>,
        spec: BigSidebarShellSpec,
        store: Rc<dyn BigPreferenceStore>,
    ) -> Self {
        let resolved = spec.clone().resolved(store.as_ref());
        let sidebar_content_slot = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .build();
        let split_spec = BigWorkspaceSidebarSplitSpec::new()
            .min_sidebar_width(f64::from(resolved.min_width))
            .max_sidebar_width(f64::from(resolved.width))
            .shows_sidebar_initially(resolved.is_visible)
            // Docked persistent sidebar: never start collapsed, or the split
            // renders content-only and the sidebar exists only as a hidden
            // overlay.
            .is_collapsed_initially(false)
            .accessible_name(resolved.accessible_name);
        let split_spec = resolved
            .css_classes
            .iter()
            .fold(split_spec, |spec, css_class| spec.css_class(*css_class));
        let split = BigWorkspaceSidebarSplit::new(&sidebar_content_slot, content, split_spec);
        split
            .split_view()
            .set_sidebar_position(resolved.side.pack_type());

        Self {
            split,
            sidebar_content_slot,
            store,
            keys: resolved.keys,
            default_side: spec.default_side,
            default_visible: spec.default_visible,
            default_width: resolved.width,
            min_width: resolved.min_width,
            max_width: resolved.max_width,
            side: Cell::new(resolved.side),
            is_visible: Cell::new(resolved.is_visible),
            width: Cell::new(resolved.width),
        }
    }

    /// Borrow the underlying split view.
    #[must_use]
    pub fn split_view(&self) -> &adw::OverlaySplitView {
        self.split.split_view()
    }

    /// Sidebar content slot owned by the shell; apps append their sidebar UI.
    #[must_use]
    pub fn sidebar_content_slot(&self) -> &gtk::Box {
        &self.sidebar_content_slot
    }

    /// Current sidebar side.
    #[must_use]
    pub fn side(&self) -> BigSidebarSide {
        self.side.get()
    }

    /// Current sidebar visibility.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.is_visible.get()
    }

    /// Current clamped sidebar width.
    #[must_use]
    pub fn width(&self) -> i32 {
        self.width.get()
    }

    /// Apply and persist a sidebar side.
    pub fn apply_side(&self, side: BigSidebarSide) {
        self.apply_side_to_widget(side);
        self.store
            .set_string(self.keys.side, side.as_persisted_value());
    }

    /// Apply and persist sidebar visibility.
    pub fn set_visible(&self, is_visible: bool) {
        self.apply_visible_to_widget(is_visible);
        self.store.set_bool(self.keys.visible, is_visible);
    }

    /// Apply and persist sidebar width after clamping it to the spec bounds.
    pub fn set_width(&self, width: i32) {
        let width = clamp_sidebar_width(width, self.min_width, self.max_width);
        self.apply_width_to_widget(width);
        self.store.set_i64(self.keys.width, i64::from(width));
    }

    /// Refresh side, visibility, and width from the store without writing them
    /// back. Use from settings-listener callbacks.
    pub fn apply_from_store(&self) {
        let side = self
            .store
            .get_string(self.keys.side)
            .map_or(self.default_side, |value| {
                BigSidebarSide::from_persisted_value(&value, self.default_side)
            });
        let is_visible = self
            .store
            .get_bool(self.keys.visible)
            .unwrap_or(self.default_visible);
        let width = self
            .store
            .get_i64(self.keys.width)
            .and_then(|width| i32::try_from(width).ok())
            .map_or(self.default_width, |width| {
                clamp_sidebar_width(width, self.min_width, self.max_width)
            });
        self.apply_side_to_widget(side);
        self.apply_visible_to_widget(is_visible);
        self.apply_width_to_widget(width);
    }

    fn apply_side_to_widget(&self, side: BigSidebarSide) {
        self.side.set(side);
        self.split_view().set_sidebar_position(side.pack_type());
    }

    fn apply_visible_to_widget(&self, is_visible: bool) {
        self.is_visible.set(is_visible);
        self.split_view().set_show_sidebar(is_visible);
    }

    fn apply_width_to_widget(&self, width: i32) {
        self.width.set(width);
        self.split_view().set_max_sidebar_width(f64::from(width));
    }
}

fn clamp_sidebar_width(width: i32, min_width: i32, max_width: i32) -> i32 {
    width.clamp(min_width, max_width.max(min_width))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_setting_keys_from_prefix() {
        let keys = BigSidebarShellSettingKeys::from_prefix("sidebar");

        assert_eq!(keys.side, "sidebar_side");
        assert_eq!(keys.visible, "sidebar_visible");
        assert_eq!(keys.width, "sidebar_width");
    }

    #[test]
    fn clamps_width_to_sanitized_bounds() {
        assert_eq!(clamp_sidebar_width(120, 200, 480), 200);
        assert_eq!(clamp_sidebar_width(320, 200, 480), 320);
        assert_eq!(clamp_sidebar_width(900, 200, 480), 480);
        assert_eq!(clamp_sidebar_width(120, 200, 100), 200);
    }

    #[test]
    fn side_parse_falls_back_to_default() {
        assert_eq!(
            BigSidebarSide::from_persisted_value("end", BigSidebarSide::Start),
            BigSidebarSide::End
        );
        assert_eq!(
            BigSidebarSide::from_persisted_value("unknown", BigSidebarSide::End),
            BigSidebarSide::End
        );
    }
}
