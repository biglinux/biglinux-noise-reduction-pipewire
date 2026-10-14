//! Audio-processing configuration sections.
//!
//! Each struct is a section of `settings.json`. All fields carry
//! `#[serde(default)]` so missing keys fall back to the constants below,
//! letting new fields ship without breaking existing files.

use std::collections::BTreeMap;

use crate::config::dynamics::GateDerived;
pub use crate::config::noise_model::{NoiseModel, deepfilter_attenuation_db};
use serde::{Deserialize, Serialize};

// ── Noise reduction ──────────────────────────────────────────────────

pub const STRENGTH_DEFAULT: f32 = 1.0;
/// Attenuation of the DeepFilterNet3 voice gate while nobody speaks, in dB.
pub const VOICE_GATE_DEPTH_DEFAULT: u8 = 40;
/// 60 dB and more mutes completely; the plugin accepts more, which changes nothing.
pub const VOICE_GATE_DEPTH_MAX: u8 = 60;
/// Voice gate sensitivity level: 1 keeps the quietest voices and lets more noise
/// through, 5 mutes more noise and may cut faint words.
pub const VOICE_SENSITIVITY_DEFAULT: u8 = 3;
pub const VOICE_SENSITIVITY_MAX: u8 = 5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseReductionConfig {
    pub enabled: bool,
    pub model: NoiseModel,
    /// How much noise to remove, 0..=1; mapped to the plugin's attenuation limit.
    pub strength: f32,
    /// Silence the DeepFilterNet3 output while the Silero detector hears no
    /// speech. Adds latency (66 ms in total for either model), so it is a choice.
    pub voice_gate: bool,
    /// The same gate on DPDFNet, off by default: that model keeps the background
    /// quiet itself, and the gate adds a detector and 6 ms.
    pub dpdfnet_voice_gate: bool,
    /// Attenuation of that gate, in dB. Only the command line sets it.
    pub voice_gate_depth_db: u8,
    /// How readily the gate's detector hears a voice, 1..=5 (3 = normal).
    pub voice_sensitivity: u8,
    /// How hard the plugin's silence expander ducks the residual floor in pauses,
    /// in dB (0 = off). The plugin caps it by the attenuation limit. Variable
    /// because the low-latency model leaves a louder residual than the light one.
    pub silence_depth: u8,
    /// Optional independent LSNR gate calibration. None preserves the legacy
    /// depth mapping; this is an SNR estimate, NOT a speech probability.
    pub min_snr_override_db: Option<f32>,
    /// Existing DFN spectral post-filter, beta 0 = off. Start A/B at 0.02,
    /// not at 100%: this may remove residual noise AND weaken speech details.
    pub post_filter_beta: f32,
    /// DFN activation-only startup mute, not algorithmic latency. Changing it
    /// requires a reload; retain 1000 ms until the short-onset preset is tested.
    pub startup_mute_ms: u16,
    /// Optional per-frequency "silence floor" curve (dBFS) drawn by the user over
    /// the spectrum. `None` = the scalar pause gate only (default). `Some` sends the
    /// 10 anchor thresholds to the DFN plugin's floor ports; the processed spectrum
    /// must rise above the curve (per band) to leave silence. It only *tightens*
    /// muting (AND-ed with the model gate), and is pushed live like the other
    /// controls (no reload).
    pub silence_curve_db: Option<[f32; SILENCE_ANCHORS]>,
}

/// Speech peak (dBFS) a calibration pass levels the microphone to. Not LUFS and
/// not a volume fraction: it is the sustained sample-peak level of the speaking
/// voice, with headroom left for transients. -12 dBFS is where the old
/// target slider's midpoint (85%) sat, and where people calibrate against an OBS
/// meter.
pub const CALIBRATION_TARGET_DBFS: f32 = -12.0;

