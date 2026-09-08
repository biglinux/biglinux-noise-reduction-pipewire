// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Display-free bootstrap contract for a BigLinux desktop application.
//!
//! `AppShellSpec` is the single declarative description of everything a
//! standard app needs before it draws its first window: application identity,
//! settings location and defaults, optional one-shot legacy imports (into the
//! app store and, when opened, the suite store), the shared suite store,
//! appearance boot, static accelerators, file-handling, extra icon-resource
//! paths, and durable window-state persistence. The GTK
//! adapter that turns this spec into a running `adw::Application` lives in
//! `big_relm4_components::app_shell`; the reusable GTK service primitives live
//! in `big_app_kit::app_shell`. This module stays free of gtk4/libadwaita/relm4
//! so packaging tooling and headless tests can validate a boot contract without
//! a display.
//!
//! # Chrome is data; AppShell boots services
//!
//! `AppShellSpec` deliberately carries **no** chrome placement. Header bars,
//! tab strips, toolbar positions, and per-button surfaces stay declaratively in
//! the window-shell specs (`BigApplicationWindowShellSpec` /
//! `BigWorkspaceWindowShellSpec`) that the app passes to its own window build.
//! AppShell only boots process-wide services (resources, settings, appearance,
//! actions, single-instance, window-state) and then hands the app an
//! `AppShellContext`; the window-shell spec the app builds owns every chrome
//! decision. Keeping chrome out of AppShell is a framework acceptance criterion:
//! two apps with identical services can present completely different chrome.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::window_shell::BigWindowStatePersistenceSpec;

/// Where an app's settings document lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BigSettingsLocation {
    /// `settings.json` under the XDG config home in a per-app directory, e.g.
    /// `$XDG_CONFIG_HOME/<dir_name>/settings.json` (or `$HOME/.config/...`).
    XdgApp {
        /// Per-app directory name under the XDG config home.
        dir_name: &'static str,
    },
    /// An explicit absolute path chosen by the app (tests, sandboxes, ports).
    Explicit(PathBuf),
}

impl BigSettingsLocation {
    /// Resolve the settings file path against an already-resolved config home.
    ///
    /// The `config_home` is the value the imperative shell reads once (normally
    /// `big_os_kit::xdg_dirs::config_dir()`, which already applies the
    /// `XDG_CONFIG_HOME` → `$HOME/.config` fallback). Passing it in keeps this
    /// resolution pure and unit-testable without touching the environment. When
    /// `config_home` is `None`, a relative `.config` base is used so the result
    /// is always defined (mirrors `suite_settings_file`).
    #[must_use]
    pub fn settings_file(&self, config_home: Option<&Path>) -> PathBuf {
        match self {
            Self::Explicit(path) => path.clone(),
            Self::XdgApp { dir_name } => config_home
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".config"))
                .join(dir_name)
                .join("settings.json"),
        }
    }

    fn validate(&self) -> Result<(), AppShellSpecError> {
        match self {
            Self::XdgApp { dir_name } => require(
                !dir_name.trim().is_empty(),
                AppShellSpecError::EmptySettingsDirName,
            ),
            Self::Explicit(path) => require(
                !path.as_os_str().is_empty(),
                AppShellSpecError::EmptyExplicitSettingsPath,
            ),
        }
    }
}

/// One-shot import of keys from a legacy settings document into the app store.
#[derive(Debug, Clone)]
pub struct BigLegacyImport {
    /// Legacy settings file to read (never modified).
    pub legacy_file: PathBuf,
    /// Predicate deciding which legacy keys are copied when unset.
    pub key_filter: fn(&str) -> bool,
}

impl BigLegacyImport {
    /// Import every legacy key that `key_filter` accepts.
    #[must_use]
    pub fn new(legacy_file: impl Into<PathBuf>, key_filter: fn(&str) -> bool) -> Self {
        Self {
            legacy_file: legacy_file.into(),
            key_filter,
        }
    }
}

