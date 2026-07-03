// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests for the mic filter-chain conf builder. Split out of `mic.rs` (recipe:
//! file-size budget); as a child module they still reach `mic`'s private node
//! builders via `super::*`.

use super::*;
use crate::config::{AppSettings, GateConfig};

fn default_settings() -> AppSettings {
    AppSettings::default()
}

#[test]
fn cascade_mic_off_drops_every_filter_chain_dependency() {
    // Defaults leave noise_reduction, echo_cancel and stereo on.
    // mic_chain_wanted ORs over all flags, so cascading must clear
    // every one of them — otherwise the simple-view master toggle
    // off leaves the mic loader running on the surviving
    // default-on flags and the user sees no process drop.
    let mut s = AppSettings::default();
    assert!(mic_chain_wanted(&s));

    cascade_mic_off(&mut s);

    assert!(!s.noise_reduction.enabled);
    assert!(!s.echo_cancel.enabled);
    assert!(!s.gate.enabled);
    assert!(!s.hpf.enabled);
    assert!(!s.stereo.enabled);
    assert!(!s.equalizer.enabled);
    assert!(!s.compressor.enabled);
    assert!(!mic_chain_wanted(&s));
}

#[test]
fn conf_is_a_bare_module_args_body() {
    // The pwloader hands the file contents straight to
    // `pw_context_load_module()` as the `args` C-string. No comment
    // header, no `context.modules` wrapper — just the SPA-JSON
    // `{ … }` block the filter-chain module knows how to parse.
    let conf = build_mic_conf(&default_settings());
    assert!(conf.starts_with('{'));
    assert!(conf.trim_end().ends_with('}'));
    assert!(!conf.contains("context.modules"));
    assert!(!conf.contains("libpipewire-module-filter-chain"));
    assert!(conf.contains("filter.graph = {"));
}

#[test]
fn conf_declares_smart_filter_when_aec_disabled() {
    // No AEC: WirePlumber's smart-filter policy keeps the visible
    // default source as the user's hardware mic and inserts
    // `mic-biglinux` between every default-following app and that
    // hardware. No `filter.smart.target` so the policy follows
    // whichever source the user picked as default. No
    // `filter.smart.before` either — there is no EC filter to
    // cascade with.
    let s = AppSettings {
        echo_cancel: crate::config::EchoCancelConfig { enabled: false },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(conf.contains("media.class = Audio/Source"));
    assert!(conf.contains("filter.smart = true"));
    assert!(conf.contains("filter.smart.name = \"big.filter-microphone\""));
    assert!(
        !conf.contains("filter.smart.target"),
        "smart filter must follow the default source, never pin to one node",
    );
    assert!(
        !conf.contains("filter.smart.before"),
        "no EC filter to cascade with when AEC is off",
    );
}

#[test]
fn conf_pins_capture_to_aec_source_when_enabled() {
    // AEC on: `mic-biglinux` remains the only smart source filter.
    // Its capture stream reads explicitly from `echo-cancel-source`
    // so WirePlumber cannot link it directly to the hardware mic and
    // bypass the canceller.
    let s = AppSettings {
        echo_cancel: crate::config::EchoCancelConfig { enabled: true },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(conf.contains("filter.smart = true"));
    assert!(conf.contains("filter.smart.name = \"big.filter-microphone\""));
    assert!(
        conf.contains("target.object = \"echo-cancel-source\""),
        "AEC mode must pin the mic chain to the cleaned source",
    );
    assert!(!conf.contains("filter.smart.before"));
    assert!(
        !conf.contains("priority.session"),
        "mic-biglinux must not promote itself as the visible default",
    );
    assert!(
        conf.contains("node.latency = \"1920/48000\""),
        "when AEC is upstream, the mic chain must follow AEC's 40 ms (4× WebRTC frame) quantum",
    );
}

#[test]
fn conf_omits_capture_target_when_aec_disabled() {
    let s = AppSettings {
        echo_cancel: crate::config::EchoCancelConfig { enabled: false },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("target.object ="));
    assert!(
        conf.contains("node.latency = \"1024/48000\""),
        "without AEC, keep the regular GTCRN-friendly quantum",
    );
}

#[test]
fn conf_hpf_frequency_matches_settings() {
    let mut s = default_settings();
    s.hpf.enabled = true;
    s.hpf.frequency = 80.0;
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Freq\" = 80.0"));
}

