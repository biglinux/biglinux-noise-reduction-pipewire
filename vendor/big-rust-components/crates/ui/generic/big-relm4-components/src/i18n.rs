// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared `big-components` textdomain for component-owned user-visible
//! strings (shared preference rows, shared page titles).
//!
//! Apps keep their own textdomain for app strings and compose a chained
//! translator for shared rows: try the app domain first, fall back here.
//! Binding is lazy and domain-explicit (`dgettext`) — it never touches the
//! process default domain or locale, so embedded/multicall hosts are safe.

use std::path::PathBuf;
use std::sync::Once;

const DOMAIN: &str = "big-components";
const SYSTEM_LOCALE_DIR: &str = "/usr/share/locale";

static BIND: Once = Once::new();

/// Bind the shared components textdomain. Idempotent; called lazily by
/// [`t`], or explicitly at app boot.
pub fn bind_components_textdomain() {
    BIND.call_once(|| {
        let dir = resolve_locale_dir();
        if let Err(e) = gettextrs::bindtextdomain(DOMAIN, dir.clone()) {
            log::warn!("bindtextdomain({DOMAIN}, {}) failed: {e}", dir.display());
        }
    });
}

/// Translate a component-owned msgid through the `big-components` domain.
/// Returns the msgid itself when no catalog is installed.
#[must_use]
pub fn t(msgid: &str) -> String {
    bind_components_textdomain();
    gettextrs::dgettext(DOMAIN, msgid)
}

/// Marks a component-owned string for extraction without translating.
#[must_use]
pub const fn gettext_noop(msgid: &'static str) -> &'static str {
    msgid
}

fn resolve_locale_dir() -> PathBuf {
    if let Ok(env_dir) = std::env::var("BIG_COMPONENTS_LOCALE_DIR")
        && !env_dir.is_empty()
    {
        return PathBuf::from(env_dir);
    }
    PathBuf::from(SYSTEM_LOCALE_DIR)
}