/// Appearance decisions applied once, before the first window is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigAppearanceBoot {
    /// Color-scheme key forwarded to the shared `apply_color_scheme` helper at
    /// startup (`Some("gtk-light" | "gtk-dark" | "gradient" | …)`); `None`
    /// leaves the libadwaita default (follow the system preference).
    pub color_scheme: Option<&'static str>,
    /// Application CSS loaded once, before the first window, via the shared
    /// `load_app_css` helper; `None` loads no app CSS.
    pub base_css: Option<&'static str>,
    /// Install a libadwaita dark-state listener at startup so the window type's
    /// `on_dark_changed` hook runs whenever the system light/dark flips.
    pub watch_dark: bool,
}

impl BigAppearanceBoot {
    /// No color-scheme override, no app CSS, no dark listener — pure libadwaita
    /// default appearance.
    #[must_use]
    pub const fn adwaita_default() -> Self {
        Self {
            color_scheme: None,
            base_css: None,
            watch_dark: false,
        }
    }

    fn validate(&self) -> Result<(), AppShellSpecError> {
        if let Some(color_scheme) = self.color_scheme {
            require(
                !color_scheme.trim().is_empty(),
                AppShellSpecError::EmptyColorScheme,
            )?;
        }
        Ok(())
    }
}

impl Default for BigAppearanceBoot {
    fn default() -> Self {
        Self::adwaita_default()
    }
}

/// The complete display-free description of an app's bootstrap.
///
/// Construct with [`AppShellSpec::new`] and the builder setters; the GTK
/// adapter (`big_relm4_components::app_shell::run`) consumes it to stand up the
/// `adw::Application`. Fields are public for transparency and testing, but the
/// builder is the intended construction path.
#[derive(Debug, Clone)]
pub struct AppShellSpec {
    /// Reverse-DNS application id (`adw::Application` id, single-instance key).
    pub application_id: &'static str,
    /// App version string persisted in the settings envelope metadata.
    pub version: &'static str,
    /// Handle file arguments: registers `HANDLES_COMMAND_LINE` and forwards the
    /// parsed paths to the window type's `open_paths`.
    pub handle_files: bool,
    /// Extra GResource icon directories registered on the default icon theme.
    pub extra_resource_paths: &'static [&'static str],
    /// Where the app settings document lives.
    pub settings: BigSettingsLocation,
    /// Default settings values, merged for keys the document does not set.
    pub settings_defaults: fn() -> Map<String, Value>,
    /// Optional one-shot import from a legacy settings document into the app store.
    pub legacy_import: Option<BigLegacyImport>,
    /// Also open the shared suite settings store (`biglinux/suite-settings.json`).
    pub open_suite_store: bool,
    /// Optional one-shot import from a legacy settings document into the suite
    /// store; requires [`open_suite_store`](Self::open_suite_store).
    pub suite_legacy_import: Option<BigLegacyImport>,
    /// Appearance boot applied before the first window.
    pub appearance: BigAppearanceBoot,
    /// `(detailed-action-name, accelerators)` pairs bound on the application.
    pub static_accels: &'static [(&'static str, &'static [&'static str])],
    /// Optional durable window-state (geometry/maximized/fullscreen) persistence.
    pub window_state: Option<BigWindowStatePersistenceSpec>,
}

impl AppShellSpec {
    /// Start a spec with the required identity, settings location, and defaults.
    /// Everything else takes framework defaults; refine with the builder setters.
    #[must_use]
    pub fn new(
        application_id: &'static str,
        version: &'static str,
        settings: BigSettingsLocation,
        settings_defaults: fn() -> Map<String, Value>,
    ) -> Self {
        Self {
            application_id,
            version,
            handle_files: false,
            extra_resource_paths: &[],
            settings,
            settings_defaults,
            legacy_import: None,
            open_suite_store: false,
            suite_legacy_import: None,
            appearance: BigAppearanceBoot::adwaita_default(),
            static_accels: &[],
            window_state: None,
        }
    }

    /// Handle file arguments and forward parsed paths to `open_paths`.
    #[must_use]
    pub fn handle_files(mut self, handle_files: bool) -> Self {
        self.handle_files = handle_files;
        self
    }

