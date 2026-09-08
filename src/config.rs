//! Persistent application settings.
//!
//! Model split by concern; `AppSettings` aggregates every section and
//! provides the JSON load/save entry points. The on-disk file lives at
//! `~/.config/biglinux-microphone/settings.json` and is written atomically
//! (write-to-temp + fsync + rename) so a crashed save cannot leave a
//! truncated file.
//!
//! BigLinux-only target: always-latest PipeWire + WirePlumber 0.5+. Settings
//! use the current schema; malformed or unsupported files fall back to
//! defaults.

mod audio;
pub mod dynamics;
mod echo_cancel;
mod equalizer;
pub mod noise_model;
mod output_filter;
mod paths;
/// Timing a shipped plugin on this machine, so the quality policy has a measured
/// number instead of a proxy for one.
pub mod plugin_cost;
mod processing;
mod quality;
pub mod storage;
mod ui;

use std::io;
use std::path::Path;

use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::fs::read_to_string;

pub use audio::{
    GATE_INTENSITY_DEFAULT, GATE_INTENSITY_MAX, GateConfig, HPF_FREQUENCY_DEFAULT, HpfConfig,
    MonitorConfig, NoiseModel, NoiseReductionConfig, StereoConfig, StereoMode,
    deepfilter_attenuation_db, gtcrn_speech_strength,
};
pub use echo_cancel::{EchoCancelConfig, EchoMode};
pub use equalizer::{
    EQ_BAND_COUNT, EQ_BAND_MAX, EQ_BAND_MIN, EqualizerConfig, eq_preset_bands, eq_preset_ids,
};
pub use output_filter::OutputFilterSettings;
pub use paths::{
    APP_DATA_DIR, APP_ID, EQ_BANDS_HZ, GETTEXT_PACKAGE, app_id, app_version, config_dir,
    gettext_package, gtcrn_plugin, illustrations_dir, settings_file,
};
pub use processing::CompressorConfig;
pub use quality::{Machine, Quality};
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
    /// §38's choice: how much of this machine the noise models may cost.
    ///
    /// Defaulted rather than required, so a settings file written before this existed
    /// loads as "automatic" instead of failing and taking every other setting with it.
    #[serde(default)]
    pub quality: Quality,
}