#[test]
fn conf_disabled_hpf_becomes_pass_through() {
    let mut s = default_settings();
    s.hpf.enabled = false;
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Freq\" = 5.0"));
    // Only the placeholder "hpf" exists when disabled — no cascade.
    assert!(!conf.contains("name = \"hpf_pre\""));
}

#[test]
fn conf_enabled_hpf_cascades_two_biquads() {
    // Two identical Butterworth biquads (Q=0.707) at the same
    // cutoff form a 24 dB/oct Linkwitz-Riley high-pass — required
    // for the rolloff to clean rumble on laptop mics. Both nodes
    // must be present and feed each other before the rest of the
    // chain.
    let mut s = default_settings();
    s.hpf.enabled = true;
    s.hpf.frequency = 80.0;
    let conf = build_mic_conf(&s);
    assert!(conf.contains("name = \"hpf_pre\""));
    assert!(conf.contains("name = \"hpf\""));
    assert!(conf.contains("{ output = \"hpf_pre:Out\" input = \"hpf:In\" }"));
    // Both stages share the user-selected cutoff.
    let occurrences = conf.matches("\"Freq\" = 80.0").count();
    assert!(occurrences >= 2, "expected cascade to share cutoff: {conf}");
}

#[test]
fn hpf_default_frequency_protects_voice_fundamental() {
    // 80 Hz keeps the lowest adult-male F0 (~85 Hz) intact while
    // killing HVAC/fan rumble and 50/60 Hz mains harmonics. If
    // someone bumps this they should weigh the impact on bass
    // voices first.
    assert!((crate::config::HPF_FREQUENCY_DEFAULT - 80.0).abs() < f32::EPSILON);
}

#[test]
fn conf_disabled_gate_pushes_threshold_below_floor() {
    // GTCRN stays in the chain (noise reduction is still on by
    // default) so the integrated gate's threshold control is what
    // we verify here.
    let mut s = default_settings();
    s.gate = GateConfig {
        enabled: false,
        intensity: 30,
    };
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Threshold (dB)\" = -80.0"));
}

#[test]
fn conf_omits_gtcrn_when_nr_and_gate_both_off() {
    // GTCRN is a phase-vocoder + ONNX inference: the LADSPA wrapper
    // runs STFT/iSTFT every block regardless of `Enable`, so we
    // skip the node entirely when nothing actually needs it. The
    // integrated gate shares the same instance, hence the
    // double-condition.
    let mut s = default_settings();
    s.noise_reduction.enabled = false;
    s.gate.enabled = false;
    // Force compressor on so we can verify the chain re-wires past
    // the missing AI node into the next live link.
    s.compressor.enabled = true;
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("plugin = \"/usr/lib/ladspa/libgtcrn_ladspa.so\""));
    assert!(!conf.contains("name = \"ai\""));
    // Chain must skip the node and link hpf straight into the
    // compressor — otherwise the graph would have a dangling edge.
    assert!(conf.contains("{ output = \"hpf:Out\" input = \"compressor:Input\" }"));
}

#[test]
fn conf_omits_compressor_when_disabled() {
    // SC4 mono runs RMS detection + envelope per block even at
    // unity ratio. With the flag off we drop the node entirely so
    // the linker rewires the previous stage into the next live
    // node (or directly into the stereo fan-out copies).
    let mut s = default_settings();
    s.compressor.enabled = false;
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("name = \"compressor\""));
    assert!(!conf.contains("plugin = \"/usr/lib/ladspa/sc4m_1916.so\""));
}

