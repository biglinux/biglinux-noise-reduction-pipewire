//! Acoustic echo cancellation settings.
//!
//! When enabled, a separate PipeWire process loads
//! `libpipewire-module-echo-cancel` with the WebRTC AEC backend and
//! exposes a virtual source called `echo-cancel-source`. The mic
//! filter-chain then pulls from that source instead of the default
//! hardware mic, so speaker echo never reaches the GTCRN denoiser.
//!
//! Only `enabled` is user-facing — every WebRTC AEC tunable is fixed:
//!
//! - `noise_suppression = false` — GTCRN handles spectral denoising
//!   later in the chain; running WebRTC's NS first would over-process.
//! - `high_pass_filter = false` — our biquad HPF (40 Hz default) does
//!   the same job in the mic chain.
//! - `gain_control = false` — keep AEC as cancellation only; the mic
//!   chain handles level/voice shaping downstream, and AGC can amplify
//!   residual echo before GTCRN sees it.
//! - `voice_detection = true`, `delay_agnostic = true`,
//!   `extended_filter = true` — quality boosts with no toggle benefit.
//!
//! Defaulting to `true` covers the most common scenario (laptop user on
//! a call without headphones) without forcing the user to dig through
//! Advanced. Power users with headphones or well-isolated microphones
//! can disable it from the Advanced view.
//!
//! §78 asks for that choice to be contextual rather than a switch: cancellation is worth
//! its cost when the microphone can hear the speakers, and worth nothing when the sound is
//! going to headphones. So `mode` carries what somebody asked for and `enabled` stays what
//! the pipeline reads — the two are separate because "automatic" is not a state the chain
//! can be in, it is a rule about which state to be in. Whoever evaluates the rule writes
//! `enabled`; this file only remembers that the rule is what was asked for.

use serde::{Deserialize, Serialize};

/// What somebody asked for (§78).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EchoMode {
    /// Cancel when the microphone can hear the speakers, and not otherwise.
    #[default]
    #[serde(rename = "auto")]
    Automatic,
    #[serde(rename = "on")]
    Always,
    #[serde(rename = "off")]
    Never,
}

impl EchoMode {
    /// Parse the word a command line or a settings file carries.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "auto" | "automatic" => Self::Automatic,
            "on" | "true" | "yes" => Self::Always,
            "off" | "false" | "no" => Self::Never,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EchoCancelConfig {
    /// What the chain is doing. The pipeline reads this and nothing else.
    pub enabled: bool,
    /// What was asked for (§78). Defaulted, so a settings file written before this
    /// existed loads as automatic rather than failing and taking the rest with it.
    #[serde(default)]
    pub mode: EchoMode,
}

impl Default for EchoCancelConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: EchoMode::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_json_configuration_falls_back_to_default() {
        let c: EchoCancelConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(c, EchoCancelConfig::default());
    }
}
