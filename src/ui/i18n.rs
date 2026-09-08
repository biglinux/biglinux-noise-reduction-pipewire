//! gettext helpers for UI translation.
//!
//! The Relm4 root calls [`init_gettext`] once during application start-up.
//! After that, use [`i18n`] anywhere a user-facing string
//! needs to be translated — the call site doubles as a marker that the
//! string must appear in `po/POTFILES.in`.

use gettextrs::{LocaleCategory, setlocale};

use crate::config::GETTEXT_PACKAGE;

/// Translate a string through the application gettext domain.
#[must_use]
pub fn i18n(s: &str) -> String {
    gettextrs::dgettext(GETTEXT_PACKAGE, s)
}

/// Initialise the gettext locale and text domain.
///
/// Binds to the installed catalog under `/usr/share/locale` first; if
/// that lookup fails (typical during `cargo run`), falls back to the
/// in-tree `locale/` directory so developers can iterate without
/// installing the crate.
///
/// Must be called from `main`'s thread before any other thread starts —
/// see the `setlocale` call below.
pub fn init_gettext() {
    // SAFETY: `setlocale` mutates process-global locale state that C
    // library calls read without synchronisation, so it is sound only
    // while this process is single-threaded (RUSTSEC-2026-0244).
    //
    // `ui::run` calls this before `AudioMonitor::start` spawns the
    // capture thread and before `RelmApp::run` enters GTK, and the only
    // code ahead of it is argument inspection. This is also the sole
    // `setlocale` call in the crate and in the vendored components, so
    // nothing else can be racing it.
    unsafe {
        setlocale(LocaleCategory::LcAll, "");
    }
    bind_domain();
    gettextrs::textdomain(GETTEXT_PACKAGE).expect("textdomain");
}

fn bind_domain() {
    let locale_dir = resolve_locale_dir();
    gettextrs::bindtextdomain(GETTEXT_PACKAGE, locale_dir).expect("bindtextdomain");
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

    // Miri cannot call gettext's FFI; the normal cargo test
    // gate still covers the untranslated fallback contract.
    #[cfg(not(miri))]
    #[test]
    fn i18n_returns_input_when_catalog_unavailable() {
        // No `bindtextdomain` was called in this test process, so
        // `gettext` falls back to the input string verbatim. That is
        // exactly the contract callers rely on at runtime when running
        // outside an installed locale.
        assert_eq!(i18n("Microphone"), "Microphone");
        assert_eq!(i18n(""), "");
    }
}

/// Extraction marker for a literal that is translated only when displayed.
/// This must not translate at startup, before the locale is initialized.
pub const fn mark(message: &'static str) -> &'static str {
    message
}
