// SPDX-License-Identifier: MIT

//! The three native denoisers the microphone and output chains can run.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Stable identifier of a noise-reduction model.
///
/// Serialized as `u8` in `settings.json` and published as `biglinux.model` on the
/// running node; the numbers are the ones those already carry. A number this build
/// does not know reads as the default model.
///
/// # Examples
///
/// ```
/// use biglinux_microphone::config::noise_model::NoiseModel;
///
/// assert_eq!(NoiseModel::from(12), NoiseModel::Dfn3Ll);
/// assert_eq!(NoiseModel::from(4), NoiseModel::default());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum NoiseModel {
    /// DeepFilterNet3 (W8A16): the lightest, with the Silero voice gate.
    #[default]
    Dfn3 = 11,
    /// DeepFilterNet3-LL (W8A16): no lookahead, larger network.
    Dfn3Ll = 12,
    /// DPDFNet-2 48 kHz HR (W8A16): the heaviest; keeps its own quiet floor.
    Dpdfnet = 13,
}

impl From<u8> for NoiseModel {
    fn from(value: u8) -> Self {
        match value {
            12 => Self::Dfn3Ll,
            13 => Self::Dpdfnet,
            _ => Self::Dfn3,
        }
    }
}

impl From<NoiseModel> for u8 {
    fn from(model: NoiseModel) -> Self {
        model as Self
    }
}

impl NoiseModel {
    pub const ALL: [Self; 3] = [Self::Dfn3, Self::Dfn3Ll, Self::Dpdfnet];

    /// Whether the plugin is one of the DeepFilterNet3 pair, which share the SNR
    /// gate, silence expander, floor curve and voice gate ports.
    #[must_use]
    pub fn is_dfn3(self) -> bool {
        matches!(self, Self::Dfn3 | Self::Dfn3Ll)
    }

    /// The word the command line takes and prints for this model.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Dfn3 => "dfn3",
            Self::Dfn3Ll => "dfn3-ll",
            Self::Dpdfnet => "dpdfnet",
        }
    }

    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.word() == word)
    }

    /// Audio port names of the plugin.
    #[must_use]
    pub fn attenuation_ports(self) -> (&'static str, &'static str) {
        match self {
            Self::Dpdfnet => ("Input", "Output"),
            Self::Dfn3 | Self::Dfn3Ll => ("Audio In", "Audio Out"),
        }
    }

    /// The plugin's shared object and LADSPA label.
    #[must_use]
    pub fn plugin_and_label(self) -> (&'static str, &'static str) {
        match self {
            Self::Dfn3 => (
                "/usr/lib/ladspa/libdfn3_ladspa.so",
                "deep_filter_net3_rs_mono",
            ),
            Self::Dfn3Ll => (
                "/usr/lib/ladspa/libdfn3ll_ladspa.so",
                "deep_filter_net3_ll_rs_mono",
            ),
            Self::Dpdfnet => (
                "/usr/lib/ladspa/libdpdfnet_native.so",
                "dpdfnet_native_48hr",
            ),
        }
    }

    /// Whether the plugin file is present on disk right now (one `stat`).
    #[must_use]
    pub fn plugin_available(self) -> bool {
        Path::new(self.plugin_and_label().0).is_file()
    }
}

pub fn deepfilter_attenuation_db(strength: f32) -> f64 {
    let strength = if strength.is_nan() {
        0.0
    } else {
        strength.clamp(0.0, 1.0)
    };
    f64::from(strength * strength) * 100.0
}

#[cfg(test)]
mod tests {
    use super::NoiseModel;

    #[test]
    fn models_round_trip_as_their_numbers_and_unknown_numbers_read_as_the_default() {
        for model in NoiseModel::ALL {
            let json = serde_json::to_string(&model).unwrap();
            assert_eq!(serde_json::from_str::<NoiseModel>(&json).unwrap(), model);
            assert_eq!(NoiseModel::parse(model.word()), Some(model));
        }
        assert_eq!(serde_json::to_string(&NoiseModel::Dfn3Ll).unwrap(), "12");
        for removed in ["0", "4", "10", "99"] {
            assert_eq!(
                serde_json::from_str::<NoiseModel>(removed).unwrap(),
                NoiseModel::Dfn3
            );
        }
    }
}