/// What a calibration left behind for one capture device.
///
/// A calibration is a decision the person made once, out loud, about this
/// microphone in this room: it is not live state and nothing here changes while
/// they talk. The continuous controller this replaced could not tell its owner
/// from a television in the next room, so it is gone — and the fast pass that
/// took its place needs no such judgement, because the person is deliberately
/// speaking into the microphone for the few seconds it lasts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DeviceCalibration {
    /// Capture volume the quick pass settled on (the `wpctl` fraction, 0..=3).
    pub volume: f32,
    /// The window a daemon may move inside, from the advanced two-point pass:
    /// speaking far away sets the loud end, speaking close sets the quiet end.
    /// Both or neither — one bound alone describes no window.
    pub min_volume: Option<f32>,
    pub max_volume: Option<f32>,
    /// What this person's voice looked like while it was being measured, so a
    /// daemon can apply a conservative heuristic, not identify a speaker.
    ///
    /// Two numbers per axis, the quietest and the loudest across every situation
    /// measured in the round. `voice_dbfs` is the level referred to unity gain,
    /// so it says what reaches the capsule rather than where the knob happened
    /// to be. `voice_tilt_db` is the energy above 2 kHz minus the energy below
    /// it (legacy mean-log-band index, not integrated energy). Microphone
    /// response, EQ, distance and room acoustics can all change this index; it
    /// does not reliably distinguish an owner from television or other voices.
    ///
    /// `voice_metered_filtered` says which signal was measured — the filter
    /// chain's output or the raw device — because the two are different signals
    /// and the denoiser moves both numbers. Measured under one and metered under
    /// the other, these say nothing, and the daemon ignores them.
    pub voice_low_dbfs: Option<f32>,
    pub voice_high_dbfs: Option<f32>,
    pub voice_tilt_low_db: Option<f32>,
    pub voice_tilt_high_db: Option<f32>,
    pub voice_metered_filtered: Option<bool>,
}

/// What a calibration pass observed about the voice itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceEvidence {
    /// Speaking level referred to unity gain, in dBFS.
    pub dbfs: f32,
    /// Legacy difference of mean log-band magnitudes above/below 2 kHz;
    /// not integrated energy, distance or speaker identity.
    pub tilt_db: f32,
    /// True when the filter chain's output was the signal measured.
    pub metered_filtered: bool,
}

impl DeviceCalibration {
    /// Start a round of measurements: this is the everyday volume now, and any
    /// window an earlier round left behind is gone.
    ///
    /// A window is a claim about the places measured in *one* round. Keeping the
    /// previous one lets a measurement nobody remembers taking decide how far
    /// the level may travel — which is how a single measurement came to be
    /// reported on screen as a window from 300% to 300%.
    pub fn start_round(&mut self, volume: f32, voice: Option<VoiceEvidence>) {
        *self = Self {
            volume,
            ..Self::default()
        };
        if let Some(voice) = voice {
            self.voice_low_dbfs = Some(voice.dbfs);
            self.voice_high_dbfs = Some(voice.dbfs);
            self.voice_tilt_low_db = Some(voice.tilt_db);
            self.voice_tilt_high_db = Some(voice.tilt_db);
            self.voice_metered_filtered = Some(voice.metered_filtered);
        }
    }

    /// Another place the same person speaks from: stretch the window to hold it.
    ///
    /// The first measurement of the round is a place too, so the window starts
    /// around the volume it settled on. Only the quietest and the loudest place
    /// matter — everything between them is inside the window already — so any
    /// number of places can be added and none of them has to be named.
    pub fn add_place(&mut self, volume: f32, voice: Option<VoiceEvidence>) {
        self.min_volume = Some(self.min_volume.unwrap_or(self.volume).min(volume));
        self.max_volume = Some(self.max_volume.unwrap_or(self.volume).max(volume));
        let Some(voice) = voice else { return };
        // A situation measured under the other meter is not comparable with what
        // the round already holds; it replaces rather than widens, and says so.
        if self.voice_metered_filtered != Some(voice.metered_filtered) {
            self.voice_low_dbfs = None;
            self.voice_high_dbfs = None;
            self.voice_tilt_low_db = None;
            self.voice_tilt_high_db = None;
        }
        self.voice_metered_filtered = Some(voice.metered_filtered);
        self.voice_low_dbfs = Some(self.voice_low_dbfs.unwrap_or(voice.dbfs).min(voice.dbfs));
        self.voice_high_dbfs = Some(self.voice_high_dbfs.unwrap_or(voice.dbfs).max(voice.dbfs));
        self.voice_tilt_low_db = Some(
            self.voice_tilt_low_db
                .unwrap_or(voice.tilt_db)
                .min(voice.tilt_db),
        );
        self.voice_tilt_high_db = Some(
            self.voice_tilt_high_db
                .unwrap_or(voice.tilt_db)
                .max(voice.tilt_db),
        );
    }

