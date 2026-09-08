// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]
#![forbid(unsafe_code)]

//! Agent-friendly app contracts for BigLinux Rust desktop apps.
//!
//! The crate keeps app code small by exposing typed specs for common desktop
//! workflows. Specs are display-free and testable; Relm4/GTK widgets can wrap
//! them without moving app domain logic into shared UI crates.
//!
//! # Layout
//!
//! Modules are grouped by responsibility:
//!
//! - Data + persistence: [`storage`], [`recent_files`], [`profile_registry`],
//!   [`keyring`], [`scheme_loader`].
//! - Desktop integration: [`actions`], [`desktop`], [`shell`], [`subprocess`], [`url`],
//!   [`services`].
//! - User interface contracts: [`dialogs`], [`file_dialogs`], [`forms`],
//!   [`previews`], [`widgets`], [`catalog`], [`collections`],
//!   [`toolbar_layout`].
//! - Media + remote: [`mpris`], `remote_control` (feature `remote-control`),
//!   `http_client` (feature `http-client`).
//! - AI workflow scaffolding: [`ai_assistant`], [`ai_history`],
//!   [`ai_provider_config`].
//! - Misc: [`files`], [`tasks`], [`keybindings`].
//!
/// Application and window action contracts.
pub mod actions;
/// Chat panel scaffolding for AI assistant integration.
pub mod ai_assistant;
/// GTK adapter primitives for the app bootstrap contract (`AppShellContext`,
/// `BigAppWindow`); the display-free spec lives in
/// `big_app_kit_core::app_shell` and the `run` orchestrator in
/// `big_relm4_components::app_shell`.
pub mod app_shell;
/// Component-fault containment for multi-app host processes (big-host).
pub mod containment;
/// Desktop integration: actions, accelerators, app menus.
pub mod desktop;
/// Generic dialog specs decoupled from any widget toolkit.
pub mod dialogs;
/// Open/save file picker contracts.
pub mod file_dialogs;
mod gtk_accessibility_policy;
/// Keyboard shortcut specs and rebind helpers.
pub mod keybindings;
/// MPRIS D-Bus service for media players.
pub mod mpris;
/// Debounced, atomic, listener-backed app settings document.
pub mod settings_store;
/// GTK widget helpers built on top of the kit's contracts.
pub mod widgets;
/// Compositor background-blur integration for BigLinux app windows.
pub mod window_blur;

// Display-free (GTK-free) contracts live in `big-app-kit-core` so non-GUI
// crates reuse them without the gtk4/libadwaita/relm4 graph. Re-exported here
// unchanged: `big_app_kit::<module>::…` keeps working. The gtk halves of
// `actions`, `dialogs`, and `keybindings` stay above and pull their specs from
// core internally.
#[cfg(feature = "http-client")]
#[doc(inline)]
pub use big_app_kit_core::http_client;
#[cfg(feature = "remote-control")]
#[doc(inline)]
pub use big_app_kit_core::remote_control;
#[doc(inline)]
pub use big_app_kit_core::{
    ai_history, ai_provider_config, catalog, collections, files, forms, previews, profile_registry,
    recent_files, scheme_loader, services, settings_keys, shell, tasks, toolbar_layout, url,
    window_shell, zone_layout,
};

// GTK-free OS/persistence/secret modules now live in `big-os-kit` so non-GUI
// crates can reuse them without the gtk4/libadwaita/relm4 graph. Re-exported
// here unchanged: `big_app_kit::{subprocess, storage, keyring}` keep working.
#[doc(inline)]
pub use big_os_kit::{keyring, storage, subprocess};

/// Repository URL used in generated docs/snippets.
pub const REPOSITORY: &str = "https://github.com/biglinux/big-rust-components";

/// Recommended dependency order for a typical BigLinux Rust desktop app.
///
/// Curated subset, not an exhaustive crate list: foundation crates that are
/// re-exported transitively (`big-ui-tokens`) and pure foundation crates
/// (`big-media-engines`, `big-media-data`, `big-os-kit`) are omitted.
/// See `ARCHITECTURE.md` / the workspace `Cargo.toml` for the full crate set.
pub const CRATE_LAYERS: &[&str] = &[
    "big-app-kit",
    "big-relm4-components",
    "big-media-components",
    "big-audio-effects",
    "big-mpv-render",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_layers_start_with_app_contracts() {
        assert_eq!(CRATE_LAYERS.first(), Some(&"big-app-kit"));
        assert!(CRATE_LAYERS.contains(&"big-relm4-components"));
        assert!(CRATE_LAYERS.contains(&"big-media-components"));
    }
}
