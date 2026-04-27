//! Persistent application settings.
//!
//! Model split by concern; `AppSettings` aggregates every section and
//! provides the JSON load/save entry points. The on-disk file lives at
//! `~/.config/biglinux-microphone/settings.json` and is written atomically
//! (write-to-temp + fsync + rename) so a crashed save cannot leave a
//! truncated file.
//!
//! BigLinux-only target: always-latest PipeWire + WirePlumber 0.5+. No
//! legacy format support is provided — malformed or outdated files fall
//! back to defaults.

mod audio;
mod echo_cancel;
mod equalizer;
mod output_filter;
mod paths;
mod processing;
mod ui;

use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;

use log::{debug, error, info};
use serde::{Deserialize, Serialize};

pub use audio::{
    GateConfig, HpfConfig, MonitorConfig, NoiseModel, NoiseReductionConfig, StereoConfig,
    StereoMode, GATE_INTENSITY_DEFAULT, GATE_INTENSITY_MAX,
};
pub use echo_cancel::EchoCancelConfig;
pub use equalizer::{
    eq_preset_bands, eq_preset_ids, EqualizerConfig, EQ_BAND_COUNT, EQ_BAND_MAX, EQ_BAND_MIN,
};
pub use output_filter::OutputFilterSettings;
pub use paths::{
    app_id, app_version, config_dir, gettext_package, gtcrn_plugin, illustrations_dir, ladspa_dir,
    settings_file, APP_DATA_DIR, APP_ID, EQ_BANDS_HZ, GETTEXT_PACKAGE,
};
pub use processing::{CompressorConfig, CompressorDerived, GateDerived, ProcessingChain};
pub use ui::{UiConfig, WindowConfig};

/// Full settings snapshot, serialized to `settings.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub noise_reduction: NoiseReductionConfig,
    pub gate: GateConfig,
    pub compressor: CompressorConfig,
    pub hpf: HpfConfig,
    pub stereo: StereoConfig,
    pub equalizer: EqualizerConfig,
    pub window: WindowConfig,
    pub ui: UiConfig,
    pub monitor: MonitorConfig,
    pub output_filter: OutputFilterSettings,
    pub echo_cancel: EchoCancelConfig,
}

impl AppSettings {
    /// Load from the default location (`~/.config/biglinux-microphone/settings.json`).
    /// Missing file → defaults. Malformed JSON → defaults + error log.
    #[must_use]
    pub fn load() -> Self {
        let path = settings_file();
        Self::load_from(&path)
    }

    /// Load from an explicit path. Testable variant of [`AppSettings::load`].
    pub fn load_from(path: &Path) -> Self {
        let Ok(content) = fs::read_to_string(path) else {
            info!("settings: no file at {}, using defaults", path.display());
            return Self::default();
        };
        match serde_json::from_str::<Self>(&content) {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "settings: parse error at {}: {e} — falling back to defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Persist atomically. Writes to `<path>.tmp` then renames over `<path>`,
    /// calling `sync_all` between so a crash mid-write cannot produce a
    /// truncated file.
    pub fn save(&self) -> io::Result<()> {
        let path = settings_file();
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            let json = serde_json::to_vec_pretty(self)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        debug!("settings: saved to {}", path.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_round_trip_json() {
        let s = AppSettings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn missing_file_returns_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let s = AppSettings::load_from(&path);
        assert_eq!(s, AppSettings::default());
    }

    #[test]
    fn malformed_json_returns_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, b"{ not json").unwrap();
        let s = AppSettings::load_from(&path);
        assert_eq!(s, AppSettings::default());
    }

    #[test]
    fn save_is_atomic_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let original = AppSettings {
            gate: GateConfig {
                intensity: 25,
                ..GateConfig::default()
            },
            noise_reduction: NoiseReductionConfig {
                strength: 0.8,
                ..NoiseReductionConfig::default()
            },
            ..AppSettings::default()
        };
        original.save_to(&path).unwrap();

        // Temp file must be gone after successful save
        assert!(!path.with_extension("tmp").exists());

        let reloaded = AppSettings::load_from(&path);
        assert_eq!(reloaded, original);
    }

    #[test]
    fn partial_json_merges_with_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("partial.json");
        fs::write(&path, r#"{"window":{"width":900,"height":600}}"#).unwrap();

        let s = AppSettings::load_from(&path);
        assert_eq!(s.window.width, 900);
        assert_eq!(s.window.height, 600);
        assert_eq!(s.noise_reduction, NoiseReductionConfig::default());
        assert_eq!(s.stereo, StereoConfig::default());
    }
}
