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

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EchoCancelConfig {
    pub enabled: bool,
}

impl Default for EchoCancelConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_enabled() {
        assert!(EchoCancelConfig::default().enabled);
    }

    #[test]
    fn round_trips_through_json() {
        let c = EchoCancelConfig { enabled: false };
        let s = serde_json::to_string(&c).unwrap();
        let back: EchoCancelConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn empty_object_falls_back_to_default() {
        let c: EchoCancelConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(c, EchoCancelConfig::default());
    }
}
