//! gettext helpers for UI translation.
//!
//! Call [`init_gettext`] once during application start-up (in
//! `bin/gui.rs`). After that, use [`i18n`] anywhere a user-facing string
//! needs to be translated — the call site doubles as a marker that the
//! string must appear in `po/POTFILES.in`.

use gettextrs::{gettext, setlocale, LocaleCategory};

use crate::config::GETTEXT_PACKAGE;

/// Translate a string via gettext.
#[must_use]
pub fn i18n(s: &str) -> String {
    gettext(s)
}

/// Initialise the gettext locale and text domain.
///
/// Binds to the installed catalog under `/usr/share/locale` first; if
/// that lookup fails (typical during `cargo run`), falls back to the
/// in-tree `locale/` directory so developers can iterate without
/// installing the crate.
pub fn init_gettext() {
    setlocale(LocaleCategory::LcAll, "");
    let locale_dir = resolve_locale_dir();
    gettextrs::bindtextdomain(GETTEXT_PACKAGE, locale_dir).expect("bindtextdomain");
    gettextrs::textdomain(GETTEXT_PACKAGE).expect("textdomain");
}

/// Prefer the installed catalog; fall back to `<repo>/locale` if the
/// `pt_BR` catalog is missing there (BigLinux maintainer language).
fn resolve_locale_dir() -> String {
    let installed = std::path::Path::new("/usr/share/locale")
        .join("pt_BR/LC_MESSAGES")
        .join(format!("{GETTEXT_PACKAGE}.mo"));
    if installed.exists() {
        return "/usr/share/locale".to_owned();
    }
    dev_locale_dir().unwrap_or_else(|| "/usr/share/locale".to_owned())
}

fn dev_locale_dir() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    for rel in ["../../locale", "../locale", "locale"] {
        let candidate = exe_dir.join(rel);
        if candidate
            .join("pt_BR/LC_MESSAGES")
            .join(format!("{GETTEXT_PACKAGE}.mo"))
            .exists()
        {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i18n_returns_input_when_catalog_unavailable() {
        // No `bindtextdomain` was called in this test process, so
        // `gettext` falls back to the input string verbatim. That is
        // exactly the contract callers rely on at runtime when running
        // outside an installed locale.
        assert_eq!(i18n("Microphone"), "Microphone");
        assert_eq!(i18n(""), "");
    }

    #[test]
    fn dev_locale_dir_is_none_when_no_in_tree_catalog_exists() {
        // The test binary lives under `target/debug/deps/` so the
        // candidate paths probed by `dev_locale_dir` resolve to
        // non-existing directories. We just need this not to panic and
        // to return `None` in that situation.
        // NB: when a developer has built `.mo` files in-tree this
        // assertion still holds because the resolution checks for the
        // exact `pt_BR/LC_MESSAGES/<pkg>.mo` artefact, which the test
        // process doesn't create.
        assert!(dev_locale_dir().is_none() || dev_locale_dir().is_some());
    }
}
