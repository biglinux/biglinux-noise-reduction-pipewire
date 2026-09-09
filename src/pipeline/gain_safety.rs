//! Conservative static headroom plus a zero-lookahead sample ceiling.
//! The headroom is the peak of the equalizer's own frequency response — the
//! most any single instant can be amplified — so it protects against clipping
//! without needlessly attenuating. A final clamp contains transients; it can
//! distort an overloaded signal and is not an oversampled true-peak limiter.
use super::nodes::Node;
use crate::config::{
    AppSettings, EQ_BAND_COUNT, EQ_BAND_MAX, EQ_BANDS_HZ, EqualizerConfig, GainSafety, StereoMode,
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

/// Graph sample rate. Every chain declares `node.latency = "…/48000"`; the EQ
/// biquad response is evaluated at the rate the filters actually run at.
const EQ_RATE_HZ: f64 = 48_000.0;

/// The largest amount, in dB and never negative, that the ten-band equalizer
/// amplifies any frequency.
///
/// The bands are one octave apart at Q = 1.41, so their responses barely
/// overlap. Summing the ten gains — which this once did — treats a graphic EQ
/// as if every band peaked at the same frequency: a routine boosty preset
/// (5 + 5 + 10 + 5 + 5 + 10 + 5 = 45 dB) drove the headroom to 10^(-45/20), a
/// −45 dB attenuation that muted the microphone and looked like it had stopped
/// capturing. The real ceiling is the highest point of the cascade's magnitude
/// response, so build it and take that.
///
/// Only boosts can push a sample toward the clamp, so cuts are floored to 0
/// before the response is summed: a cut at one band must never be credited
/// against a boost at another and let the peak through unprotected.
fn eq_boost(eq: &EqualizerConfig) -> f64 {
    if !eq.enabled {
        return 0.0;
    }
    let bands = if eq.bands.len() == EQ_BAND_COUNT {
        eq.bands.clone()
    } else {
        eq_preset_bands(&eq.preset).map_or_else(|| vec![0.0; EQ_BAND_COUNT], |bands| bands.to_vec())
    };
    let boosts: Vec<f64> = bands
        .into_iter()
        .map(|gain| {
            if gain.is_finite() {
                f64::from(gain.clamp(0.0, EQ_BAND_MAX))
            } else {
                0.0
            }
        })
        .collect();
    if boosts.iter().all(|&gain| gain == 0.0) {
        return 0.0;
    }

    // Sample the cascade every 1/12 octave from 20 Hz up: finer than the
    // one-octave band spacing, and cheap — this runs once per settings change,
    // never per audio sample.
    let step = 2.0_f64.powf(1.0 / 12.0);
    let mut peak_db = 0.0_f64;
    let mut freq = 20.0_f64;
    while freq <= EQ_RATE_HZ / 2.0 {
        let response: f64 = boosts
            .iter()
            .zip(EQ_BANDS_HZ.iter())
            .map(|(&gain, &center)| peaking_gain_db(freq, f64::from(center), 1.41, gain))
            .sum();
        peak_db = peak_db.max(response);
        freq *= step;
    }
    peak_db.max(0.0)
}

/// Magnitude, in dB, of one RBJ peaking-EQ biquad at frequency `f`.
///
/// The same coefficient formulas the `param_eq` builtin uses, evaluated on the
/// unit circle, so this reads the response the graph will actually produce
/// rather than an approximation of it.
fn peaking_gain_db(f: f64, f0: f64, q: f64, gain_db: f64) -> f64 {
    if gain_db == 0.0 {
        return 0.0;
    }
    let a = 10.0_f64.powf(gain_db / 40.0);
    let w0 = std::f64::consts::TAU * f0 / EQ_RATE_HZ;
    let alpha = w0.sin() / (2.0 * q);
    let cos_w0 = w0.cos();
    let (b0, b1, b2) = (1.0 + alpha * a, -2.0 * cos_w0, 1.0 - alpha * a);
    let (a0, a1, a2) = (1.0 + alpha / a, -2.0 * cos_w0, 1.0 - alpha / a);

    let w = std::f64::consts::TAU * f / EQ_RATE_HZ;
    let (cos1, cos2) = (w.cos(), (2.0 * w).cos());
    let (sin1, sin2) = (w.sin(), (2.0 * w).sin());
    let num = ((b0 + b1 * cos1 + b2 * cos2).powi(2) + (b1 * sin1 + b2 * sin2).powi(2)).sqrt();
    let den = ((a0 + a1 * cos1 + a2 * cos2).powi(2) + (a1 * sin1 + a2 * sin2).powi(2)).sqrt();
    20.0 * (num / den).log10()
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
    fn separated_bands_do_not_stack_into_a_muting_headroom() {
        let mut settings = AppSettings::default();
        settings.equalizer.enabled = true;
        settings.equalizer.bands = vec![0.0; EQ_BAND_COUNT];
        settings.equalizer.bands[0] = 12.0; // 31 Hz
        settings.equalizer.bands[1] = -6.0; // 63 Hz — a cut adds no gain
        settings.equalizer.bands[2] = 8.0; // 125 Hz, two octaves from the 12 dB
        // The 12 dB band is the peak; the others are octaves away and barely
        // reach it. Summing the boosts (20 dB → 0.1) would over-attenuate by
        // 8 dB. The real cascade peak is 12.25 dB.
        assert!((mic_headroom(&settings) - 0.244_047).abs() < 1e-4);
        settings.gain_safety = GainSafety::Unrestricted;
        assert_eq!(mic_headroom(&settings), 1.0);
        assert!(nodes(&settings, false).is_empty());
    }

    #[test]
    fn a_boosty_preset_does_not_mute_the_microphone() {
        // The exact bands that shipped as a preset and, summed to 45 dB,
        // drove the headroom to 0.0056 (−45 dB) — the microphone looked like
        // it had stopped capturing. Its real response peaks at 12.1 dB.
        let mut settings = AppSettings::default();
        settings.equalizer.enabled = true;
        settings.equalizer.bands = vec![5.0, 5.0, 10.0, 5.0, 0.0, 5.0, 10.0, 5.0, 0.0, -2.0];
        let headroom = mic_headroom(&settings);
        assert!(
            (headroom - 0.247_786).abs() < 1e-4,
            "headroom was {headroom}"
        );
        // Far above the −45 dB the arithmetic sum produced.
        assert!(headroom > 0.2);
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