    /// What this person's voice may look like, for a daemon metering the signal
    /// the calibration measured. `None` when nothing was recorded, when the
    /// recording is half written, or when the daemon is metering the other one.
    ///
    /// The bounds are deliberately generous. Level is open at the top: leaning
    /// into the microphone is still the owner, and only what arrives far below
    /// anything they were measured saying is refused. Tilt is bounded both ways:
    /// a wall takes treble off a voice and a keyboard adds it.
    #[must_use]
    pub fn voice_profile(&self, metering_filtered: bool) -> Option<VoiceProfile> {
        if self.voice_metered_filtered? != metering_filtered {
            return None;
        }
        let (low, tilt_low, tilt_high) = (
            self.voice_low_dbfs?,
            self.voice_tilt_low_db?,
            self.voice_tilt_high_db?,
        );
        let sane = |v: f32| v.is_finite();
        (sane(low) && sane(tilt_low) && sane(tilt_high) && tilt_low <= tilt_high).then_some(
            VoiceProfile {
                floor_dbfs: low - VOICE_LEVEL_MARGIN_DB,
                tilt_low_db: tilt_low - VOICE_TILT_MARGIN_DB,
                tilt_high_db: tilt_high + VOICE_TILT_MARGIN_DB,
            },
        )
    }

    /// Draw the window by hand instead of measuring it, from the percentages a
    /// person reads off a slider.
    ///
    /// Two measured places are the ordinary way to get a window; this is the
    /// fine adjustment afterwards, for somebody who knows their room and wants
    /// the daemon kept between two numbers of their choosing. A hand-drawn
    /// window must have width — `window()` tolerates `low == high` for a pair
    /// that measured identically, but nobody sets that on purpose and nothing
    /// can move inside it.
    pub fn set_window_percent(
        &mut self,
        low_percent: f32,
        high_percent: f32,
    ) -> Result<(), String> {
        let (low, high) = (low_percent / 100.0, high_percent / 100.0);
        let sane = |v: f32| v.is_finite() && (0.0..=3.0).contains(&v);
        if !(sane(low) && sane(high)) {
            return Err("a level range runs from 0% to 300%".into());
        }
        if low >= high {
            return Err("the lowest level must be below the highest".into());
        }
        self.min_volume = Some(low);
        self.max_volume = Some(high);
        Ok(())
    }

    /// Forget the window. The everyday volume stays: it is a different fact.
    pub fn clear_window(&mut self) {
        self.min_volume = None;
        self.max_volume = None;
    }

    /// The window the daemon may work inside, if this device has one.
    ///
    /// Anything outside 0..=3, inverted, or half-written is not a window: a
    /// hand-edited or truncated file must leave the volume alone rather than
    /// drive it somewhere nobody chose.
    #[must_use]
    pub fn window(&self) -> Option<(f32, f32)> {
        let (low, high) = (self.min_volume?, self.max_volume?);
        let sane = |v: f32| v.is_finite() && (0.0..=3.0).contains(&v);
        (sane(low) && sane(high) && low <= high).then_some((low, high))
    }
}

/// How far under the quietest measured voice something may still be that voice.
///
/// Speaking effort varies by a few decibels from day to day and a head turns
/// without anybody thinking of it as a new situation; twelve decibels covers
/// that and still sits far above a talker through a wall, who arrives thirty or
/// more below. Being generous here costs a little noise immunity; being mean
/// costs somebody their own microphone, which is not a trade this makes.
pub const VOICE_LEVEL_MARGIN_DB: f32 = 12.0;
/// The same idea for the spectral tilt, where the spread between one sitting
/// position and another is smaller and the difference a wall makes is larger.
pub const VOICE_TILT_MARGIN_DB: f32 = 6.0;

/// The bounds a daemon admits a frame within. Level is one-sided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceProfile {
    pub floor_dbfs: f32,
    pub tilt_low_db: f32,
    pub tilt_high_db: f32,
}