    /// Register extra GResource icon directories on the default icon theme.
    #[must_use]
    pub fn extra_resource_paths(mut self, extra_resource_paths: &'static [&'static str]) -> Self {
        self.extra_resource_paths = extra_resource_paths;
        self
    }

    /// Run a one-shot legacy-settings import during boot.
    #[must_use]
    pub fn legacy_import(mut self, legacy_import: BigLegacyImport) -> Self {
        self.legacy_import = Some(legacy_import);
        self
    }

    /// Also open the shared suite settings store during boot.
    #[must_use]
    pub fn open_suite_store(mut self, open_suite_store: bool) -> Self {
        self.open_suite_store = open_suite_store;
        self
    }

    /// Run a one-shot legacy import into the suite store during boot.
    ///
    /// Requires [`open_suite_store`](Self::open_suite_store); [`validate`] rejects
    /// a suite import without an open suite store.
    ///
    /// [`validate`]: Self::validate
    #[must_use]
    pub fn suite_legacy_import(mut self, suite_legacy_import: BigLegacyImport) -> Self {
        self.suite_legacy_import = Some(suite_legacy_import);
        self
    }

    /// Set the appearance boot.
    #[must_use]
    pub fn appearance(mut self, appearance: BigAppearanceBoot) -> Self {
        self.appearance = appearance;
        self
    }

    /// Set the static accelerators bound on the application.
    #[must_use]
    pub fn static_accels(
        mut self,
        static_accels: &'static [(&'static str, &'static [&'static str])],
    ) -> Self {
        self.static_accels = static_accels;
        self
    }

    /// Persist and restore durable window state under `window_state`.
    #[must_use]
    pub fn window_state(mut self, window_state: BigWindowStatePersistenceSpec) -> Self {
        self.window_state = Some(window_state);
        self
    }

    /// Resolve the settings file path for this spec (see
    /// [`BigSettingsLocation::settings_file`]).
    #[must_use]
    pub fn settings_file(&self, config_home: Option<&Path>) -> PathBuf {
        self.settings.settings_file(config_home)
    }

    /// Validate the bootstrap contract before it drives GTK.
    ///
    /// # Errors
    ///
    /// Returns [`AppShellSpecError`] when identity, settings location, appearance,
    /// resource paths, legacy import, accelerators, or window-state identity are
    /// empty or malformed, or when a suite legacy import is requested without an
    /// open suite store.
    pub fn validate(&self) -> Result<(), AppShellSpecError> {
        require(
            !self.application_id.trim().is_empty(),
            AppShellSpecError::EmptyApplicationId,
        )?;
        require(
            !self.version.trim().is_empty(),
            AppShellSpecError::EmptyVersion,
        )?;
        self.settings.validate()?;
        self.appearance.validate()?;
        for path in self.extra_resource_paths {
            require(
                !path.trim().is_empty(),
                AppShellSpecError::EmptyResourcePath,
            )?;
        }
        if let Some(legacy_import) = &self.legacy_import {
            require(
                !legacy_import.legacy_file.as_os_str().is_empty(),
                AppShellSpecError::EmptyLegacyImportPath,
            )?;
        }
        if let Some(suite_legacy_import) = &self.suite_legacy_import {
            require(
                self.open_suite_store,
                AppShellSpecError::SuiteImportWithoutSuiteStore,
            )?;
            require(
                !suite_legacy_import.legacy_file.as_os_str().is_empty(),
                AppShellSpecError::EmptyLegacyImportPath,
            )?;
        }
        for (action, _) in self.static_accels {
            require(
                !action.trim().is_empty(),
                AppShellSpecError::EmptyAccelAction,
            )?;
        }
        if let Some(window_state) = &self.window_state {
            require(
                !window_state.persistence_id.trim().is_empty(),
                AppShellSpecError::EmptyWindowStatePersistenceId,
            )?;
        }
        Ok(())
    }
}

