//! Output filter chain — processes audio the user **hears**.
//!
//! Typical use case: a noisy video call. The output filter sink is
//! exposed as a regular PipeWire `Audio/Sink` virtual device. The user
//! routes specific apps to it through the standard system audio panel
//! (KDE/GNOME volume mixer), exactly like any other sink — no per-app
//! state is maintained inside this app.

use serde::{Deserialize, Serialize};

use super::audio::{GateConfig, HpfConfig, NoiseReductionConfig};
use super::equalizer::EqualizerConfig;
use super::processing::CompressorConfig;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputFilterSettings {
    pub enabled: bool,
    pub noise_reduction: NoiseReductionConfig,
    pub hpf: HpfConfig,
    pub gate: GateConfig,
    pub compressor: CompressorConfig,
    pub equalizer: EqualizerConfig,
    /// `node.name` of the hardware sink the output filter plays into.
    /// Captured by the reconciler the first time the user enables the
    /// output filter (snapshot of the system default sink right before
    /// `output-biglinux` is promoted), so the playback side of the
    /// filter chain has a stable target to forward processed audio to —
    /// without it, `node.passive` would follow the new default and
    /// loop back into us.
    #[serde(default)]
    pub target_sink_name: Option<String>,
}

impl Default for OutputFilterSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            noise_reduction: NoiseReductionConfig {
                enabled: true,
                ..NoiseReductionConfig::default()
            },
            hpf: HpfConfig {
                enabled: false,
                frequency: 40.0,
            },
            gate: GateConfig::default(),
            compressor: CompressorConfig {
                enabled: false,
                intensity: 0.0,
            },
            equalizer: EqualizerConfig::default(),
            target_sink_name: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_disabled_effects() {
        let s = OutputFilterSettings::default();
        assert!(!s.enabled);
        assert!(!s.gate.enabled);
        assert!(!s.compressor.enabled);
        assert!(!s.equalizer.enabled);
        assert!(!s.hpf.enabled);
    }

    #[test]
    fn deserializes_minimal_payload() {
        let raw = r#"{ "enabled": true }"#;
        let s: OutputFilterSettings = serde_json::from_str(raw).unwrap();
        assert!(s.enabled);
        assert!(!s.hpf.enabled);
    }

    #[test]
    fn legacy_routed_apps_field_is_ignored() {
        // Existing settings.json files from the per-app era include a
        // `routed_apps` array. Serde must accept and discard it.
        let raw = r#"{ "enabled": true, "routed_apps": ["Firefox"] }"#;
        let s: OutputFilterSettings = serde_json::from_str(raw).unwrap();
        assert!(s.enabled);
    }
}
