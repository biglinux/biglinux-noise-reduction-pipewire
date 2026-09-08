//! Filesystem paths and version constants.
//!
//! Centralising these here keeps hard-coded paths out of the rest of the
//! codebase. Tests that need a synthetic config root override via
//! `AppSettings::load_from` / `save_to`.

use std::path::{Path, PathBuf};

pub const EQ_BANDS_HZ: [u32; 10] = [31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
use crate::config::noise_model::{
    DEEPFILTER_LADSPA_PATH, GTCRN_LADSPA_PATH, LADSPA_DIR_PATH, NoiseModel,
};

/// D-Bus / desktop application identifier.
pub const APP_ID: &str = "br.com.biglinux.microphone";

/// Gettext translation domain.
pub const GETTEXT_PACKAGE: &str = "biglinux-microphone";

/// System data directory shipped by the package — holds the SVG
/// illustrations the didactic UI cards display next to each control.
/// XDG_DATA_DIRS is consulted at runtime via [`illustrations_dir`] so
/// dev runs can override it without an install.
pub const APP_DATA_DIR: &str = "/usr/share/biglinux-microphone";

/// User config directory: `$XDG_CONFIG_HOME/biglinux-microphone` with
/// `$HOME/.config/biglinux-microphone` as the portable fallback.
#[must_use]
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".config")
        })
        .join("biglinux-microphone")
}

/// Path to `settings.json` inside the user config directory.
#[must_use]
pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

/// System LADSPA directory.
#[must_use]
pub fn ladspa_dir() -> &'static Path {
    Path::new(LADSPA_DIR_PATH)
}

/// Path to the GTCRN LADSPA plugin shared object.
#[must_use]
pub fn gtcrn_plugin() -> PathBuf {
    PathBuf::from(GTCRN_LADSPA_PATH)
}

/// Path to the DeepFilterNet3 LADSPA plugin shared object. Shipped by
/// the optional `deepfilternet-ladspa` package — call
/// [`deepfilter_available`] before assuming it exists.
#[must_use]
pub fn deepfilter_plugin() -> PathBuf {
    PathBuf::from(DEEPFILTER_LADSPA_PATH)
}

/// Whether the DeepFilterNet3 LADSPA plugin is currently installed.
/// Used by the UI to gate the DFN3 option in the model selector and by
/// the settings loader to fall back to GTCRN when an installed system
/// previously had DFN3 selected and then uninstalled the package.
#[must_use]
pub fn deepfilter_available() -> bool {
    NoiseModel::DeepFilterNet3.plugin_available()
}

/// Package version read from `Cargo.toml`.
#[must_use]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Application id (`br.com.biglinux.microphone`).
#[must_use]
pub fn app_id() -> &'static str {
    APP_ID
}

/// Gettext domain name.
#[must_use]
pub fn gettext_package() -> &'static str {
    GETTEXT_PACKAGE
}

/// Directory holding the didactic SVG illustrations.
///
/// Resolution order:
/// 1. `BIGLINUX_MICROPHONE_DATA_DIR` env var (dev override).
/// 2. Each entry of `XDG_DATA_DIRS` joined with
///    `biglinux-microphone/illustrations` until one exists.
/// 3. The compiled-in [`APP_DATA_DIR`] fallback.
#[must_use]
pub fn illustrations_dir() -> PathBuf {
    if let Some(dev) = std::env::var_os("BIGLINUX_MICROPHONE_DATA_DIR") {
        return PathBuf::from(dev).join("illustrations");
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_DIRS") {
        for entry in std::env::split_paths(&xdg) {
            let candidate = entry.join("biglinux-microphone").join("illustrations");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from(APP_DATA_DIR).join("illustrations")
}