/// Settings keys for durable window geometry, derived from the persistence id.
///
/// Keys are `<persistence_id>.width`, `.height`, `.maximized`, and
/// `.fullscreen`. Derivation is pure so the GTK binder in `big_app_kit` and its
/// tests agree on the exact key names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWindowStateKeys {
    /// Settings key holding the restored default width.
    pub width: String,
    /// Settings key holding the restored default height.
    pub height: String,
    /// Settings key holding the maximized flag.
    pub maximized: String,
    /// Settings key holding the fullscreen flag.
    pub fullscreen: String,
}

impl BigWindowStateKeys {
    /// Derive the geometry keys for a persistence spec.
    #[must_use]
    pub fn for_spec(spec: &BigWindowStatePersistenceSpec) -> Self {
        Self {
            width: format!("{}.width", spec.persistence_id),
            height: format!("{}.height", spec.persistence_id),
            maximized: format!("{}.maximized", spec.persistence_id),
            fullscreen: format!("{}.fullscreen", spec.persistence_id),
        }
    }
}

/// Error returned when an [`AppShellSpec`] cannot be accepted for boot.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AppShellSpecError {
    /// Application id is empty or whitespace-only.
    EmptyApplicationId,
    /// Version string is empty or whitespace-only.
    EmptyVersion,
    /// XDG settings directory name is empty or whitespace-only.
    EmptySettingsDirName,
    /// Explicit settings path is empty.
    EmptyExplicitSettingsPath,
    /// Color-scheme key is present but empty or whitespace-only.
    EmptyColorScheme,
    /// An extra icon-resource path is empty or whitespace-only.
    EmptyResourcePath,
    /// A legacy-import source path (app or suite) is empty.
    EmptyLegacyImportPath,
    /// A suite legacy import was requested without opening the suite store.
    SuiteImportWithoutSuiteStore,
    /// A static-accelerator action name is empty or whitespace-only.
    EmptyAccelAction,
    /// Window-state persistence id is empty or whitespace-only.
    EmptyWindowStatePersistenceId,
}

impl fmt::Display for AppShellSpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyApplicationId => formatter.write_str("application id must not be empty"),
            Self::EmptyVersion => formatter.write_str("version must not be empty"),
            Self::EmptySettingsDirName => {
                formatter.write_str("xdg settings directory name must not be empty")
            }
            Self::EmptyExplicitSettingsPath => {
                formatter.write_str("explicit settings path must not be empty")
            }
            Self::EmptyColorScheme => formatter.write_str("color scheme key must not be empty"),
            Self::EmptyResourcePath => {
                formatter.write_str("extra icon-resource path must not be empty")
            }
            Self::EmptyLegacyImportPath => {
                formatter.write_str("legacy-import source path must not be empty")
            }
            Self::SuiteImportWithoutSuiteStore => {
                formatter.write_str("suite legacy import requires open_suite_store")
            }
            Self::EmptyAccelAction => {
                formatter.write_str("static accelerator action name must not be empty")
            }
            Self::EmptyWindowStatePersistenceId => {
                formatter.write_str("window-state persistence id must not be empty")
            }
        }
    }
}

impl Error for AppShellSpecError {}