impl VoiceProfile {
    /// Could this frame be the voice the calibration measured?
    #[must_use]
    pub fn admits(&self, dbfs: f32, tilt_db: f32) -> bool {
        dbfs.is_finite()
            && tilt_db.is_finite()
            && dbfs >= self.floor_dbfs
            && (self.tilt_low_db..=self.tilt_high_db).contains(&tilt_db)
    }
}

/// Per-device calibrations, keyed by PipeWire `node.name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MicCalibrationConfig {
    pub devices: BTreeMap<String, DeviceCalibration>,
}

/// Default silence-expander depth (dB) and the plugin port's ceiling. The default
/// sits at 75% of the range, retaining the pre-audit preset. It is not proof of
/// voice preservation: the level expander and LSNR gate need corpus validation.
pub const SILENCE_DEPTH_DEFAULT: u8 = 30;
pub const SILENCE_DEPTH_MAX: u8 = 40;

/// Per-frequency silence-floor curve: number of anchors and their exact DFN plugin
/// port names (must match `dfn3-ladspa`/`dfn3ll-ladspa` `PN_A0..PN_A9`). A value
/// at or below the sentinel means "off" for that anchor; `None` sends all sentinel.
pub const SILENCE_ANCHORS: usize = 10;
pub const SILENCE_ANCHOR_SENTINEL: f64 = -200.0;
pub const SILENCE_ANCHOR_PORTS: [&str; SILENCE_ANCHORS] = [
    "Silence floor 60 Hz (dB)",
    "Silence floor 125 Hz (dB)",
    "Silence floor 250 Hz (dB)",
    "Silence floor 500 Hz (dB)",
    "Silence floor 1 kHz (dB)",
    "Silence floor 2 kHz (dB)",
    "Silence floor 4 kHz (dB)",
    "Silence floor 8 kHz (dB)",
    "Silence floor 12 kHz (dB)",
    "Silence floor 16 kHz (dB)",
];
/// Anchor centre frequencies (Hz), matching the plugin's `ANCHOR_HZ`. Used by the
/// GUI/CLI to label the curve; the plugin owns the interpolation to ERB bands.
pub const SILENCE_ANCHOR_HZ: [f32; SILENCE_ANCHORS] = [
    60.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 12000.0, 16000.0,
];

/// Legacy coupling between pause depth and the model's LSNR threshold.
/// Higher thresholds reject more low-SNR frames; they can also reject weak
/// speech. No threshold here guarantees that soft onsets will be preserved.
/// Independent calibration is available through `min_snr_override_db`.
pub const MIN_SNR_GATE_AT_ZERO_DB: f64 = -10.0;
pub const MIN_SNR_GATE_AT_MAX_DB: f64 = -4.0;

impl NoiseReductionConfig {
    /// The `Silence expander depth (dB)` control value, clamped to the port range.
    /// When a per-frequency floor curve is set it is the only silence control, so
    /// the expander runs at full depth (the curve decides *where* to mute, the
    /// depth only needs to be non-zero for the mute to reach silence).
    #[must_use]
    pub fn silence_expander_depth_db(&self) -> f64 {
        if self.silence_curve_db.is_some() {
            return f64::from(SILENCE_DEPTH_MAX);
        }
        f64::from(self.silence_depth.min(SILENCE_DEPTH_MAX))
    }

    /// The `Min SNR gate (dB)` control value. Derived from the same "quiet pauses"
    /// depth so the single UI slider drives both the post-model level expander
    /// (steady floor) and the model's own gate (intermittent bursts) together.
    #[must_use]
    pub fn min_snr_gate_db(&self) -> f64 {
        if let Some(value) = self.min_snr_override_db.filter(|v| v.is_finite()) {
            return f64::from(value.clamp(-40.0, 0.0));
        }
        let f = f64::from(self.silence_depth.min(SILENCE_DEPTH_MAX)) / f64::from(SILENCE_DEPTH_MAX);
        MIN_SNR_GATE_AT_ZERO_DB + f * (MIN_SNR_GATE_AT_MAX_DB - MIN_SNR_GATE_AT_ZERO_DB)
    }
}

