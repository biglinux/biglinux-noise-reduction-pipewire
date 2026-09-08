// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Embedded `big-components` GResource bundle: illustrations shared by
//! suite-wide declarative preference rows (e.g. [`crate::layout::tab_prefs`]).
//!
//! Apps without their own didactic SVGs point [`BigPrefHost::resource_prefix`]
//! at [`ILLUSTRATION_RESOURCE_PREFIX`] and call [`ensure_registered`] before
//! building preference pages. Apps that ship the same file names under their
//! own prefix (big-terminal) keep doing so — the bundle stays dormant.
//!
//! [`BigPrefHost::resource_prefix`]: crate::input::preference_rows::BigPrefHost

use std::sync::OnceLock;

use relm4::gtk::gio;

/// GResource directory holding the shared didactic illustrations.
pub const ILLUSTRATION_RESOURCE_PREFIX: &str = "/org/biglinux/big-components/illustrations";

static REGISTERED: OnceLock<()> = OnceLock::new();

/// Register the embedded `big-components` bundle with GIO (idempotent).
pub fn ensure_registered() {
    REGISTERED.get_or_init(|| {
        gio::resources_register_include!("big-components.gresource").unwrap_or_else(|err| {
            log::error!("big-components: failed to register gresource bundle: {err}");
        });
    });
}