fn require(condition: bool, error: AppShellSpecError) -> Result<(), AppShellSpecError> {
    if condition { Ok(()) } else { Err(error) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_defaults() -> Map<String, Value> {
        Map::new()
    }

    fn spec() -> AppShellSpec {
        AppShellSpec::new(
            "br.com.biglinux.Sample",
            "1.2.3",
            BigSettingsLocation::XdgApp {
                dir_name: "big-sample",
            },
            empty_defaults,
        )
    }

    #[test]
    fn xdg_settings_path_joins_config_home_dir_and_file() {
        let location = BigSettingsLocation::XdgApp {
            dir_name: "big-sample",
        };
        let resolved = location.settings_file(Some(Path::new("/home/u/.config")));
        assert_eq!(
            resolved,
            PathBuf::from("/home/u/.config/big-sample/settings.json")
        );
    }

    #[test]
    fn xdg_settings_path_falls_back_to_relative_config_when_home_unresolved() {
        // Mirrors `config_dir()` returning None (no HOME / XDG_CONFIG_HOME): the
        // adapter still gets a defined path instead of panicking.
        let location = BigSettingsLocation::XdgApp {
            dir_name: "big-sample",
        };
        let resolved = location.settings_file(None);
        assert_eq!(resolved, PathBuf::from(".config/big-sample/settings.json"));
    }

    #[test]
    fn explicit_settings_path_passes_through_config_home_unchanged() {
        let location = BigSettingsLocation::Explicit(PathBuf::from("/tmp/app/state.json"));
        assert_eq!(
            location.settings_file(Some(Path::new("/ignored"))),
            PathBuf::from("/tmp/app/state.json")
        );
    }

    #[test]
    fn valid_minimal_spec_passes_validation() {
        assert!(spec().validate().is_ok());
    }

    #[test]
    fn empty_application_id_is_rejected() {
        let mut spec = spec();
        spec.application_id = "  ";
        assert_eq!(spec.validate(), Err(AppShellSpecError::EmptyApplicationId));
    }

    #[test]
    fn empty_xdg_dir_name_is_rejected() {
        let spec = AppShellSpec::new(
            "br.com.biglinux.Sample",
            "1.0",
            BigSettingsLocation::XdgApp { dir_name: " " },
            empty_defaults,
        );
        assert_eq!(
            spec.validate(),
            Err(AppShellSpecError::EmptySettingsDirName)
        );
    }

    #[test]
    fn empty_color_scheme_key_is_rejected() {
        let spec = spec().appearance(BigAppearanceBoot {
            color_scheme: Some(""),
            base_css: None,
            watch_dark: false,
        });
        assert_eq!(spec.validate(), Err(AppShellSpecError::EmptyColorScheme));
    }

    #[test]
    fn empty_window_state_persistence_id_is_rejected() {
        let spec = spec().window_state(BigWindowStatePersistenceSpec::new(" "));
        assert_eq!(
            spec.validate(),
            Err(AppShellSpecError::EmptyWindowStatePersistenceId)
        );
    }

    #[test]
    fn window_state_keys_are_persistence_id_prefixed() {
        let keys = BigWindowStateKeys::for_spec(&BigWindowStatePersistenceSpec::new("main-window"));
        assert_eq!(keys.width, "main-window.width");
        assert_eq!(keys.height, "main-window.height");
        assert_eq!(keys.maximized, "main-window.maximized");
        assert_eq!(keys.fullscreen, "main-window.fullscreen");
    }

    #[test]
    fn builder_defaults_are_conservative() {
        let spec = spec();
        assert!(!spec.handle_files);
        assert!(!spec.open_suite_store);
        assert!(spec.legacy_import.is_none());
        assert!(spec.suite_legacy_import.is_none());
        assert!(spec.window_state.is_none());
        assert_eq!(spec.appearance, BigAppearanceBoot::adwaita_default());
        assert!(spec.extra_resource_paths.is_empty());
        assert!(spec.static_accels.is_empty());
    }

    #[test]
    fn suite_import_without_suite_store_is_rejected() {
        let spec = spec().suite_legacy_import(BigLegacyImport::new(
            PathBuf::from("/legacy/settings.json"),
            |key| key.starts_with("ai_"),
        ));
        assert_eq!(
            spec.validate(),
            Err(AppShellSpecError::SuiteImportWithoutSuiteStore)
        );
    }

    #[test]
    fn suite_import_with_open_suite_store_passes_validation() {
        let spec = spec()
            .open_suite_store(true)
            .suite_legacy_import(BigLegacyImport::new(
                PathBuf::from("/legacy/settings.json"),
                |key| key.starts_with("ai_"),
            ));
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn empty_suite_import_path_is_rejected() {
        let spec = spec()
            .open_suite_store(true)
            .suite_legacy_import(BigLegacyImport::new(PathBuf::new(), |_| true));
        assert_eq!(
            spec.validate(),
            Err(AppShellSpecError::EmptyLegacyImportPath)
        );
    }
}