impl NoiseReductionConfig {
    /// Controls of an attenuation-only denoiser, for the static graph and the
    /// live update alike. The native DPDFNet plugin has only the attenuation
    /// cap; the DFN3 gate, expander and extension ports do not exist on it.
    #[must_use]
    pub fn attenuation_controls(
        &self,
        attenuation_db: f64,
        min_snr_gate_db: f64,
    ) -> Vec<(&'static str, f64)> {
        let mut controls = vec![("Attenuation Limit (dB)", attenuation_db)];
        if self.model.is_dfn3() {
            controls.push(("Min SNR gate (dB)", min_snr_gate_db));
            controls.push((
                "Silence expander depth (dB)",
                self.silence_expander_depth_db(),
            ));
            controls.extend(self.dfn_extension_controls());
        }
        controls
    }

    /// Extension ports of the DeepFilterNet3 plugins only. Both renderer and live
    /// control use this list; DPDFNet has none of these ports.
    #[must_use]
    pub fn dfn_extension_controls(&self) -> Vec<(&'static str, f64)> {
        if !self.model.is_dfn3() {
            return Vec::new();
        }
        let beta = if self.post_filter_beta.is_finite() {
            f64::from(self.post_filter_beta.clamp(0.0, 1.0))
        } else {
            0.0
        };
        let mut controls = vec![
            ("Post filter beta", beta),
            (
                "Startup mute (ms)",
                f64::from(self.startup_mute_ms.min(1000)),
            ),
        ];
        // Per-frequency silence-floor anchors. Always send all 10 so clearing the
        // curve (None) actively resets the plugin ports to the sentinel (off) via
        // the same live-control path — never leaves a stale curve on the plugin.
        for (i, name) in SILENCE_ANCHOR_PORTS.iter().enumerate() {
            let v = match self.silence_curve_db {
                Some(curve) if curve[i].is_finite() => {
                    f64::from(curve[i]).clamp(SILENCE_ANCHOR_SENTINEL, 0.0)
                }
                _ => SILENCE_ANCHOR_SENTINEL,
            };
            controls.push((name, v));
        }
        // 0 switches the gate off. The plugin decides on or off when it starts
        // running, so changing this one way or the other needs a reload.
        controls.push(("Voice gate depth (dB)", self.voice_gate_depth()));
        controls
    }

    /// Voice gate ports for the static graph only: the sensitivity, and DPDFNet's
    /// depth, which DFN3 takes with its extension controls. The plugin reads them
    /// once per activation, so a change reloads the chain; and a plugin build
    /// without the ports ignores them there with a warning, where the live push
    /// would refuse the whole request.
    #[must_use]
    pub fn voice_gate_controls(&self) -> Vec<(&'static str, f64)> {
        let mut controls = Vec::new();
        if !self.model.is_dfn3() {
            controls.push(("Voice gate depth (dB)", self.voice_gate_depth()));
        }
        controls.push((
            "Voice sensitivity (1 sensitive, 3 normal, 5 strict)",
            f64::from(self.voice_sensitivity.clamp(1, VOICE_SENSITIVITY_MAX)),
        ));
        controls
    }

    /// Whether `model` runs the voice gate.
    #[must_use]
    pub fn voice_gate_for(&self, model: NoiseModel) -> bool {
        if model.is_dfn3() {
            self.voice_gate
        } else {
            self.dpdfnet_voice_gate
        }
    }

    /// The `Voice gate depth (dB)` control value: 0 when the gate is off.
    #[must_use]
    pub fn voice_gate_depth(&self) -> f64 {
        if self.voice_gate_for(self.model) {
            f64::from(self.voice_gate_depth_db.clamp(1, VOICE_GATE_DEPTH_MAX))
        } else {
            0.0
        }
    }
}

