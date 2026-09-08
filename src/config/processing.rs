//! Derived LADSPA control values for the gate and compressor.
//!
//! The UI exposes a single intensity slider per effect. Plugins expect
//! several tuned parameters (threshold, range, attack, release, …) so this
//! module projects the user intensity onto a coherent parameter set using
//! curve shapes tuned for voice capture.

use crate::config::dynamics::{
    COMPRESSOR_INTENSITY_DEFAULT as SHARED_COMPRESSOR_INTENSITY_DEFAULT, Sc4CompressorControls,
};
use serde::{Deserialize, Serialize};

// ── Compressor config + derived parameters ───────────────────────────

pub const COMPRESSOR_INTENSITY_DEFAULT: f32 = SHARED_COMPRESSOR_INTENSITY_DEFAULT as f32;

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

impl CompressorConfig {
    /// Resolve the bit-stable controls consumed by the SC4 LADSPA plugin.
    #[must_use]
    pub fn ladspa_controls(&self) -> Sc4CompressorControls {
        Sc4CompressorControls::from_unit_intensity(self.intensity)
    }
}
