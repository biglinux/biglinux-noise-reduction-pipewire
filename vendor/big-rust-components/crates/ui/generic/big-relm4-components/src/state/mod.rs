// SPDX-License-Identifier: MIT

//! UI state primitives.
//!
//! Common visual states that every BigLinux app needs around the
//! happy path:
//!
//! - [`empty`] — first-launch empty state with a primary CTA.
//! - [`loading`] — spinner / progress with anti-flash threshold.
//! - [`error`] — typed error state with copyable details and retry.
//! - [`skeleton`] — placeholder cards while data hydrates.
//! - [`retry`] — backoff policy reused by `error` and async commands.
//! - [`async_surface`] — the `gtk::Stack` orchestrator that composes the
//!   specs above into skeleton/loading/content/error pages.
//!
//! The `*_spec` modules are display-free `Spec` values; the widget builders
//! that turn them into GTK trees live in sibling modules. [`async_surface`] is
//! the one GTK orchestrator here — it composes those specs into a live surface.

pub mod async_surface;
pub mod empty;
pub mod error;
pub mod loading;
pub mod retry;
pub mod skeleton;

pub use async_surface::*;
pub use empty::*;
pub use error::*;
pub use loading::*;
pub use retry::*;
pub use skeleton::*;
