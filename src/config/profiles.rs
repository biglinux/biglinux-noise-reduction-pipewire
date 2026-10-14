//! Scenarios: a whole set of preferences under a name somebody chose.
//!
//! "Live", "meeting", "recording" are not three microphones — they are three
//! opinions about the same one. So a profile carries the preferences and the
//! device it would like, and deliberately does NOT carry two things:
//!
//! * **the calibration.** `DeviceCalibration` is a measurement of one
//!   microphone in one room. Letting "live" and "meeting" each remember their
//!   own capture volume for the same device would let two scenarios disagree
//!   about a measured fact, and the one that was applied last would win.
//! * **`runtime`** (fast-CPU preference, memory reservation). Changing those
//!   forces a full restart of every chain; switching scenario must not be able
//!   to do that by accident.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{
    CompressorConfig, EchoCancelConfig, EqualizerConfig, GainSafety, GateConfig, HpfConfig,
    NoiseReductionConfig, OutputFilterSettings, StereoConfig,
};

/// The audio preferences a scenario remembers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileAudio {
    pub mic_bypass: bool,
    pub noise_reduction: NoiseReductionConfig,
    pub gate: GateConfig,
    pub compressor: CompressorConfig,
    pub hpf: HpfConfig,
    pub stereo: StereoConfig,
    pub equalizer: EqualizerConfig,
    pub output_filter: OutputFilterSettings,
    pub echo_cancel: EchoCancelConfig,
    pub gain_safety: GainSafety,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    /// What the person typed, as they typed it.
    pub label: String,
    /// RFC3339, so a list can be ordered by "what did I touch last".
    pub updated: String,
    /// The microphone this scenario wants, by PipeWire `node.name`. `None`
    /// means the scenario has no opinion — which is the right answer for
    /// "meeting" on a laptop that meets in different rooms.
    pub microphone: Option<String>,
    /// Kept beside the name so a missing device can be named in a sentence
    /// rather than shown as an `alsa_input.usb-…` identifier.
    pub microphone_description: Option<String>,
    pub audio: ProfileAudio,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfilesConfig {
    /// The scenario currently applied, if the settings still match one.
    pub active: Option<String>,
    pub profiles: BTreeMap<String, Profile>,
}

impl ProfilesConfig {
    /// A name a person typed, as a key that survives a settings file.
    #[must_use]
    pub fn slug(label: &str) -> String {
        let slug: String = label
            .trim()
            .to_lowercase()
            .chars()
            // Unicode, not ASCII: "Reunião" is a name somebody typed, and
            // turning it into "reuni-o" makes the key look like a bug.
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        let slug = slug.trim_matches('-').replace("--", "-");
        if slug.is_empty() {
            "scenario".into()
        } else {
            slug
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_name_becomes_a_key_that_survives_a_file() {
        assert_eq!(ProfilesConfig::slug("Live"), "live");
        assert_eq!(
            ProfilesConfig::slug("  Reunião de equipe "),
            "reunião-de-equipe"
        );
        assert_eq!(ProfilesConfig::slug("///"), "scenario");
    }

    #[test]
    fn a_scenario_round_trips_through_the_settings_file() {
        let mut config = ProfilesConfig::default();
        config.profiles.insert(
            "live".into(),
            Profile {
                label: "Live".into(),
                updated: "2026-09-18T00:00:00Z".into(),
                microphone: Some("alsa_input.usb-Yeti".into()),
                microphone_description: Some("Blue Yeti".into()),
                audio: ProfileAudio::default(),
            },
        );
        config.active = Some("live".into());
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(
            serde_json::from_str::<ProfilesConfig>(&json).unwrap(),
            config
        );
    }

    #[test]
    fn a_scenario_cannot_carry_a_calibration() {
        // The guard is structural: there is nowhere to put one. If somebody
        // adds a field for it, this test is where they have to argue for it.
        let json = serde_json::to_string(&ProfileAudio::default()).unwrap();
        assert!(!json.contains("calibration"), "{json}");
        assert!(!json.contains("runtime"), "{json}");
    }
}
