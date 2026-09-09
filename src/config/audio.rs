//! Audio-processing configuration sections.
//!
//! Each struct is a section of `settings.json`. All fields carry
//! `#[serde(default)]` so missing keys fall back to the constants below,
//! letting new fields ship without breaking existing files.

use crate::config::dynamics::GateDerived;
pub use crate::config::noise_model::{NoiseModel, deepfilter_attenuation_db};
use serde::{Deserialize, Serialize};

// ── Noise reduction (GTCRN) ──────────────────────────────────────────

// The model owner preserves the established LADSPA identifiers and serialized values.

pub const STRENGTH_DEFAULT: f32 = 1.0;
// The lookahead buffer backdates VAD decisions across speech onsets, so the
// first syllable after silence is not clipped. It also delays the microphone by
// every millisecond of it: measured through the shipped plugin at a 960-sample
// block, the noise reducer's own delay is 28.0 ms at 0 ms of lookahead, 44.0 at
// 20, 76.0 at 40 and 92.0 at the 60 this used to ask for.
//
// 20 ms is where the onset it exists to protect stops improving. Averaged over
// eight onsets that follow at least 200 ms of silence, the first 20 ms of
// speech comes out 2.47 dB below that utterance's own steady level at 0 ms of
// lookahead, 1.82 dB at 20 ms and 1.95 dB at 60 ms; on white and keyboard noise
// the three settings are indistinguishable (within 0.03 dB), and the pause
// floor does not improve either (-15.75 dB at 20 ms against -15.46 dB at 60).
// So the last 40 ms bought 48 ms of latency and nothing else.
pub const LOOKAHEAD_MS_DEFAULT: u32 = 20;
pub const MODEL_BLENDING_DEFAULT: f32 = 0.0;
// 1.0 — restore the full HF tail from the dry signal above the model's
// 8 kHz internal cutoff. Paired with `gtcrn_speech_strength` softening
// the NR during voice frames, full HF recovery preserves consonants
// and sibilance without leaking measurable extra HF noise (verified on
// the BigLinux teste.m4a benchmark — 0 dB delta vs 0.85).
pub const VOICE_RECOVERY_DEFAULT: f32 = 1.0;

/// Ratio applied to the user `strength` slider before it is sent to the
/// GTCRN `SpeechStrength` LADSPA port. The plugin's `effective_strength`
/// linearly interpolates between `Strength` (used during silence) and
/// `SpeechStrength` (used during voice) by the VAD gate. Wiring both
/// ports to the same value made the model denoise at full power during
/// speech and audibly cut voice harmonics (≈3.4 dB voice attenuation
/// vs ≈1.9 dB for DFN3 on the BigLinux teste.m4a benchmark). Halving
/// the SpeechStrength keeps full denoise during silence (where Strength
/// is the only thing the plugin sees) but softens it during voice,
/// matching DFN3's voice preservation while keeping GTCRN's CPU cost.
pub const GTCRN_SPEECH_STRENGTH_RATIO: f32 = 0.5;

/// Compute the GTCRN `SpeechStrength` port value from the unified
/// `strength` slider. Centralised so every code path that drives the
/// LADSPA chain (filter-chain conf renderer, live `Object/PARAM` push,
/// reconciler) ends up with the same number — drift between them
/// caused the v4 regression where the conf had one value and the live
/// path another.
#[must_use]
pub fn gtcrn_speech_strength(strength: f32) -> f64 {
    f64::from(strength.clamp(0.0, 1.0) * GTCRN_SPEECH_STRENGTH_RATIO)
}

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
    /// (threshold / range / hold / release) by `big-audio-effects`.
    pub intensity: u8,
}

impl GateConfig {
    /// Resolve the slider position into the plugin's threshold, range and
    /// timing values.
    ///
    /// The 0..=`GATE_INTENSITY_MAX` scale is normalised here and nowhere else:
    /// both filter-chain generators and the live control pusher have to agree
    /// on the number, or the same slider produces one gate while the graph is
    /// running and a different one after a reload.
    #[must_use]
    pub fn ladspa_controls(&self) -> GateDerived {
        GateDerived::from_unit_intensity(
            f64::from(self.intensity.min(GATE_INTENSITY_MAX)) / f64::from(GATE_INTENSITY_MAX),
        )
    }
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

// 80 Hz sits just below the lowest adult-male fundamental (~85 Hz) and
// well above HVAC/fan rumble + 50/60 Hz mains harmonics that dominate
// laptop mic noise. The chain cascades two Butterworth biquads at this
// cutoff (Linkwitz-Riley 4th order, 24 dB/oct) so the rolloff is steep
// enough to actually clean the rumble band when the user enables HPF.
pub const HPF_FREQUENCY_DEFAULT: f32 = 80.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HpfConfig {
    pub enabled: bool,
    pub frequency: f32,
}

impl Default for HpfConfig {
    fn default() -> Self {
        // HPF off by default — restored-defaults users get the AI
        // pipeline only. When the user opts in, HPF_FREQUENCY_DEFAULT
        // is the safe-for-voice cutoff documented above.
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

/// **What happens to the two channels.**
///
/// There were two more fields here once — `crossfeed_enabled` and `crossfeed_level` —
/// and nothing in this program ever read them. No pipeline stage, no command-line key, no
/// interface: a feature that existed only as two lines in everybody's settings file, which
/// is worse than a missing feature because anybody reading that file believed in it.
/// Removed rather than implemented: `#[serde(default)]` means an existing file carrying
/// the old keys still loads, and they go the next time it is written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StereoConfig {
    pub enabled: bool,
    pub mode: StereoMode,
    pub width: f32,
}

impl Default for StereoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: StereoMode::default(),
            width: STEREO_WIDTH_DEFAULT,
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
    fn deepfilter_attenuation_curve_is_quadratic() {
        // Endpoints + clamp.
        assert!((deepfilter_attenuation_db(0.0) - 0.0).abs() < 1e-6);
        assert!((deepfilter_attenuation_db(1.0) - 100.0).abs() < 1e-6);
        assert!((deepfilter_attenuation_db(-1.0) - 0.0).abs() < 1e-6);
        assert!((deepfilter_attenuation_db(2.0) - 100.0).abs() < 1e-6);
        // Quadratic midpoints — give the slider perceptible travel
        // (linear `s*100` saturates DFN3 above s≈0.1 in practice).
        assert!((deepfilter_attenuation_db(0.5) - 25.0).abs() < 1e-4);
        assert!((deepfilter_attenuation_db(0.25) - 6.25).abs() < 1e-4);
    }

    #[test]
    fn noise_model_ladspa_control_value() {
        assert!((NoiseModel::GtcrnDns3.ladspa_control() - 0.0).abs() < f32::EPSILON);
        assert!((NoiseModel::GtcrnVctk.ladspa_control() - 1.0).abs() < f32::EPSILON);
    }
}
