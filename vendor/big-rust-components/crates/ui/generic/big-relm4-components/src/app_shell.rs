// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! The `run` orchestrator that turns an `AppShellSpec` into a running app.
//!
//! This is the top of the bootstrap: it composes the display-free spec
//! (`big_app_kit_core::app_shell`), the GTK adapter primitives
//! ([`big_app_kit::app_shell`]), the shared resource bundle
//! ([`crate::resources`]), and the shared theme helpers ([`crate::theme`]) —
//! the reason it lives here and not in `big-app-kit`, which cannot depend on
//! this crate.
//!
//! An adopter shrinks its `main.rs` to a spec plus a `BigAppWindow`:
//!
//! ```ignore
//! fn main() -> gtk::glib::ExitCode {
//!     big_relm4_components::app_shell::run::<MyWindow>(my_spec())
//! }
//! ```
//!
//! `run` boots services only; the window-shell spec the app builds inside
//! `BigAppWindow::build` owns all chrome (see `big_app_kit_core::app_shell`
//! for the chrome-is-data contract).

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::actions::install_static_accels;
use big_app_kit::settings_store::{
    BigSettingsStore, import_legacy_settings, open_suite_settings, suite_settings_file,
};
use relm4::gtk;
use relm4::gtk::{gio, glib};

use crate::{resources, theme};

// Re-export the whole adopter-facing surface so an app boots from one import
// path: the spec types (from core, via big-app-kit), the GTK context/trait, and
// the action handler.
#[doc(inline)]
pub use big_app_kit::app_shell::{
    AppShellContext, AppShellSpec, AppShellSpecError, BigAppWindow, BigAppearanceBoot,
    BigLegacyImport, BigSettingsLocation, BigShellActionHandler, register_shell_actions_on,
};

/// Boot an [`AppShellSpec`] and run the windowed application to completion.
///
/// Boots the shared process-wide services via `boot_services` (validate →
/// init gtk/adw → register the shared resource bundle and any extra
/// icon-resource paths → open the settings store, optional app legacy import,
/// suite store, and suite legacy import → build the `adw::Application` with
/// `HANDLES_COMMAND_LINE` iff the spec handles files → seed Relm4 → install
/// static accelerators), then runs the window-specific tail: boot appearance
/// (color scheme, app CSS, optional dark listener) and, on first activation,
/// build the window with [`BigAppWindow::build`], bind window state, and
/// present; on later activations present the existing window. File arguments
/// are forwarded to [`BigAppWindow::open_paths`].
///
/// Returns the application's exit code; returns [`glib::ExitCode::FAILURE`]
/// without starting a loop when the spec is invalid or the toolkit fails to
/// initialize.
#[must_use]
pub fn run<W: BigAppWindow + 'static>(spec: AppShellSpec) -> glib::ExitCode {
    let Some((app, settings, suite)) = boot_services(&spec) else {
        return glib::ExitCode::FAILURE;
    };
    boot_appearance::<W>(&spec.appearance);
    connect_activation::<W>(&app, Rc::new(spec), settings, suite);
    app.run()
}

/// Windowless AppShell: runs the same service boot as [`run`], but the
/// activation builds surfaces/services itself instead of one presented window.
/// For layer-shell shells and background services with no single main window.
/// The `build` closure owns appearance and lifecycle; AppShell does not
/// present, `set_application`, or bind window state.
///
/// Appearance is deliberately not booted here (unlike [`run`]): the closure
/// owns it. Returns the application's exit code; returns
/// [`glib::ExitCode::FAILURE`] without starting a loop when the spec is invalid
/// or the toolkit fails to initialize.
#[must_use]
pub fn run_service(
    spec: AppShellSpec,
    build: impl FnOnce(&AppShellContext) + 'static,
) -> glib::ExitCode {
    let Some((app, settings, suite)) = boot_services(&spec) else {
        return glib::ExitCode::FAILURE;
    };
    connect_service_activation(&app, settings, suite, build);
    app.run()
}

