//! Output filter chain — processes audio the user **hears**.
//!
//! Typical use case: a noisy video call. WirePlumber transparently
//! inserts the filter before the current default sink, so this app keeps
//! no per-application routing state.

use serde::{Deserialize, Serialize};

use super::audio::{GateConfig, HpfConfig, NoiseReductionConfig};
use super::equalizer::EqualizerConfig;
use super::processing::CompressorConfig;

/// Spatial audio is preserved unless the user explicitly chooses mono.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputChannelMode {
    #[default]
    Stereo,
    Mono,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutputFilterSettings {
    pub channel_mode: OutputChannelMode,
    pub enabled: bool,
    pub noise_reduction: NoiseReductionConfig,
    pub hpf: HpfConfig,
    pub gate: GateConfig,
    pub compressor: CompressorConfig,
    pub equalizer: EqualizerConfig,
    /// Legacy persisted target retained for settings and API compatibility.
    /// WirePlumber now follows the current default sink directly.
    #[serde(default)]
    pub target_sink_name: Option<String>,
}

impl Default for OutputFilterSettings {
    fn default() -> Self {
        Self {
            channel_mode: OutputChannelMode::Stereo,            enabled: false,
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
    fn deserializes_minimal_payload() {
        let raw = r#"{ "enabled": true }"#;
        let s: OutputFilterSettings = serde_json::from_str(raw).unwrap();
        assert!(s.enabled);
        assert!(!s.hpf.enabled);
    }

    #[test]
    fn unknown_routed_apps_field_is_rejected() {
        let raw = r#"{ "enabled": true, "routed_apps": ["Firefox"] }"#;
        let error = serde_json::from_str::<OutputFilterSettings>(raw).unwrap_err();

        assert!(error.to_string().contains("unknown field `routed_apps`"));
    }
}