impl AppSettings {
    pub fn load_strict() -> io::Result<Self> {
        let path = settings_file();
        match std::fs::read(&path) {
            Ok(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if !value.is_object() {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "settings must be a JSON object"));
                }
                // Validate the schema before using the compatibility loader.
                // Old quality spelling is accepted by its serde alias.
                serde_json::from_value::<Self>(value).map_err(|error|
                    io::Error::new(io::ErrorKind::InvalidData, error))?;
                Ok(Self::load_from(&path))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    /// Load from the default location (`~/.config/biglinux-microphone/settings.json`).
    /// Missing file → defaults. Malformed JSON → defaults + error log.
    #[must_use]
    pub fn load() -> Self {
        let path = settings_file();
        Self::load_from(&path)
    }

    /// Load from an explicit path. Testable variant of [`AppSettings::load`].
    pub fn load_from(path: &Path) -> Self {
        let content = match read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                info!("settings: no file at {}, using defaults", path.display());
                return Self::default();
            }
            Err(e) => {
                error!(
                    "settings: read error at {}: {e} — falling back to defaults",
                    path.display()
                );
                return Self::default();
            }
        };
        let parsed = serde_json::from_str::<serde_json::Value>(&content).and_then(|mut value| {
            // Older standalone settings only had the switch. An explicit off
            // remains off when the automatic mode is introduced.
            if let Some(echo) = value
                .get_mut("echo_cancel")
                .and_then(serde_json::Value::as_object_mut)
                && !echo.contains_key("mode")
                && echo.get("enabled") == Some(&serde_json::Value::Bool(false))
            {
                echo.insert("mode".into(), "off".into());
            }
            if value.get("quality").is_none()
                && value
                    .get("noise_reduction")
                    .and_then(|noise| noise.get("model"))
                    .is_some()
                && let Some(settings) = value.as_object_mut()
            {
                settings.insert("quality".into(), "manual".into());
            }
            if let Some(output) = value
                .get_mut("output_filter")
                .and_then(serde_json::Value::as_object_mut)
            {
                output.remove("routed_apps");
            }
            serde_json::from_value::<Self>(value)
        });
        match parsed {
            Ok(mut s) => {
                s.equalizer.normalize();
                s.output_filter.equalizer.normalize();
                s.window = s.window.sanitized();
                s.demote_unavailable_models();
                s
            }
            Err(e) => {
                error!(
                    "settings: parse error at {}: {e} — falling back to defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Settings may persist an optional model from a previous run while
    /// its LADSPA package has since been uninstalled — or left present
    /// but unloadable (see [`NoiseModel::plugin_loadable_cached`]). Demote silently
    /// to the default GTCRN variant so the rendered filter-chain
    /// doesn't reference a broken .so.
    fn demote_unavailable_models(&mut self) {
        self.noise_reduction.model = available_or_default(self.noise_reduction.model);
        self.output_filter.noise_reduction.model =
            available_or_default(self.output_filter.noise_reduction.model);
    }

    /// Put §38's choice into effect: pick the model and write it into both chains.
    ///
    /// Called where the graph is about to be built, never on load — a policy that ran on
    /// every read would move the model while somebody was looking at the list of them.
    /// Leaves a hand-picked model alone, which is what `Quality::Manual` means.
    ///
    /// The machine is read here rather than by the caller, and only past the
    /// guard: `Machine::read` walks `/sys/class/power_supply` and, the first
    /// time it sees a plugin, forks `measure-model` and blocks for seconds.
    /// A `Manual` install chose its model by hand and must pay none of that,
    /// yet all three callers used to read it before asking.
    pub fn settle_quality(&mut self) {
        if !self.quality.decides_the_model() {
            return;
        }
        let machine = Machine::read(self.filters_running());
        let wanted = quality::loadable(quality::choose(
            self.quality,
            &machine,
            Some(self.noise_reduction.model),
        ));
        self.noise_reduction.model = wanted;
        self.output_filter.noise_reduction.model = wanted;
    }

    /// How many chains run a noise model at once, which is one of §38's inputs and the
    /// only one that comes from the settings themselves.
    ///
    /// The echo canceller is deliberately not counted. It used to be, and that made the
    /// heavier model unreachable on every ordinary setup: the mic filter and the echo
    /// canceller are both on by default, the policy saw two chains, and two is already
    /// its reason to step down. Nobody with echo cancellation on ever got the better
    /// model, whatever their machine could do.
    ///
    /// It is also the wrong thing to count. The step-down exists because two neural
    /// models on one graph compete for the same core. WebRTC's canceller is a fixed
    /// filter that costs a fraction of one, and it is part of the same microphone path
    /// rather than a second demand on it.
    #[must_use]
    pub fn filters_running(&self) -> usize {
        usize::from(self.noise_reduction.enabled) + usize::from(self.output_filter.enabled)
    }

    /// Persist atomically through the shared storage boundary so a crash
    /// mid-write cannot produce a truncated file.
    pub fn save(&self) -> io::Result<()> {
        let path = settings_file();
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        let json = storage::serialized_preserving_unknown(self, path)?;
        if std::fs::read(path).is_ok_and(|existing| existing == json) {
            return Ok(());
        }
        atomic_write_private(path, &json)?;
        debug!("settings: saved to {}", path.display());
        Ok(())
    }
}

fn available_or_default(model: NoiseModel) -> NoiseModel {
    if model == NoiseModel::default() || model.plugin_loadable_cached() {
        model
    } else {
        NoiseModel::default()
    }
}

pub(crate) fn atomic_write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use gio::prelude::*;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    gio::File::for_path(path)
        .replace_contents(
            bytes,
            None,
            false,
            gio::FileCreateFlags::PRIVATE | gio::FileCreateFlags::REPLACE_DESTINATION,
            gio::Cancellable::NONE,
        )
        .map_err(io::Error::other)?;
    std::fs::File::open(path)?.sync_all()?;
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
        atomic_write_private(&path, b"{ not json").unwrap();
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

        let reloaded = AppSettings::load_from(&path);
        assert_eq!(reloaded, original);
    }

    #[test]
    fn partial_json_merges_with_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("partial.json");
        atomic_write_private(&path, br#"{"window":{"width":900,"height":600}}"#).unwrap();

        let s = AppSettings::load_from(&path);
        assert_eq!(s.window.width, 900);
        assert_eq!(s.window.height, 600);
        assert_eq!(s.noise_reduction, NoiseReductionConfig::default());
        assert_eq!(s.stereo, StereoConfig::default());
    }

    #[test]
    fn load_sanitizes_persisted_values() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        atomic_write_private(
            &path,
            br#"{
                "window": {"width": 1, "height": 2},
                "equalizer": {"bands": [100, -100, 0, 0, 0, 0, 0, 0, 0, 0]},
                "output_filter": {"equalizer": {"bands": [1, 2, 3]}}
            }"#,
        )
        .unwrap();

        let s = AppSettings::load_from(&path);
        assert_eq!(s.window.width, ui::WINDOW_WIDTH_MIN);
        assert_eq!(s.window.height, ui::WINDOW_HEIGHT_MIN);
        assert_eq!(s.equalizer.bands[0], EQ_BAND_MAX);
        assert_eq!(s.equalizer.bands[1], EQ_BAND_MIN);
        assert_eq!(
            s.output_filter.equalizer.bands,
            EqualizerConfig::default().bands
        );
    }
}
