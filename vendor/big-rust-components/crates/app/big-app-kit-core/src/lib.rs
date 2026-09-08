// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]
#![forbid(unsafe_code)]

//! Display-free app contracts for BigLinux Rust desktop apps.
//!
//! This is the GTK-free core of `big-app-kit`: every module here compiles
//! without gtk4/libadwaita/relm4/gdk so non-GUI crates (packaging tooling,
//! headless services, tests) can depend on the specs without the widget graph.
//! `big-app-kit` re-exports every item unchanged, so
//! `big_app_kit::<module>::…` paths keep working; the toolkit-bound halves of
//! [`actions`], [`dialogs`], and `keybindings` live in `big-app-kit`.

/// Application and window action contracts (specs; GIO installers live in
/// `big_app_kit::actions`).
pub mod actions;
/// Persisted assistant conversation history.
pub mod ai_history;
/// Provider/model config for AI integrations.
pub mod ai_provider_config;
/// Display-free application bootstrap contract (`AppShellSpec`); the GTK
/// adapter lives in `big_relm4_components::app_shell`.
pub mod app_shell;
/// Catalog of typed UI contracts (registry-style discovery).
pub mod catalog;
/// List/grid/queue collection contracts.
pub mod collections;
/// Generic dialog specs decoupled from any widget toolkit (adw builders live in
/// `big_app_kit::dialogs`).
pub mod dialogs;
/// Filesystem helpers (path normalisation, type detection).
pub mod files;
/// Form/settings field specs and validation hints.
pub mod forms;
/// Blocking HTTP client wrapper (feature `http-client`).
#[cfg(feature = "http-client")]
pub mod http_client;
/// Keyboard shortcut specs and rebind helpers (gdk keyval helpers live in
/// `big_app_kit::keybindings`).
pub mod keybindings;
/// Preview pane contracts for media/file viewers.
pub mod previews;
/// Persistent app/user profile registry.
pub mod profile_registry;
/// Recent-files tracking, persisted between sessions.
pub mod recent_files;
/// Local HTTP remote-control endpoint (feature `remote-control`).
#[cfg(feature = "remote-control")]
pub mod remote_control;
/// Loader for `.scheme`/colour theme files.
pub mod scheme_loader;
/// Long-running background services (file watcher, notifications, single-instance).
pub mod services;
/// Stable settings-document keys for the shared text-editor surface.
pub mod settings_keys;
/// XDG shell integration: open URLs, files, app launchers.
pub mod shell;
/// Async task envelope types and progress contracts.
pub mod tasks;
/// GTK-free customizable toolbar layout contracts.
pub mod toolbar_layout;
/// URL parsing and validation helpers.
pub mod url;
/// Application and workspace window shell contracts.
pub mod window_shell;
/// Display-free zone-layout model (control catalog + per-zone ordered ids +
/// hidden set) behind the customizable button-zone editor and runtime applier.
pub mod zone_layout;
