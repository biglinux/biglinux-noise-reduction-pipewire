// SPDX-License-Identifier: MIT

//! LADSPA gate and SC4 compressor controls for the microphone and output chains.

/// Default compressor intensity on a 0.0..=1.0 scale.
///
/// A gentle starting point: enough makeup gain and ratio to even out a
/// voice track but far from the aggressive curves used for broadcast-style
/// presets.
pub const COMPRESSOR_INTENSITY_DEFAULT: f64 = 0.25;

/// Resolved noise-gate parameters derived from a single intensity value.
///
/// Construct with [`GateDerived::from_unit_intensity`] to obtain the threshold,
/// range and timing values for one slider position. The fields mirror the
/// parameters of the FFmpeg `agate` filter so callers can hand them straight to
/// the filter formatter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateDerived {
    /// Open threshold in dBFS. Signals below this level are gated.
    pub threshold_db: f64,
    /// Attenuation applied to gated signal, in dB (negative value).
    pub range_db: f64,
    /// Attack time in milliseconds before the gate opens.
    pub attack_ms: f64,
    /// Hold time in milliseconds after the threshold is crossed downwards.
    pub hold_ms: f64,
    /// Release time in milliseconds for closing the gate.
    pub release_ms: f64,
}

impl GateDerived {
    /// Derive gate parameters from a unit-interval intensity.
    ///
    /// `intensity` is clamped to `0.0..=1.0`, with `NaN` treated as zero.
    /// The curve uses a square-root
    /// shaping so small slider moves near zero already produce an audible
    /// effect while the upper half stays smooth.
    ///
    /// # Examples
    ///
    /// ```
    /// use biglinux_microphone::config::dynamics::GateDerived;
    ///
    /// let gate = GateDerived::from_unit_intensity(0.0);
    /// assert_eq!(gate.threshold_db, -55.0);
    /// ```
    #[must_use]
    pub fn from_unit_intensity(intensity: f64) -> Self {
        let n = if intensity.is_nan() {
            0.0
        } else {
            intensity.clamp(0.0, 1.0)
        };
        let sqrt_n = n.sqrt();
        Self {
            threshold_db: -55.0 + sqrt_n * 25.0,
            range_db: -18.0 - sqrt_n * 22.0,
            attack_ms: 2.0,
            hold_ms: 150.0 - sqrt_n * 50.0,
            release_ms: 250.0 - sqrt_n * 100.0,
        }
    }
}

macro_rules! compressor_curve {
    ($intensity:expr) => {{
        let intensity = if $intensity.is_nan() {
            0.0
        } else {
            $intensity.clamp(0.0, 1.0)
        };
        (
            -15.0 - intensity * 15.0,
            2.0 + intensity * 4.0,
            10.0,
            100.0,
            2.0 + intensity * 8.0,
            3.0 + intensity * 5.0,
            0.0,
        )
    }};
}

/// SC4 LADSPA controls derived with the plugin UI's native f32 precision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sc4CompressorControls {
    /// Knee-center threshold in dBFS.
    pub threshold_db: f32,
    /// Compression ratio above the threshold.
    pub ratio: f32,
    /// Attack time in milliseconds.
    pub attack_ms: f32,
    /// Release time in milliseconds.
    pub release_ms: f32,
    /// Post-compression makeup gain in dB.
    pub makeup_gain_db: f32,
    /// Soft-knee width in dB.
    pub knee_db: f32,
    /// RMS-vs-peak detection bias.
    pub rms_peak: f32,
}

impl Sc4CompressorControls {
    /// Derive bit-stable SC4 controls from a unit-interval intensity.
    #[must_use]
    pub fn from_unit_intensity(intensity: f32) -> Self {
        let (threshold_db, ratio, attack_ms, release_ms, makeup_gain_db, knee_db, rms_peak) =
            compressor_curve!(intensity);
        Self {
            threshold_db,
            ratio,
            attack_ms,
            release_ms,
            makeup_gain_db,
            knee_db,
            rms_peak,
        }
    }
}
