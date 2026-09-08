// SPDX-License-Identifier: MIT

//! Desktop service contracts.
//!
//! Each module defines a typed `Spec` value plus a trait that
//! production code implements (`zbus`, `notify`, `ashpd`, etc.).
//! Specs are display-free and serde-friendly.
//!
//! Modules:
//! - [`crate::services::notifications`] — FreeDesktop notifications with batching policy.
//! - [`crate::services::file_watcher`] — filesystem change watcher with debounce.
//! - [`crate::services::single_instance`] — D-Bus well-known-name election + activation
//!   forwarding for single-instance apps.

/// Filesystem change watcher contracts with debounce and ignore-pattern policy.
///
/// Defines a display-free `Spec` describing which paths to observe, how long
/// to coalesce events, and which file names to skip; production code wires
/// the spec to `notify`/`inotify` while tests use a fake source.
pub mod file_watcher;
/// FreeDesktop notification contracts with urgency and coalescing policy.
///
/// Provides typed specs for the bubble payload (summary, body, icon, actions)
/// plus a trait implemented in production against `zbus`/`ashpd`.
pub mod notifications;
/// Single-instance election and activation forwarding over D-Bus.
///
/// Encapsulates well-known-name acquisition, secondary-instance hand-off,
/// and the activation payload (CLI args, URIs) carried to the primary.
pub mod single_instance;