#[test]
fn conf_omits_param_eq_when_disabled() {
    // 10 cascaded `bq_peaking` biquads run every block regardless
    // of gain. With the flag off we drop the param_eq node
    // entirely.
    let mut s = default_settings();
    s.equalizer.enabled = false;
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("name = \"eq\""));
    assert!(!conf.contains("type = bq_peaking"));
}

#[test]
fn conf_minimal_chain_links_hpf_directly_to_copies() {
    // Only NR on — the linear chain reduces to hpf → ai → copies.
    let mut s = default_settings();
    s.compressor.enabled = false;
    s.equalizer.enabled = false;
    s.gate.enabled = false;
    s.hpf.enabled = false;
    s.stereo.enabled = false;
    let conf = build_mic_conf(&s);
    assert!(conf.contains("{ output = \"hpf:Out\" input = \"ai:Input\" }"));
    assert!(conf.contains("{ output = \"ai:Output\" input = \"copy_l:In\" }"));
    assert!(conf.contains("{ output = \"ai:Output\" input = \"copy_r:In\" }"));
}

#[test]
fn conf_keeps_gtcrn_when_only_gate_is_on() {
    // Gate alone still requires the integrated GTCRN-side gate, so
    // the node must stay even with NR off.
    let mut s = default_settings();
    s.noise_reduction.enabled = false;
    s.gate.enabled = true;
    let conf = build_mic_conf(&s);
    assert!(conf.contains("plugin = \"/usr/lib/ladspa/libgtcrn_ladspa.so\""));
    assert!(conf.contains("\"Enable\" = 0.0"));
    assert!(conf.contains("{ output = \"hpf:Out\" input = \"ai:Input\" }"));
}

#[test]
fn conf_enabled_compressor_uses_derived_ratio_and_makeup() {
    // When the compressor is on its node carries the user's
    // intensity-derived ratio and makeup gain — proves we still
    // route the live values into the LADSPA controls now that the
    // node is no longer always-instantiated.
    let mut s = default_settings();
    s.compressor.enabled = true;
    s.compressor.intensity = 1.0;
    let conf = build_mic_conf(&s);
    assert!(conf.contains("name = \"compressor\""));
    // Intensity 1.0 derives non-trivial ratio and positive makeup.
    assert!(!conf.contains("\"Ratio (1:n)\" = 1.0"));
    assert!(!conf.contains("\"Makeup gain (dB)\" = 0.0"));
}

#[test]
fn conf_gtcrn_model_control_matches_variant() {
    let s = default_settings();
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Model\" = 0.0"));

    let mut s = default_settings();
    s.noise_reduction.model = crate::config::NoiseModel::GtcrnVctk;
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Model\" = 1.0"));
}

#[test]
fn conf_param_eq_emits_ten_bq_peaking_filters() {
    let mut s = default_settings();
    s.equalizer.enabled = true;
    let conf = build_mic_conf(&s);
    let count = conf.matches("type = bq_peaking").count();
    assert_eq!(count, EQ_BAND_COUNT);
    // Frequencies must appear in ascending order
    let mut last = 0_u32;
    for f in EQ_BANDS_HZ {
        assert!(
            conf.contains(&format!("freq = {f}")),
            "missing EQ band {f}Hz in conf",
        );
        assert!(f > last);
        last = f;
    }
}

#[test]
fn conf_wires_full_chain_links_without_pitch() {
    // Force every linear sub-effect on so the full default-style
    // pipeline is materialised (compressor + EQ default to off).
    let mut s = default_settings();
    s.compressor.enabled = true;
    s.equalizer.enabled = true;
    let conf = build_mic_conf(&s);
    for link in [
        "{ output = \"hpf:Out\" input = \"ai:Input\" }",
        "{ output = \"ai:Output\" input = \"compressor:Input\" }",
        "{ output = \"compressor:Output\" input = \"eq:In 1\" }",
        "{ output = \"eq:Out 1\" input = \"copy_l:In\" }",
        "{ output = \"eq:Out 1\" input = \"copy_r:In\" }",
    ] {
        assert!(conf.contains(link), "missing link: {link}\n{conf}");
    }
    assert!(!conf.contains("name = \"pitch\""));
    assert!(!conf.contains("name = \"pitch_gain\""));
}