/// Boot the process-wide services shared by [`run`] and [`run_service`].
///
/// Order (reconciled from the app fleet): validate the spec → init gtk/adw →
/// register the shared resource bundle and any extra icon-resource paths → open
/// the settings store (+ optional app legacy import, suite store, and suite
/// legacy import) → build the `adw::Application` (with `HANDLES_COMMAND_LINE`
/// iff the spec handles files) → seed Relm4 via `RelmApp::from_app` → install
/// static accelerators.
///
/// Returns the application and its opened stores on success. Returns `None`
/// (after logging) when the spec is invalid or the toolkit fails to initialize;
/// both callers map that to [`glib::ExitCode::FAILURE`]. Appearance boot and
/// activation are intentionally excluded — they differ between the windowed
/// [`run`] (appearance + window activation) and the windowless [`run_service`]
/// (the `build` closure owns both).
fn boot_services(
    spec: &AppShellSpec,
) -> Option<(adw::Application, BigSettingsStore, Option<BigSettingsStore>)> {
    if let Err(error) = spec.validate() {
        log::error!("app shell: spec invalid: {error}");
        return None;
    }
    if initialize_gtk_runtime().is_err() {
        return None;
    }

    resources::ensure_registered();
    register_extra_resource_paths(spec.extra_resource_paths);

    let settings = open_app_settings(spec);
    apply_legacy_import(&settings, spec.legacy_import.as_ref());
    let suite = open_optional_suite_store(spec);
    apply_suite_legacy_import(suite.as_ref(), spec.suite_legacy_import.as_ref());

    let app = build_application(spec);
    let _relm = relm4::RelmApp::<()>::from_app(app.clone());
    install_static_accels(&app, spec.static_accels);

    Some((app, settings, suite))
}

/// Bind the windowless activation: the first activation runs `build` inside an
/// [`AppShellContext`] with no window-state; later activations are a no-op.
///
/// The `take()` is the single-instance guard — it reproduces the windowed
/// [`run`] behavior where a second activation does not rebuild (there it
/// presents the existing window; here there is no window to present, so it
/// simply returns). AppShell does not present, `set_application`, or bind
/// window state: the `build` closure owns every surface and its lifetime.
fn connect_service_activation(
    app: &adw::Application,
    settings: BigSettingsStore,
    suite: Option<BigSettingsStore>,
    build: impl FnOnce(&AppShellContext) + 'static,
) {
    let build = RefCell::new(Some(build));
    app.connect_activate(move |app| {
        let Some(build) = build.borrow_mut().take() else {
            return;
        };
        let cx = AppShellContext {
            app,
            settings: &settings,
            suite: suite.as_ref(),
            window_state: None,
        };
        build(&cx);
    });
}

fn initialize_gtk_runtime() -> Result<(), glib::ExitCode> {
    if let Err(error) = gtk::init() {
        log::error!("app shell: failed to initialize GTK: {error}");
        return Err(glib::ExitCode::FAILURE);
    }
    if let Err(error) = adw::init() {
        log::error!("app shell: failed to initialize libadwaita: {error}");
        return Err(glib::ExitCode::FAILURE);
    }
    Ok(())
}

fn register_extra_resource_paths(paths: &[&str]) {
    if paths.is_empty() {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else {
        log::warn!(
            "app shell: no default display; skipping {} icon resource path(s)",
            paths.len()
        );
        return;
    };
    let icon_theme = gtk::IconTheme::for_display(&display);
    for path in paths {
        icon_theme.add_resource_path(path);
    }
}

fn boot_appearance<W: BigAppWindow + 'static>(appearance: &BigAppearanceBoot) {
    if let Some(color_scheme) = appearance.color_scheme {
        theme::apply_color_scheme(color_scheme);
    }
    if let Some(base_css) = appearance.base_css {
        theme::load_app_css(base_css);
    }
    if appearance.watch_dark {
        theme::install_style_dark_listener(W::on_dark_changed);
    }
}

fn build_application(spec: &AppShellSpec) -> adw::Application {
    let mut builder = adw::Application::builder().application_id(spec.application_id);
    if spec.handle_files {
        builder = builder.flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE);
    }
    builder.build()
}

fn open_app_settings(spec: &AppShellSpec) -> BigSettingsStore {
    let path = spec.settings_file(big_os_kit::xdg_dirs::config_dir().as_deref());
    let defaults = (spec.settings_defaults)();
    BigSettingsStore::open(&path, spec.version, &defaults).unwrap_or_else(|error| {
        log::warn!("app shell: settings open failed ({error}); starting blank");
        BigSettingsStore::blank(&path, spec.version, &defaults)
    })
}

