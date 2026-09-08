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
    fn no_boost_keeps_unity_gain_and_a_normal_sample_ceiling() {
        let settings = AppSettings::default();
        assert_eq!(mic_headroom(&settings), 1.0);
        let protection = nodes(&settings, false);
        assert_eq!(protection[1].controls, vec![("Min", -1.0), ("Max", 1.0)]);
    }
}
