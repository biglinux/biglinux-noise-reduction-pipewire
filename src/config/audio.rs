//! Audio-processing configuration sections.
//!
//! Each struct is a section of `settings.json`. All fields carry
//! `#[serde(default)]` so missing keys fall back to the constants below,
//! letting new fields ship without breaking existing files.

use serde::{Deserialize, Serialize};

// ── Noise reduction (GTCRN) ──────────────────────────────────────────

/// GTCRN model variant. The numeric value is forwarded as the LADSPA
/// `model` control input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum NoiseModel {
    /// Strongest noise reduction, highest quality.
    #[default]
    GtcrnDns3 = 0,
    /// Gentler noise reduction, lower CPU cost.
    GtcrnVctk = 1,
}

impl TryFrom<u8> for NoiseModel {
    type Error = String;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Self::GtcrnDns3),
            1 => Ok(Self::GtcrnVctk),
            other => Err(format!("unknown NoiseModel value: {other}")),
        }
    }
}

impl From<NoiseModel> for u8 {
    fn from(m: NoiseModel) -> Self {
        m as Self
    }
}

impl NoiseModel {
    /// LADSPA control value for the GTCRN plugin's `model` input.
    #[must_use]
    pub fn ladspa_control(self) -> f32 {
        f32::from(u8::from(self))
    }
}

pub const STRENGTH_DEFAULT: f32 = 1.0;
// 60 ms gives the lookahead buffer enough frames to backdate VAD decisions
// across speech onsets, avoiding clipped first syllables after silence.
pub const LOOKAHEAD_MS_DEFAULT: u32 = 60;
pub const MODEL_BLENDING_DEFAULT: f32 = 0.0;
// 0.85 reconstructs more high-frequency content (consonants, sibilance)
// than 0.75 while still cutting noise above 8 kHz.
pub const VOICE_RECOVERY_DEFAULT: f32 = 0.85;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseReductionConfig {
    pub enabled: bool,
    pub model: NoiseModel,
    /// Single intensity control. Drives both the GTCRN `Strength` and
    /// `SpeechStrength` LADSPA ports — separating them only made sense
    /// while two GTCRN instances ran in parallel; the unified UI is the
    /// canonical surface so the data model now matches it.
    pub strength: f32,
    pub lookahead_ms: u32,
    pub model_blending: f32,
    pub voice_recovery: f32,
}

impl Default for NoiseReductionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: NoiseModel::default(),
            strength: STRENGTH_DEFAULT,
            lookahead_ms: LOOKAHEAD_MS_DEFAULT,
            model_blending: MODEL_BLENDING_DEFAULT,
            voice_recovery: VOICE_RECOVERY_DEFAULT,
        }
    }
}

// ── Gate (silence filter) ────────────────────────────────────────────

// 30 sits in the middle of the calibrated curve (threshold ≈ -36 dB,
// range ≈ -35 dB, hold ≈ 110 ms, release ≈ 175 ms) — enough to clamp
// keyboard/fan noise without chopping voice tails the moment the user
// flips it on. Stored even while disabled so the UI slider shows a
// useful starting point.
pub const GATE_INTENSITY_DEFAULT: u8 = 30;
pub const GATE_INTENSITY_MAX: u8 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateConfig {
    pub enabled: bool,
    /// Intensity scale 0..=50. Mapped to LADSPA parameters
    /// (threshold / range / hold / release) by
    /// [`super::ProcessingChain`].
    pub intensity: u8,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: GATE_INTENSITY_DEFAULT,
        }
    }
}

// ── High-pass filter (rumble removal) ────────────────────────────────

pub const HPF_FREQUENCY_DEFAULT: f32 = 40.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HpfConfig {
    pub enabled: bool,
    pub frequency: f32,
}

impl Default for HpfConfig {
    fn default() -> Self {
        // HPF off by default — many laptop mics already roll off below
        // ~80 Hz and adding another bite there made the restored-defaults
        // preset sound thin.
        Self {
            enabled: false,
            frequency: HPF_FREQUENCY_DEFAULT,
        }
    }
}

// ── Stereo enhancement (mic only) ────────────────────────────────────

/// Stereo processing mode applied to the captured microphone signal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StereoMode {
    #[default]
    Mono,
    DualMono,
    VoiceChanger,
}

pub const STEREO_WIDTH_DEFAULT: f32 = 0.7;
pub const CROSSFEED_DEFAULT: f32 = 0.3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StereoConfig {
    pub enabled: bool,
    pub mode: StereoMode,
    pub width: f32,
    pub crossfeed_enabled: bool,
    pub crossfeed_level: f32,
}

impl Default for StereoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: StereoMode::default(),
            width: STEREO_WIDTH_DEFAULT,
            crossfeed_enabled: false,
            crossfeed_level: CROSSFEED_DEFAULT,
        }
    }
}

// ── Monitor (headphone passthrough, mic-only option) ─────────────────

pub const MONITOR_DELAY_MS_DEFAULT: u32 = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorConfig {
    pub enabled: bool,
    pub delay_ms: u32,
    pub volume: f32,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            delay_ms: MONITOR_DELAY_MS_DEFAULT,
            volume: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_mode_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&StereoMode::DualMono).unwrap(),
            r#""dual_mono""#
        );
        assert_eq!(
            serde_json::to_string(&StereoMode::VoiceChanger).unwrap(),
            r#""voice_changer""#
        );
    }

    #[test]
    fn stereo_mode_rejects_unknown_string() {
        assert!(serde_json::from_str::<StereoMode>(r#""squirrel""#).is_err());
    }

    #[test]
    fn noise_model_round_trips_as_number() {
        assert_eq!(serde_json::to_string(&NoiseModel::GtcrnVctk).unwrap(), "1");
        let back: NoiseModel = serde_json::from_str("0").unwrap();
        assert_eq!(back, NoiseModel::GtcrnDns3);
    }

    #[test]
    fn noise_model_rejects_out_of_range() {
        let err = serde_json::from_str::<NoiseModel>("99").unwrap_err();
        assert!(err.to_string().contains("unknown NoiseModel"));
    }

    #[test]
    fn noise_reduction_defaults() {
        let c = NoiseReductionConfig::default();
        assert!(c.enabled);
        assert_eq!(c.model, NoiseModel::GtcrnDns3);
        assert!((c.strength - STRENGTH_DEFAULT).abs() < f32::EPSILON);
    }

    #[test]
    fn noise_model_ladspa_control_value() {
        assert!((NoiseModel::GtcrnDns3.ladspa_control() - 0.0).abs() < f32::EPSILON);
        assert!((NoiseModel::GtcrnVctk.ladspa_control() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn gate_intensity_default_matches_constant() {
        assert_eq!(GateConfig::default().intensity, GATE_INTENSITY_DEFAULT);
    }

    #[test]
    fn gate_intensity_rejects_overflow_in_json() {
        // serde enforces u8 bounds on deserialization
        let err =
            serde_json::from_str::<GateConfig>(r#"{"enabled":true,"intensity":500}"#).unwrap_err();
        assert!(err.to_string().to_ascii_lowercase().contains("invalid"));
    }
}