#[test]
fn conf_omits_pitch_node_when_voice_changer_off() {
    let conf = build_mic_conf(&default_settings());
    // Pitch shifter is a phase vocoder — never emit at unity, the
    // STFT/iSTFT pass alone audibly smears transients.
    assert!(!conf.contains("\"Pitch co-efficient\""));
    assert!(!conf.contains("plugin = \"/usr/lib/ladspa/pitch_scale_1193.so\""));
}

#[test]
fn conf_voice_changer_emits_pitch_node_and_rewires_chain() {
    let s = AppSettings {
        stereo: crate::config::StereoConfig {
            enabled: true,
            mode: crate::config::StereoMode::VoiceChanger,
            width: 1.0,
            ..crate::config::StereoConfig::default()
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Pitch co-efficient\" = 2.0"));
    // 2.0x pitch attenuates by 3 dB
    assert!(conf.contains("\"Amps gain (dB)\" = -3.0"));
    // Copies must now feed off the gain stage, not the EQ directly.
    assert!(conf.contains("{ output = \"pitch_gain:Output\" input = \"copy_l:In\" }"));
    assert!(!conf.contains("{ output = \"eq:Out 1\" input = \"copy_l:In\" }"));
}

#[test]
fn conf_voice_changer_deep_voice_compensates_with_positive_gain() {
    let s = AppSettings {
        stereo: crate::config::StereoConfig {
            enabled: true,
            mode: crate::config::StereoMode::VoiceChanger,
            width: 0.0,
            ..crate::config::StereoConfig::default()
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Pitch co-efficient\" = 0.5"));
    // (1.0 - 0.5) * 20 = +10 dB
    assert!(conf.contains("\"Amps gain (dB)\" = 10.0"));
}

#[test]
fn voice_changer_mid_high_width_uses_exponential_pitch_curve() {
    // width=0.75 sits between passthrough and full high voice:
    // 0.5 * 4^0.75 = sqrt(2), then attenuate by (sqrt(2) - 1) * 3 dB.
    let s = AppSettings {
        stereo: crate::config::StereoConfig {
            enabled: true,
            mode: crate::config::StereoMode::VoiceChanger,
            width: 0.75,
            ..crate::config::StereoConfig::default()
        },
        ..AppSettings::default()
    };
    let (coeff, gain_db) = pitch_controls(&s).expect("voice changer controls");
    assert!((coeff - std::f64::consts::SQRT_2).abs() < 1e-12);
    assert!((gain_db + 1.242_640_687_119_285_4).abs() < 1e-12);
}