fn apply_legacy_import(store: &BigSettingsStore, legacy_import: Option<&BigLegacyImport>) {
    let Some(legacy_import) = legacy_import else {
        return;
    };
    match import_legacy_settings(store, &legacy_import.legacy_file, legacy_import.key_filter) {
        Ok(0) => {}
        Ok(imported) => log::info!("app shell: imported {imported} legacy setting(s)"),
        Err(error) => log::warn!("app shell: legacy settings import failed: {error}"),
    }
}

/// Seed a brand-new suite document from a legacy file, exactly once.
///
/// Gating decision (preserves the editor's original `import_legacy_suite_settings`
/// behavior): this runs **only when the suite document did not exist before this
/// boot**, on top of `import_legacy_settings`' own per-key never-overwrite. That
/// is stricter than the app-store [`apply_legacy_import`], which imports any
/// still-unset key regardless of the document's prior existence — an existing
/// suite document is left untouched even if it lacks some importable keys,
/// because the suite store is shared across the suite and its keys may have been
/// set independently. `open_optional_suite_store` opened the store with
/// `BigSettingsStore::open`, which never writes, so the existence check here
/// still reflects the pre-boot state.
fn apply_suite_legacy_import(
    suite: Option<&BigSettingsStore>,
    suite_legacy_import: Option<&BigLegacyImport>,
) {
    let (Some(suite), Some(suite_legacy_import)) = (suite, suite_legacy_import) else {
        return;
    };
    if suite_settings_file().exists() {
        return;
    }
    match import_legacy_settings(
        suite,
        &suite_legacy_import.legacy_file,
        suite_legacy_import.key_filter,
    ) {
        Ok(0) => {}
        Ok(imported) => log::info!("app shell: imported {imported} legacy suite setting(s)"),
        Err(error) => log::warn!("app shell: legacy suite settings import failed: {error}"),
    }
}

fn open_optional_suite_store(spec: &AppShellSpec) -> Option<BigSettingsStore> {
    if !spec.open_suite_store {
        return None;
    }
    match open_suite_settings(spec.version) {
        Ok(store) => Some(store),
        Err(error) => {
            log::warn!("app shell: suite settings open failed ({error}); using blank suite store");
            Some(BigSettingsStore::blank(
                &suite_settings_file(),
                spec.version,
                &serde_json::Map::new(),
            ))
        }
    }
}

fn connect_activation<W: BigAppWindow + 'static>(
    app: &adw::Application,
    spec: Rc<AppShellSpec>,
    settings: BigSettingsStore,
    suite: Option<BigSettingsStore>,
) {
    if spec.handle_files {
        app.connect_command_line(move |app, command_line| {
            let file_args = command_line_file_args(command_line);
            drive_activation::<W>(app, &spec, &settings, suite.as_ref(), file_args);
            glib::ExitCode::from(0u8)
        });
    } else {
        app.connect_activate(move |app| {
            drive_activation::<W>(app, &spec, &settings, suite.as_ref(), Vec::new());
        });
    }
}

fn drive_activation<W: BigAppWindow>(
    app: &adw::Application,
    spec: &AppShellSpec,
    settings: &BigSettingsStore,
    suite: Option<&BigSettingsStore>,
    file_args: Vec<String>,
) {
    if let Some(window) = app.active_window() {
        // Single-instance: hand the invocation to the live window, then present
        // it. `open_paths` is called even with no file args so a multi-window
        // app can act on a bare reactivation (open a new tab/window per its own
        // policy); single-window apps default to a no-op and just get presented.
        W::open_paths(file_args);
        window.present();
        return;
    }

    let context = AppShellContext {
        app,
        settings,
        suite,
        window_state: spec.window_state.as_ref(),
    };
    let window = W::build(&context);
    // Associate the built window with the application so both an app-bound
    // `adw::ApplicationWindow` and a plain `adw::Window` participate in
    // single-instance and keep the app alive while shown. Idempotent when the
    // window was already app-bound at build.
    window.set_application(Some(app));
    context.bind_window_state(&window);
    W::open_paths(file_args);
    window.present();
}

fn command_line_file_args(command_line: &gio::ApplicationCommandLine) -> Vec<String> {
    // Raw argv strings (path or URI), so remote URIs reach the app verbatim.
    command_line
        .arguments()
        .into_iter()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect()
}
