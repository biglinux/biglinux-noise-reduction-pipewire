//! Derived LADSPA control values for the gate and compressor.
//!
//! The UI exposes a single intensity slider per effect. Plugins expect
//! several tuned parameters (threshold, range, attack, release, …) so this
//! module projects the user intensity onto a coherent parameter set using
//! curve shapes tuned for voice capture.

use serde::{Deserialize, Serialize};

use super::audio::{GateConfig, GATE_INTENSITY_MAX};

// ── Gate-derived parameters ──────────────────────────────────────────

/// Pre-derived LADSPA parameters for the gate plugin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateDerived {
    pub threshold_db: f64,
    pub range_db: f64,
    pub attack_ms: f64,
    pub hold_ms: f64,
    pub release_ms: f64,
}

impl GateDerived {
    #[must_use]
    pub fn from_intensity(intensity: u8) -> Self {
        let n = f64::from(intensity.min(GATE_INTENSITY_MAX)) / f64::from(GATE_INTENSITY_MAX);
        let sqrt_n = n.sqrt();
        // Curve calibrated against broadcast/podcast voice-gate
        // references (Cockos ReaGate "voice" preset, OBS noise-gate
        // recommended defaults, Adobe Audition "voice gate" template):
        //
        // - attack 2 ms — fast enough to not clip plosives but slow
        //   enough to avoid clicking on near-threshold transients.
        // - threshold -55..-30 dB — covers room tone (~-55) up to a
        //   moderately noisy laptop fan environment (~-30). Going
        //   above -30 starts cutting soft voice tails.
        // - range -18..-40 dB — leaves a small ambience floor so the
        //   transition feels natural; full -60 attenuation makes the
        //   gate audibly "chop" between words.
        // - hold 150..100 ms — long enough to bridge a single
        //   syllable break, short enough to release after a sentence.
        // - release 250..150 ms — covers a typical voice tail decay
        //   without truncating sibilants.
        Self {
            threshold_db: -55.0 + sqrt_n * 25.0,
            range_db: -18.0 - sqrt_n * 22.0,
            attack_ms: 2.0,
            hold_ms: 150.0 - sqrt_n * 50.0,
            release_ms: 250.0 - sqrt_n * 100.0,
        }
    }

    #[must_use]
    pub fn from_config(cfg: &GateConfig) -> Self {
        Self::from_intensity(cfg.intensity)
    }
}

// ── Compressor config + derived parameters ───────────────────────────

pub const COMPRESSOR_INTENSITY_DEFAULT: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressorConfig {
    pub enabled: bool,
    pub intensity: f32,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: COMPRESSOR_INTENSITY_DEFAULT,
        }
    }
}

/// Derived LADSPA parameters for the SC4 mono compressor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressorDerived {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_gain_db: f32,
    pub knee_db: f32,
    pub rms_peak: f32,
}

impl CompressorDerived {
    #[must_use]
    pub fn from_intensity(intensity: f32) -> Self {
        let i = intensity.clamp(0.0, 1.0);
        Self {
            threshold_db: -15.0 - i * 15.0,
            ratio: 2.0 + i * 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            makeup_gain_db: 2.0 + i * 8.0,
            knee_db: 3.0 + i * 5.0,
            rms_peak: 0.0,
        }
    }

    #[must_use]
    pub fn from_config(cfg: &CompressorConfig) -> Self {
        Self::from_intensity(cfg.intensity)
    }
}

// ── Convenience bundle ───────────────────────────────────────────────

pub struct ProcessingChain {
    pub gate: GateDerived,
    pub compressor: CompressorDerived,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_minimum_intensity_produces_base_curve() {
        let g = GateDerived::from_intensity(0);
        assert!((g.threshold_db - -55.0).abs() < 1e-9);
        assert!((g.range_db - -18.0).abs() < 1e-9);
        assert!((g.attack_ms - 2.0).abs() < 1e-9);
        assert!((g.hold_ms - 150.0).abs() < 1e-9);
        assert!((g.release_ms - 250.0).abs() < 1e-9);
    }

    #[test]
    fn gate_maximum_intensity_saturates_curve() {
        let g = GateDerived::from_intensity(50);
        assert!((g.threshold_db - -30.0).abs() < 1e-9);
        assert!((g.range_db - -40.0).abs() < 1e-9);
        assert!((g.hold_ms - 100.0).abs() < 1e-9);
        assert!((g.release_ms - 150.0).abs() < 1e-9);
    }

    #[test]
    fn gate_intensity_clamped_above_max() {
        let high = GateDerived::from_intensity(200);
        assert!((high.threshold_db - -30.0).abs() < 1e-9);
    }

    #[test]
    fn compressor_intensity_is_clamped() {
        let low = CompressorDerived::from_intensity(-1.0);
        assert!((low.ratio - 2.0).abs() < f32::EPSILON);

        let high = CompressorDerived::from_intensity(2.0);
        assert!((high.ratio - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn compressor_midpoint_known() {
        let mid = CompressorDerived::from_intensity(0.5);
        assert!((mid.threshold_db - -22.5).abs() < f32::EPSILON);
        assert!((mid.ratio - 4.0).abs() < f32::EPSILON);
        assert!((mid.makeup_gain_db - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn compressor_config_defaults() {
        let c = CompressorConfig::default();
        assert!(!c.enabled);
        assert!((c.intensity - COMPRESSOR_INTENSITY_DEFAULT).abs() < f32::EPSILON);
    }
}