impl Default for NoiseReductionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: NoiseModel::default(),
            strength: STRENGTH_DEFAULT,
            voice_gate: true,
            dpdfnet_voice_gate: false,
            voice_gate_depth_db: VOICE_GATE_DEPTH_DEFAULT,
            voice_sensitivity: VOICE_SENSITIVITY_DEFAULT,
            silence_depth: SILENCE_DEPTH_DEFAULT,
            min_snr_override_db: None,
            post_filter_beta: 0.0,
            startup_mute_ms: 1000,
            silence_curve_db: None,
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

    const SENSITIVITY: &str = "Voice sensitivity (1 sensitive, 3 normal, 5 strict)";

    #[test]
    fn dfn_extension_controls_are_bounded_and_model_specific() {
        let mut nr = NoiseReductionConfig::default();
        let ctrls = nr.dfn_extension_controls();
        assert_eq!(ctrls[0], ("Post filter beta", 0.0));
        assert_eq!(ctrls[1], ("Startup mute (ms)", 1000.0));
        // No curve by default → all 10 anchor ports carry the sentinel (off).
        assert_eq!(ctrls.len(), 3 + SILENCE_ANCHORS);
        assert!(
            ctrls[2..2 + SILENCE_ANCHORS]
                .iter()
                .all(|&(n, v)| n.starts_with("Silence floor") && v == SILENCE_ANCHOR_SENTINEL)
        );
        nr.post_filter_beta = f32::NAN;
        nr.startup_mute_ms = u16::MAX;
        assert_eq!(nr.dfn_extension_controls()[0].1, 0.0);
        assert_eq!(nr.dfn_extension_controls()[1].1, 1000.0);
        assert_eq!(
            ctrls.last(),
            Some(&("Voice gate depth (dB)", f64::from(VOICE_GATE_DEPTH_DEFAULT)))
        );
        nr.voice_gate = false;
        assert_eq!(nr.dfn_extension_controls().last().unwrap().1, 0.0);
        nr.voice_gate = true;
        nr.voice_gate_depth_db = 0;
        assert_eq!(nr.dfn_extension_controls().last().unwrap().1, 1.0);
        nr.voice_gate_depth_db = u8::MAX;
        assert_eq!(nr.dfn_extension_controls().last().unwrap().1, 60.0);
        assert_eq!(nr.voice_gate_controls(), [(SENSITIVITY, 3.0)]);
        nr.voice_sensitivity = 0;
        assert_eq!(nr.voice_gate_controls(), [(SENSITIVITY, 1.0)]);
        nr.voice_sensitivity = u8::MAX;
        assert_eq!(nr.voice_gate_controls(), [(SENSITIVITY, 5.0)]);
        nr.model = NoiseModel::Dpdfnet;
        // Its own switch, off by default.
        assert_eq!(nr.voice_gate_controls()[0], ("Voice gate depth (dB)", 0.0));
        nr.dpdfnet_voice_gate = true;
        assert_eq!(nr.voice_gate_controls()[0], ("Voice gate depth (dB)", 60.0));
        assert!(nr.dfn_extension_controls().is_empty());
        assert_eq!(nr.attenuation_controls(100.0, -10.0).len(), 1);
        nr.min_snr_override_db = Some(-15.0);
        nr.silence_depth = 40;
        assert_eq!(nr.min_snr_gate_db(), -15.0);
    }

    #[test]
    fn min_snr_gate_tracks_the_quiet_pauses_depth() {
        let gate = |d: u8| {
            NoiseReductionConfig {
                silence_depth: d,
                ..Default::default()
            }
            .min_snr_gate_db()
        };
        // Default depth (30 = 75%) lands at -5.5 dB. These are legacy
        // calibration values, not a validated speech-probability threshold.
        assert!((gate(SILENCE_DEPTH_DEFAULT) - (-5.5)).abs() < 1e-9);
        assert!((gate(0) - MIN_SNR_GATE_AT_ZERO_DB).abs() < 1e-9);
        assert!((gate(SILENCE_DEPTH_MAX) - MIN_SNR_GATE_AT_MAX_DB).abs() < 1e-9);
        // Over-range depth is clamped, never past the ceiling.
        assert!((gate(200) - MIN_SNR_GATE_AT_MAX_DB).abs() < 1e-9);
    }

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
    fn a_round_of_places_keeps_only_the_quietest_and_the_loudest() {
        let mut device = DeviceCalibration::default();
        device.start_round(1.0, None);
        assert_eq!(device.window(), None, "one place is not a window");

        device.add_place(1.6, None);
        assert_eq!(
            device.window(),
            Some((1.0, 1.6)),
            "the first place is an end"
        );
        device.add_place(1.3, None);
        assert_eq!(
            device.window(),
            Some((1.0, 1.6)),
            "a place inside changes nothing"
        );
        device.add_place(0.4, None);
        assert_eq!(
            device.window(),
            Some((0.4, 1.6)),
            "a quieter place moves the low end"
        );

        device.start_round(2.0, None);
        assert_eq!(
            device.window(),
            None,
            "a fresh round forgets the old window"
        );
        assert!((device.volume - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_voice_is_recognised_by_what_the_calibration_heard_of_it() {
        let near = VoiceEvidence {
            dbfs: -24.0,
            tilt_db: -8.0,
            metered_filtered: true,
        };
        let far = VoiceEvidence {
            dbfs: -31.0,
            tilt_db: -14.0,
            metered_filtered: true,
        };
        let mut device = DeviceCalibration::default();
        device.start_round(1.57, Some(near));
        device.add_place(2.08, Some(far));

        let profile = device
            .voice_profile(true)
            .expect("two situations describe a voice");
        assert!(
            profile.admits(-24.0, -8.0),
            "the situation it was measured in"
        );
        assert!(profile.admits(-31.0, -14.0), "and the other one");
        assert!(
            profile.admits(-10.0, -9.0),
            "leaning into the microphone is still them"
        );
        assert!(
            !profile.admits(-60.0, -10.0),
            "a talker through a wall is not"
        );
        assert!(
            !profile.admits(-25.0, -30.0),
            "nor anything that lost its treble"
        );
        assert!(
            !profile.admits(-25.0, 4.0),
            "nor a keyboard, which is all treble"
        );

        // Measured through the filter chain, metered raw: different signals, so
        // the numbers say nothing and the test disables itself.
        assert!(device.voice_profile(false).is_none());

        // A situation measured under the other meter replaces the round rather
        // than widening it with something not comparable.
        device.add_place(
            1.8,
            Some(VoiceEvidence {
                dbfs: -40.0,
                tilt_db: -20.0,
                metered_filtered: false,
            }),
        );
        let profile = device.voice_profile(false).expect("the new meter's round");
        assert!(profile.admits(-40.0, -20.0));
        assert!(
            !profile.admits(-24.0, -8.0),
            "the old meter's voice did not carry over"
        );
    }

    #[test]
    fn a_hand_drawn_window_is_checked_before_it_is_kept() {
        let mut device = DeviceCalibration {
            volume: 1.2,
            ..DeviceCalibration::default()
        };
        assert!(device.set_window_percent(120.0, 200.0).is_ok());
        assert_eq!(device.window(), Some((1.2, 2.0)));
        assert!(device.set_window_percent(200.0, 120.0).is_err(), "inverted");
        assert!(device.set_window_percent(150.0, 150.0).is_err(), "no width");
        assert!(
            device.set_window_percent(10.0, 400.0).is_err(),
            "past the actuator"
        );
        assert!(
            device.set_window_percent(f32::NAN, 200.0).is_err(),
            "not a number"
        );
        assert_eq!(
            device.window(),
            Some((1.2, 2.0)),
            "a refused range changes nothing"
        );
        device.clear_window();
        assert_eq!(device.window(), None);
        assert!(
            (device.volume - 1.2).abs() < f32::EPSILON,
            "the everyday volume stayed"
        );
    }

    #[test]
    fn a_half_written_calibration_window_is_not_a_window() {
        let mut device = DeviceCalibration {
            volume: 1.2,
            min_volume: Some(0.8),
            ..DeviceCalibration::default()
        };
        assert_eq!(device.window(), None, "one bound describes no window");
        device.max_volume = Some(0.4);
        assert_eq!(device.window(), None, "inverted bounds describe no window");
        device.max_volume = Some(9.0);
        assert_eq!(
            device.window(),
            None,
            "a volume nobody can set is not a bound"
        );
        device.max_volume = Some(2.4);
        assert_eq!(device.window(), Some((0.8, 2.4)));
        let roundtrip: DeviceCalibration =
            serde_json::from_str(&serde_json::to_string(&device).unwrap()).unwrap();
        assert_eq!(roundtrip, device);
    }
}
