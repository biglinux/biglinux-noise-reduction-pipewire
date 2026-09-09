//! Conservative static headroom plus a zero-lookahead sample ceiling.
//! The sum of positive peaking-EQ gains bounds their combined steady-state
//! gain. A final clamp contains transients; it can distort an overloaded
//! signal and is not an oversampled true-peak limiter.
use super::nodes::Node;
use crate::config::{
    AppSettings, EQ_BAND_COUNT, EQ_BAND_MAX, EqualizerConfig, GainSafety, StereoMode,
    eq_preset_bands,
};

pub(super) fn nodes(settings: &AppSettings, output: bool) -> Vec<Node> {
    if settings.gain_safety == GainSafety::Unrestricted {
        return Vec::new();
    }
    let multiplier = if output {
        output_headroom(settings)
    } else {
        mic_headroom(settings)
    };
    vec![
        Node::builtin("headroom", "linear").with_controls([("Mult", multiplier), ("Add", 0.0)]),
        Node::builtin("sample_ceiling", "clamp").with_controls([("Min", -1.0), ("Max", 1.0)]),
    ]
}

/// Level fed to an attenuation-only network, undone immediately after it.
///
/// DeepFilterNet logged `Possible clipping detected (1.007 … 1.268)` from its
/// own inference while this chain fed it a fixture peaking at 0.9 — the network
/// overshoots its input by about 1.4×. The [`nodes`] headroom cannot help: it
/// sits at the end of the chain and exists to offset *downstream* boosts, and
/// with no EQ or compressor its multiplier is 1.0.
///
/// Halving in front of the network and doubling straight after keeps the chain
/// at unity gain, and both factors are powers of two, so the round trip is
/// bit-exact in the graph's float samples. Everything calibrated in dB — the
/// standalone gate, the compressor, the final ceiling — stays after the makeup
/// and therefore still sees the original level.
const NEURAL_PAD: f64 = 0.5;

/// Pad placed immediately before an attenuation-only neural node.
pub(super) fn neural_pad() -> Node {
    Node::builtin("neural_pad", "linear").with_controls([("Mult", NEURAL_PAD), ("Add", 0.0)])
}

/// Exact inverse of [`neural_pad`], placed immediately after the node.
pub(super) fn neural_makeup() -> Node {
    Node::builtin("neural_makeup", "linear")
        .with_controls([("Mult", 1.0 / NEURAL_PAD), ("Add", 0.0)])
}

/// Whether this configuration's neural node needs the pad.
///
/// GTCRN is excluded deliberately: its gate thresholds are controls on the same
/// node and are expressed in dB, so padding its input would move them. The
/// advanced opt-out that disables the rest of the gain safety disables this too.
pub(crate) fn pads_neural_input(settings: &AppSettings, output: bool) -> bool {
    if settings.gain_safety == GainSafety::Unrestricted {
        return false;
    }
    let noise_reduction = if output {
        &settings.output_filter.noise_reduction
    } else {
        &settings.noise_reduction
    };
    noise_reduction.enabled && noise_reduction.model.is_attenuation_only()
}

fn eq_boost(eq: &EqualizerConfig) -> f64 {
    if !eq.enabled {
        return 0.0;
    }
    let bands = if eq.bands.len() == EQ_BAND_COUNT {
        eq.bands.clone()
    } else {
        eq_preset_bands(&eq.preset).map_or_else(|| vec![0.0; EQ_BAND_COUNT], |bands| bands.to_vec())
    };
    bands
        .into_iter()
        .map(|gain| {
            if gain.is_finite() {
                f64::from(gain.clamp(0.0, EQ_BAND_MAX))
            } else {
                0.0
            }
        })
        .sum()
}

fn multiplier(boost_db: f64) -> f64 {
    10.0_f64.powf(-boost_db.max(0.0) / 20.0)
}

pub(crate) fn mic_headroom(settings: &AppSettings) -> f64 {
    if settings.gain_safety == GainSafety::Unrestricted {
        return 1.0;
    }
    let eq = eq_boost(&settings.equalizer);
    let compressor = if settings.compressor.enabled {
        f64::from(settings.compressor.ladspa_controls().makeup_gain_db).max(0.0)
    } else {
        0.0
    };
    let pitch = if settings.stereo.enabled && settings.stereo.mode == StereoMode::VoiceChanger {
        let coefficient = 0.5 * 4.0_f64.powf(f64::from(settings.stereo.width.clamp(0.0, 1.0)));
        ((1.0 - coefficient) * 20.0).max(0.0)
    } else {
        0.0
    };
    multiplier(eq + compressor + pitch)
}

pub(crate) fn output_headroom(settings: &AppSettings) -> f64 {
    if settings.gain_safety == GainSafety::Unrestricted || !settings.output_filter.enabled {
        return 1.0;
    }
    let output = &settings.output_filter;
    let compressor = if output.compressor.enabled {
        f64::from(output.compressor.ladspa_controls().makeup_gain_db).max(0.0)
    } else {
        0.0
    };
    multiplier(eq_boost(&output.equalizer) + compressor)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn positive_bands_share_one_conservative_headroom_budget() {
        let mut settings = AppSettings::default();
        settings.equalizer.enabled = true;
        settings.equalizer.bands = vec![0.0; EQ_BAND_COUNT];
        settings.equalizer.bands[0] = 12.0;
        settings.equalizer.bands[1] = -6.0;
        settings.equalizer.bands[2] = 8.0;
        assert!((mic_headroom(&settings) - 0.1).abs() < 1e-9);
        settings.gain_safety = GainSafety::Unrestricted;
        assert_eq!(mic_headroom(&settings), 1.0);
        assert!(nodes(&settings, false).is_empty());
    }
    #[test]
    fn the_neural_pad_is_an_exact_unity_round_trip_only_where_it_belongs() {
        let pad = neural_pad().controls;
        let makeup = neural_makeup().controls;
        let multiplier = |controls: &[(&str, f64)]| {
            controls
                .iter()
                .find(|(name, _)| *name == "Mult")
                .expect("a linear node carries Mult")
                .1
        };
        // Bit-exact in float: the product is precisely one.
        assert_eq!(multiplier(&pad) * multiplier(&makeup), 1.0);
        assert!(multiplier(&pad) < 1.0);

        let mut settings = AppSettings::default();
        // The default model is GTCRN, whose gate thresholds live on the same
        // node and are in dB, so its input is never padded.
        assert!(!pads_neural_input(&settings, false));
        settings.noise_reduction.model = crate::config::NoiseModel::DeepFilterNet3;
        assert!(pads_neural_input(&settings, false));
        // The output chain only pads while it actually runs a model.
        assert!(!pads_neural_input(&settings, true));
        settings.output_filter.noise_reduction.model = crate::config::NoiseModel::DeepFilterNet3;
        assert!(pads_neural_input(&settings, true));
        // The advanced opt-out disables this along with the rest.
        settings.gain_safety = GainSafety::Unrestricted;
        assert!(!pads_neural_input(&settings, false));
        assert!(!pads_neural_input(&settings, true));
    }

    #[test]
    fn no_boost_keeps_unity_gain_and_a_normal_sample_ceiling() {
        let settings = AppSettings::default();
        assert_eq!(mic_headroom(&settings), 1.0);
        let protection = nodes(&settings, false);
        assert_eq!(protection[1].controls, vec![("Min", -1.0), ("Max", 1.0)]);
    }
}