#[test]
fn conf_dual_mono_stereo_does_not_emit_pitch() {
    let s = AppSettings {
        stereo: crate::config::StereoConfig {
            enabled: true,
            mode: crate::config::StereoMode::DualMono,
            width: 1.0,
            ..crate::config::StereoConfig::default()
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("\"Pitch co-efficient\""));
}

#[test]
fn conf_does_not_use_optional_zeroramp_builtin() {
    // Older PipeWire versions don't ship the `zeroramp` builtin and
    // refuse to load the whole filter-chain when it appears in the
    // graph. Keep the chain to widely-available builtins.
    let conf = build_mic_conf(&default_settings());
    assert!(
        !conf.contains("zeroramp"),
        "mic chain must not depend on the optional `zeroramp` builtin",
    );
}

#[test]
fn conf_exposes_stereo_fanout_on_graph_outputs() {
    let conf = build_mic_conf(&default_settings());
    assert!(conf.contains(r#"outputs = [ "copy_l:Out" "copy_r:Out" ]"#));
}

#[test]
fn each_filter_flag_alone_keeps_chain_wanted() {
    // mic_chain_wanted ORs over every effect flag. Each flag ALONE (from an
    // all-off baseline) must still want the chain — this pins every `||`
    // operator (an `&&` would drop the chain when only one flag is enabled).
    let setters: [(&str, fn(&mut AppSettings)); 7] = [
        ("noise_reduction", |s| s.noise_reduction.enabled = true),
        ("gate", |s| s.gate.enabled = true),
        ("hpf", |s| s.hpf.enabled = true),
        ("stereo", |s| s.stereo.enabled = true),
        ("equalizer", |s| s.equalizer.enabled = true),
        ("compressor", |s| s.compressor.enabled = true),
        ("echo_cancel", |s| s.echo_cancel.enabled = true),
    ];
    for (name, enable) in setters {
        let mut s = AppSettings::default();
        cascade_mic_off(&mut s);
        assert!(!mic_chain_wanted(&s), "baseline must be all-off");
        enable(&mut s);
        assert!(mic_chain_wanted(&s), "{name} alone must want the mic chain");
    }
}

#[test]
fn deepfilter_drops_standalone_gate_unless_gate_enabled() {
    // DFN3 has no integrated gate, so a standalone SWH gate node is wired in
    // ONLY when the model is DeepFilterNet3 AND the silence gate is on (the
    // `&&` in mic_nodes).
    let mut s = AppSettings::default();
    s.noise_reduction.model = crate::config::NoiseModel::DeepFilterNet3;
    s.noise_reduction.enabled = true;
    s.gate.enabled = false;
    let conf = build_mic_conf(&s);
    assert!(
        !conf.contains("name = \"gate\""),
        "no standalone gate when the silence gate is off: {conf}",
    );

    s.gate.enabled = true;
    let conf = build_mic_conf(&s);
    assert!(
        conf.contains("name = \"gate\""),
        "DFN3 + gate on must add the standalone SWH gate",
    );
}

#[test]
fn param_eq_prefers_explicit_bands_over_preset() {
    // EQ_BAND_COUNT explicit bands win over the named preset (the `==` length
    // check in param_eq_node). A sentinel gain must reach the conf verbatim and
    // the preset's distinctive band must not.
    let mut bands = vec![0.0_f32; EQ_BAND_COUNT];
    bands[2] = 7.0;
    let s = AppSettings {
        equalizer: crate::config::EqualizerConfig {
            enabled: true,
            bands,
            preset: "voice_boost".to_string(),
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(
        conf.contains("gain = 7.00"),
        "explicit bands must be used verbatim, not the preset: {conf}",
    );
    assert!(
        !conf.contains("gain = 20.00"),
        "voice_boost preset must be ignored"
    );
}

#[test]
fn param_eq_falls_back_to_named_preset_when_bands_wrong_length() {
    // A wrong-length band vector resolves the named preset
    // (resolve_preset_or_flat): voice_boost yields EQ_BAND_COUNT bands with a
    // distinctive +20 dB and -10 dB band.
    let s = AppSettings {
        equalizer: crate::config::EqualizerConfig {
            enabled: true,
            bands: Vec::new(),
            preset: "voice_boost".to_string(),
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert_eq!(conf.matches("type = bq_peaking").count(), EQ_BAND_COUNT);
    assert!(
        conf.contains("gain = 20.00"),
        "voice_boost +20 band: {conf}"
    );
    assert!(conf.contains("gain = -10.00"), "voice_boost -10 band");
}

#[test]
fn voice_changer_unity_width_has_zero_gain_compensation() {
    // width=0.5 → coeff exactly 1.0 → no gain compensation (the passthrough
    // boundary, where both comparison branches in pitch_controls agree).
    let s = AppSettings {
        stereo: crate::config::StereoConfig {
            enabled: true,
            mode: crate::config::StereoMode::VoiceChanger,
            width: 0.5,
            ..crate::config::StereoConfig::default()
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Pitch co-efficient\" = 1.0"));
    assert!(conf.contains("\"Amps gain (dB)\" = 0.0"));
}
