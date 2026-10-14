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
mod gain_safety;
pub use gain_safety::GainSafety;
pub mod noise_model;
mod output_filter;
mod paths;
mod processing;
mod profiles;
mod runtime;
pub use runtime::RuntimeConfig;
pub mod storage;
mod ui;

use std::io;
use std::path::Path;

use log::{debug, error};
use serde::{Deserialize, Serialize};
use std::fs::read_to_string;

pub use audio::{
    CALIBRATION_TARGET_DBFS, DeviceCalibration, GATE_INTENSITY_DEFAULT, GATE_INTENSITY_MAX,
    GateConfig, HPF_FREQUENCY_DEFAULT, HpfConfig, MicCalibrationConfig, MonitorConfig, NoiseModel,
    NoiseReductionConfig, SILENCE_ANCHOR_HZ, SILENCE_ANCHOR_PORTS, SILENCE_ANCHOR_SENTINEL,
    SILENCE_ANCHORS, SILENCE_DEPTH_DEFAULT, SILENCE_DEPTH_MAX, StereoConfig, StereoMode,
    VOICE_GATE_DEPTH_MAX, VOICE_LEVEL_MARGIN_DB, VOICE_SENSITIVITY_MAX, VOICE_TILT_MARGIN_DB,
    VoiceEvidence, VoiceProfile,
};
pub use echo_cancel::{EchoCancelConfig, EchoMode};
pub use equalizer::{
    EQ_BAND_COUNT, EQ_BAND_MAX, EQ_BAND_MIN, EqualizerConfig, eq_preset_bands, eq_preset_ids,
};
pub use output_filter::{OutputChannelMode, OutputFilterSettings};
pub use paths::{
    APP_DATA_DIR, APP_ID, EQ_BANDS_HZ, GETTEXT_PACKAGE, app_id, app_version, config_dir,
    gettext_package, illustrations_dir, settings_file,
};
pub use processing::CompressorConfig;
pub use profiles::{Profile, ProfileAudio, ProfilesConfig};
pub use ui::{UiConfig, WindowConfig};

/// Full settings snapshot, serialized to `settings.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub runtime: RuntimeConfig,
    pub gain_safety: GainSafety,
    /// Temporary master bypass; individual effect preferences are retained.
    pub mic_bypass: bool,
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
    /// What each microphone's calibration pass settled on. Empty until somebody
    /// runs one; nothing here changes by itself.
    pub mic_calibration: MicCalibrationConfig,
    /// Saved scenarios: a whole set of preferences under a name.
    pub profiles: ProfilesConfig,
}

impl AppSettings {
    pub fn set_microphone_enabled(&mut self, enabled: bool) {
        self.mic_bypass = !enabled;
        if enabled && !crate::pipeline::mic_chain_wanted(self) {
            self.noise_reduction.enabled = true;
        }
    }

    /// Materialize effective flags without destroying the saved choices.
    #[must_use]
    pub fn runtime_settings(&self) -> Self {
        let mut effective = self.clone();
        if self.mic_bypass {
            crate::pipeline::cascade_mic_off(&mut effective);
        }
        // The playback chain carries notifications and music as well as voices; a
        // gate that mutes everything but speech does not belong there.
        effective.output_filter.noise_reduction.voice_gate = false;
        effective.output_filter.noise_reduction.dpdfnet_voice_gate = false;
        effective
    }

    /// Strict transaction read: migration, validation and normalization use
    /// one byte snapshot. Malformed files are never converted to defaults.
    pub fn load_strict() -> io::Result<Self> {
        Self::load_from_strict(&settings_file())
    }

    pub fn load_from_strict(path: &Path) -> io::Result<Self> {
        let content = match read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if !value.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "settings must be a JSON object",
            ));
        }
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
        if let Some(output) = value
            .get_mut("output_filter")
            .and_then(serde_json::Value::as_object_mut)
        {
            output.remove("routed_apps");
        }
        let mut settings: Self = serde_json::from_value(value)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        settings.equalizer.normalize();
        settings.output_filter.equalizer.normalize();
        settings.window = settings.window.sanitized();
        Ok(settings)
    }

    #[must_use]
    pub fn load() -> Self {
        Self::load_from(&settings_file())
    }

    /// Read-only compatibility entry point. Writers use load_strict instead.
    pub fn load_from(path: &Path) -> Self {
        Self::load_from_strict(path).unwrap_or_else(|error| {
            error!("settings: could not read {}: {error}", path.display());
            Self::default()
        })
    }

    /// Persist atomically through the shared storage boundary so a crash
    /// mid-write cannot produce a truncated file.
    pub fn save(&self) -> io::Result<()> {
        let path = settings_file();
        self.save_to(&path)
    }

    /// Writing identical content is skipped, not repeated. `biglinux-microphone.path`
    /// wakes the reconciler on every change to this file, and the reconciler saves
    /// as part of applying — so a save that rewrote unchanged bytes would wake it
    /// again, and again.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        let json = storage::serialized_preserving_unknown(self, path)?;
        if !path.is_symlink() && std::fs::read(path).is_ok_and(|existing| existing == json) {
            return Ok(());
        }
        atomic_write_private(path, &json)?;
        debug!("settings: saved to {}", path.display());
        Ok(())
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
    fn a_file_naming_a_removed_model_loads_the_default_model_and_keeps_the_rest() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        atomic_write_private(
            &path,
            br#"{"quality":"best","noise_reduction":{"model":4,"strength":0.5}}"#,
        )
        .unwrap();
        let settings = AppSettings::load_from_strict(&path).unwrap();
        assert_eq!(settings.noise_reduction.model, NoiseModel::default());
        assert!((settings.noise_reduction.strength - 0.5).abs() < f32::EPSILON);
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
