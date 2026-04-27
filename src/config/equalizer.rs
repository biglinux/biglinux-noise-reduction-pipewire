//! 10-band parametric equalizer configuration + curated voice presets.
//!
//! `bands` is index-aligned with [`super::paths::EQ_BANDS_HZ`]. The preset
//! names are stable identifiers (`"default_voice"`, `"flat"`, …) used in both
//! the JSON file and the UI. User-facing display names are translated via
//! gettext at render time so this module stays i18n-agnostic.

use serde::{Deserialize, Serialize};

pub const EQ_BAND_COUNT: usize = 10;
pub const EQ_BAND_DEFAULT: f32 = 0.0;
pub const EQ_BAND_MIN: f32 = -40.0;
pub const EQ_BAND_MAX: f32 = 40.0;

/// Identifier of the preset last applied by the user (or `"custom"` when the
/// bands have been edited manually).
pub const EQ_PRESET_DEFAULT: &str = "flat";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqualizerConfig {
    pub enabled: bool,
    pub bands: Vec<f32>,
    pub preset: String,
}

impl Default for EqualizerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bands: vec![EQ_BAND_DEFAULT; EQ_BAND_COUNT],
            preset: EQ_PRESET_DEFAULT.to_owned(),
        }
    }
}

impl EqualizerConfig {
    /// Ensure `bands.len() == EQ_BAND_COUNT`. Called after deserialisation to
    /// protect the rest of the code from malformed settings files.
    pub fn normalize(&mut self) {
        if self.bands.len() != EQ_BAND_COUNT {
            self.bands = vec![EQ_BAND_DEFAULT; EQ_BAND_COUNT];
        }
        for b in &mut self.bands {
            *b = b.clamp(EQ_BAND_MIN, EQ_BAND_MAX);
        }
    }
}

// ── Presets ──────────────────────────────────────────────────────────

/// Preset definition: raw band gains indexed with `EQ_BANDS_HZ`.
struct Preset {
    id: &'static str,
    bands: [f32; EQ_BAND_COUNT],
}

const PRESETS: &[Preset] = &[
    Preset {
        id: "default_voice",
        bands: [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0, 0.0],
    },
    Preset {
        id: "flat",
        bands: [0.0; EQ_BAND_COUNT],
    },
    Preset {
        id: "voice_boost",
        bands: [-10.0, -5.0, 0.0, 5.0, 15.0, 20.0, 15.0, 10.0, 5.0, 0.0],
    },
    Preset {
        id: "podcast",
        bands: [5.0, 5.0, 10.0, 5.0, 0.0, 5.0, 10.0, 5.0, 0.0, -5.0],
    },
    Preset {
        id: "warm",
        bands: [10.0, 15.0, 10.0, 5.0, 0.0, -5.0, -10.0, -15.0, -15.0, -20.0],
    },
    Preset {
        id: "bright",
        bands: [-10.0, -5.0, 0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 20.0, 15.0],
    },
    Preset {
        id: "de_esser",
        bands: [0.0, 0.0, 0.0, 0.0, 0.0, -5.0, -15.0, -25.0, -20.0, -10.0],
    },
    Preset {
        id: "bass_cut",
        bands: [-40.0, -35.0, -25.0, -15.0, -5.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    // Broadcast-style presence boost: keeps the voice fundamental
    // intact (0 dB at 125 Hz), trims the "boxy" 250-500 Hz region
    // that muddies laptop-mic capture, then pushes the consonant
    // clarity band (2-4 kHz, indices 6-7) where intelligibility
    // actually lives. A small lift at 8 kHz adds "air" without
    // amplifying the hiss floor at 16 kHz.
    Preset {
        id: "presence",
        bands: [0.0, 0.0, 0.0, -3.0, -2.0, 2.0, 8.0, 10.0, 5.0, 0.0],
    },
    Preset {
        id: "custom",
        bands: [0.0; EQ_BAND_COUNT],
    },
];

/// Ordered list of preset ids, useful for populating a dropdown.
#[must_use]
pub fn eq_preset_ids() -> Vec<&'static str> {
    PRESETS.iter().map(|p| p.id).collect()
}

/// Look up a preset's bands by id. `Some(slice)` on match; `None` otherwise.
#[must_use]
pub fn eq_preset_bands(id: &str) -> Option<&'static [f32; EQ_BAND_COUNT]> {
    PRESETS.iter().find(|p| p.id == id).map(|p| &p.bands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_correct_band_count() {
        let c = EqualizerConfig::default();
        assert_eq!(c.bands.len(), EQ_BAND_COUNT);
        assert_eq!(c.preset, EQ_PRESET_DEFAULT);
        assert!(!c.enabled);
    }

    #[test]
    fn normalize_fixes_wrong_length() {
        let mut c = EqualizerConfig {
            bands: vec![1.0, 2.0, 3.0],
            ..EqualizerConfig::default()
        };
        c.normalize();
        assert_eq!(c.bands.len(), EQ_BAND_COUNT);
        assert!(c
            .bands
            .iter()
            .all(|b| (*b - EQ_BAND_DEFAULT).abs() < f32::EPSILON));
    }

    #[test]
    fn normalize_clamps_out_of_range_values() {
        let mut c = EqualizerConfig {
            bands: vec![100.0, -100.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ..EqualizerConfig::default()
        };
        c.normalize();
        assert!((c.bands[0] - EQ_BAND_MAX).abs() < f32::EPSILON);
        assert!((c.bands[1] - EQ_BAND_MIN).abs() < f32::EPSILON);
    }

    #[test]
    fn presets_have_exact_band_count() {
        for p in PRESETS {
            assert_eq!(p.bands.len(), EQ_BAND_COUNT, "preset {} wrong length", p.id);
        }
    }

    #[test]
    fn eq_preset_ids_non_empty_and_contains_flat() {
        let ids = eq_preset_ids();
        assert!(!ids.is_empty());
        assert!(ids.contains(&"flat"));
    }

    #[test]
    fn eq_preset_bands_lookup() {
        let flat = eq_preset_bands("flat").expect("flat preset exists");
        assert!(flat.iter().all(|b| (*b - 0.0).abs() < f32::EPSILON));

        assert!(eq_preset_bands("nonexistent").is_none());
    }

    #[test]
    fn presence_preset_emphasises_consonant_clarity_band() {
        // Presence is the broadcast intelligibility curve: the gain
        // peak must land on 2 kHz / 4 kHz (indices 6 and 7) — that's
        // where the consonants the user actually wants live. A peak
        // anywhere else means the preset is mislabelled.
        let bands = eq_preset_bands("presence").expect("presence preset exists");
        let peak_idx = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        assert!(
            peak_idx == 6 || peak_idx == 7,
            "presence peak must sit on 2 kHz or 4 kHz, got index {peak_idx}",
        );
        // Voice fundamental must be untouched — boosting 125 Hz turns
        // a presence preset into a generic warmth preset.
        assert!(bands[2].abs() < f32::EPSILON, "125 Hz must stay flat");
        // 16 kHz hiss must not be amplified.
        assert!(bands[9] <= 0.0, "16 kHz must not boost hiss");
    }
}
