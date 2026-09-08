//! BigLinux Relm4 component crate.
//!
//! This crate hosts reusable [Relm4](https://relm4.org) + libadwaita components
//! shared by BigLinux applications (`big-video-player`, `big-audio-player`,
//! `big-video-converter`, `big-audio-converter`, etc.).
//!
//! New components must pass the upstream duplication gate documented in
//! `COMPONENT_POLICY.md`: before adding a local module, confirm it is not
//! provided by upstream `relm4-components` (see [`UPSTREAM_RELM4_COMPONENTS`]).
//! The wider Relm4 ecosystem adoption gate is documented in
//! `docs/upstream-relm4-ecosystem.md` (see
//! [`UPSTREAM_RELM4_ECOSYSTEM_CRATES`]).
//!
//! # Module map
//!
//! - [`a11y`] — row/card accessibility helpers.
//! - [`app_shell`] — the `run` bootstrap orchestrator for whole apps.
//! - [`display`] — read-only views (file browser, status page, etc.).
//! - [`feedback`] — popovers, tooltips, badges.
//! - [`input`] — entries, sliders, dropdowns, shortcut capture.
//! - [`layout`] — multi-pane layouts, preferences dialogs, tab strips.
//! - [`list`] — `AdwActionRow` derivatives.
//! - [`pane`] — the `PaneContent` keystone: one mountable content unit, three
//!   mount points, arrangeable in a split (unified-shell restructure).
//! - [`state`] — empty / loading / error / retry / skeleton states.
//! - [`theme`] — theme helpers shared across apps.
//! - [`time`] — pure duration/time formatting helpers.

#![deny(missing_docs)]

pub mod a11y;
pub mod app_shell;
pub mod diagnostics;
pub mod display;
pub mod feedback;
pub mod file_io;
pub mod i18n;
pub mod input;
pub mod layout;
pub mod list;
pub mod pane;
pub mod resources;
pub mod state;
pub mod task;
pub mod text;
pub mod theme;
pub mod time;

/// Upstream modules that must be checked before adding a local equivalent.
pub const UPSTREAM_RELM4_COMPONENTS: &[&str] = &[
    "alert",
    "open_dialog",
    "open_button",
    "save_dialog",
    "simple_adw_combo_row",
    "simple_combo_box",
    "web_image",
];

/// Upstream Relm4 ecosystem crates that must be considered before adding
/// local framework code or new dependencies.
pub const UPSTREAM_RELM4_ECOSYSTEM_CRATES: &[&str] = &[
    "relm4",
    "relm4-components",
    "relm4-css",
    "relm4-icons",
    "relm4-icons-build",
    "relm4-macros",
];

/// Shared repository URL used by BigLinux apps.
pub const REPOSITORY: &str = "https://github.com/biglinux/big-rust-components";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_duplication_gate_lists_known_relm4_components() {
        assert!(UPSTREAM_RELM4_COMPONENTS.contains(&"alert"));
        assert!(UPSTREAM_RELM4_COMPONENTS.contains(&"open_dialog"));
        assert!(UPSTREAM_RELM4_COMPONENTS.contains(&"save_dialog"));
        assert!(UPSTREAM_RELM4_COMPONENTS.contains(&"simple_adw_combo_row"));
    }

    #[test]
    fn upstream_ecosystem_gate_lists_relm4_support_crates() {
        assert!(UPSTREAM_RELM4_ECOSYSTEM_CRATES.contains(&"relm4-components"));
        assert!(UPSTREAM_RELM4_ECOSYSTEM_CRATES.contains(&"relm4-icons"));
        assert!(UPSTREAM_RELM4_ECOSYSTEM_CRATES.contains(&"relm4-icons-build"));
        assert!(UPSTREAM_RELM4_ECOSYSTEM_CRATES.contains(&"relm4-css"));
        assert!(UPSTREAM_RELM4_ECOSYSTEM_CRATES.contains(&"relm4-macros"));
    }
}
