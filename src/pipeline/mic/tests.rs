// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests for the mic filter-chain conf builder. Split out of `mic.rs` (recipe:
//! file-size budget); as a child module they still reach `mic`'s private node
//! builders via `super::*`.

use super::*;
use crate::config::{AppSettings, EQ_BAND_COUNT, GateConfig};

fn default_settings() -> AppSettings {
    AppSettings::default()
}

#[test]
fn mic_snr_gate_is_gentler_when_the_aec_is_active() {
    let mut s = AppSettings::default();
    // Standalone: the quiet-pauses gate value is used as-is.
    s.echo_cancel.enabled = false;
    let standalone = mic_min_snr_gate_db(&s);
    assert_eq!(standalone, s.noise_reduction.min_snr_gate_db());
    // With the AEC upstream, the gate is capped gentler (never more aggressive
    // than -20 dB) so the denoiser stops over-suppressing double-talk voice.
    s.echo_cancel.enabled = true;
    let with_aec = mic_min_snr_gate_db(&s);
    assert!(with_aec <= -20.0, "gate not capped with AEC: {with_aec}");
    assert!(
        with_aec <= standalone,
        "AEC gate must not be more aggressive"
    );
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
    // The mode is the intent; the flag is derived from it by
    // `services::echo::settle`, which runs before the graph is written and
    // would put an `Automatic` flag straight back to `true`.
    assert_eq!(s.echo_cancel.mode, crate::config::EchoMode::Never);
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
        echo_cancel: crate::config::EchoCancelConfig {
            enabled: false,
            ..Default::default()
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(conf.contains("media.class = Audio/Source"));
    assert!(conf.contains("filter.smart = true"));
    assert!(conf.contains("filter.smart.name = \"big.filter-microphone\""));
    assert!(
        !conf.contains("filter.smart.target ="),
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
        echo_cancel: crate::config::EchoCancelConfig {
            enabled: true,
            ..Default::default()
        },
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
        conf.contains("node.latency = \"960/48000\""),
        "the mic chain requests a 20 ms latency; the LADSPA filters are block-agnostic",
    );
}

#[test]
fn conf_omits_capture_target_when_aec_disabled() {
    let s = AppSettings {
        echo_cancel: crate::config::EchoCancelConfig {
            enabled: false,
            ..Default::default()
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("target.object ="));
    assert!(
        conf.contains("node.latency = \"960/48000\""),
        "the block is the same with or without the canceller, and a multiple of every hop",
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
    // The stream enters at the head of the cascade, not at the second
    // stage — see `graph_input_is_never_a_link_destination`.
    assert!(conf.contains("inputs = [ \"hpf_pre:In\" ]"));
    // Both stages share the user-selected cutoff.
    let occurrences = conf.matches("\"Freq\" = 80.0").count();
    assert!(occurrences >= 2, "expected cascade to share cutoff: {conf}");
}

/// Collect the `inputs = [ … ]` entries and every link destination out
/// of a rendered args body.
fn graph_inputs_and_link_targets(conf: &str) -> (Vec<String>, Vec<String>) {
    let inputs = conf
        .lines()
        .find_map(|line| line.trim().strip_prefix("inputs = ["))
        .map(|rest| {
            rest.split('"')
                .skip(1)
                .step_by(2)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let targets = conf
        .lines()
        .filter_map(|line| line.trim().strip_prefix("{ output = "))
        .filter_map(|rest| rest.split_once("input = "))
        .map(|(_, input)| input.trim_end_matches(" }").trim_matches('"').to_owned())
        .collect();
    (inputs, targets)
}

#[test]
fn graph_input_is_never_a_link_destination() {
    // `spa.filter-graph` rejects the whole graph when a declared input
    // names a port some link already feeds: "input port hpf[0]:In
    // already used by link, use mixer" (-EBUSY), and the virtual source
    // then never starts. Reproduced on PipeWire 1.6.8 with the HPF
    // cascade on, where the head is `hpf_pre` but the input said `hpf`.
    for (label, settings) in [
        ("defaults", default_settings()),
        ("hpf on", {
            let mut s = default_settings();
            s.hpf.enabled = true;
            s
        }),
        ("hpf on, everything else off", {
            let mut s = default_settings();
            cascade_mic_off(&mut s);
            s.hpf.enabled = true;
            s
        }),
        ("hpf + voice changer", {
            let mut s = default_settings();
            s.hpf.enabled = true;
            s.stereo.enabled = true;
            s.stereo.mode = StereoMode::VoiceChanger;
            s
        }),
    ] {
        let conf = build_mic_conf(&settings);
        let (inputs, targets) = graph_inputs_and_link_targets(&conf);
        assert!(!inputs.is_empty(), "{label}: no graph inputs rendered");
        for input in &inputs {
            assert!(
                !targets.contains(input),
                "{label}: graph input {input} is also a link destination:\n{conf}",
            );
        }
    }
}

#[test]
fn conf_disabled_gate_drops_the_standalone_gate_node() {
    // The default plugin is attenuation-only: its gate is a separate SWH
    // node added only when the gate is enabled, so disabled means the node
    // is absent from the graph entirely.
    let mut s = default_settings();
    s.gate = GateConfig {
        enabled: false,
        intensity: 30,
    };
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("name = \"gate\""));
}

#[test]
fn conf_omits_the_denoiser_when_nr_and_gate_both_off() {
    let mut s = default_settings();
    s.noise_reduction.enabled = false;
    s.gate.enabled = false;
    // Force compressor on so we can verify the chain re-wires past
    // the missing AI node into the next live link.
    s.compressor.enabled = true;
    let conf = build_mic_conf(&s);
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
fn conf_minimal_chain_wires_denoiser_to_copies() {
    // Only NR on — the linear chain reduces to
    // hpf → ai → headroom → sample_ceiling → copies. The input pad is gone: it
    // cost ~25 dB of denoise and the output clamp already contains the overshoot.
    let mut s = default_settings();
    s.compressor.enabled = false;
    s.equalizer.enabled = false;
    s.gate.enabled = false;
    s.hpf.enabled = false;
    s.stereo.enabled = false;
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("neural_pad"));
    assert!(!conf.contains("neural_makeup"));
    assert!(conf.contains("{ output = \"hpf:Out\" input = \"ai:Audio In\" }"));
    assert!(conf.contains("{ output = \"ai:Audio Out\" input = \"headroom:In\" }"));
    assert!(conf.contains("{ output = \"sample_ceiling:Out\" input = \"copy_l:In\" }"));
    assert!(conf.contains("{ output = \"sample_ceiling:Out\" input = \"copy_r:In\" }"));
}

#[test]
fn conf_gate_only_uses_standalone_gate_without_the_neural_net() {
    // Attenuation-only default: the gate is a separate SWH node, so gate-on
    // with NR off drops the neural node entirely and keeps just the gate.
    let mut s = default_settings();
    s.noise_reduction.enabled = false;
    s.gate.enabled = true;
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("name = \"ai\""));
    assert!(conf.contains("name = \"gate\""));
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
fn conf_runs_the_chosen_model_and_its_voice_gate() {
    use crate::config::NoiseModel;
    let depth =
        |conf: &str, db: f64| conf.contains(&format!("\"Voice gate depth (dB)\" = {db:.1}"));
    let sensitivity = |conf: &str, level: f64| {
        conf.contains(&format!(
            "\"Voice sensitivity (1 sensitive, 3 normal, 5 strict)\" = {level:.1}"
        ))
    };
    for model in NoiseModel::ALL {
        let mut s = default_settings();
        s.noise_reduction.model = model;
        let conf = build_mic_conf(&s);
        let (plugin, label) = model.plugin_and_label();
        assert!(conf.contains(&format!("plugin = \"{plugin}\"")), "{conf}");
        assert!(conf.contains(&format!("label = \"{label}\"")), "{conf}");
        // On for DFN3, off on DPDFNet until asked for.
        let default_db = if model.is_dfn3() { 40.0 } else { 0.0 };
        assert!(depth(&conf, default_db), "{model:?}: {conf}");
        assert!(sensitivity(&conf, 3.0), "{model:?}: {conf}");
        s.noise_reduction.voice_sensitivity = 5;
        s.noise_reduction.voice_gate = false;
        s.noise_reduction.dpdfnet_voice_gate = true;
        let conf = build_mic_conf(&s);
        let switched_db = if model.is_dfn3() { 0.0 } else { 40.0 };
        assert!(depth(&conf, switched_db), "{model:?}: {conf}");
        assert!(sensitivity(&conf, 5.0), "{model:?}: {conf}");
    }
}

#[test]
fn conf_param_eq_emits_ten_bq_peaking_filters() {
    let mut s = default_settings();
    s.equalizer.enabled = true;
    // One moved band: a flat equalizer is not rendered at all.
    s.equalizer.bands[6] = 3.0;
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
    s.equalizer.bands[6] = 3.0;
    let conf = build_mic_conf(&s);
    for link in [
        "{ output = \"hpf:Out\" input = \"ai:Audio In\" }",
        "{ output = \"ai:Audio Out\" input = \"compressor:Input\" }",
        "{ output = \"compressor:Output\" input = \"eq:In 1\" }",
        "{ output = \"sample_ceiling:Out\" input = \"copy_l:In\" }",
        "{ output = \"sample_ceiling:Out\" input = \"copy_r:In\" }",
    ] {
        assert!(conf.contains(link), "missing link: {link}\n{conf}");
    }
    assert!(!conf.contains("neural_pad") && !conf.contains("neural_makeup"));
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
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Pitch co-efficient\" = 2.0"));
    // 2.0x pitch attenuates by 3 dB
    assert!(conf.contains("\"Amps gain (dB)\" = -3.0"));
    // Copies must now feed off the gain stage, not the EQ directly.
    assert!(conf.contains("{ output = \"sample_ceiling:Out\" input = \"copy_l:In\" }"));
    assert!(!conf.contains("{ output = \"eq:Out 1\" input = \"copy_l:In\" }"));
}

#[test]
fn conf_voice_changer_deep_voice_compensates_with_positive_gain() {
    let s = AppSettings {
        stereo: crate::config::StereoConfig {
            enabled: true,
            mode: crate::config::StereoMode::VoiceChanger,
            width: 0.0,
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
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(!conf.contains("\"Pitch co-efficient\""));
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
        ("voice changer", |s| {
            s.stereo.enabled = true;
            s.stereo.mode = crate::config::StereoMode::VoiceChanger;
        }),
        // The flag alone is deliberately not enough: see
        // `an_equalizer_that_shapes_nothing_does_not_keep_the_chain_alive`.
        ("equalizer", |s| {
            s.equalizer.enabled = true;
            s.equalizer.bands[6] = 3.0;
        }),
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
fn the_standalone_gate_follows_the_gate_switch() {
    let mut s = AppSettings::default();
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
        },
        ..AppSettings::default()
    };
    let conf = build_mic_conf(&s);
    assert!(conf.contains("\"Pitch co-efficient\" = 1.0"));
    assert!(conf.contains("\"Amps gain (dB)\" = 0.0"));
}

#[test]
fn pinned_aec_capture_must_not_fall_back_to_raw_audio() {
    let mut s = AppSettings::default();
    s.echo_cancel.enabled = true;
    let conf = build_mic_conf(&s);
    assert!(conf.contains("node.dont-fallback = true"));
    assert!(conf.contains("node.dont-reconnect = false"));
    assert!(conf.contains("filter.smart.targetable = true"));
    s.echo_cancel.enabled = false;
    assert!(!build_mic_conf(&s).contains("node.dont-fallback"));
}

#[test]
fn an_equalizer_that_shapes_nothing_does_not_keep_the_chain_alive() {
    // Reported from a live session: the noise filter was switched off and a
    // filter device stayed in the audio settings, with a loader process behind
    // it. The cause was an equalizer marked enabled with every band at 0 dB —
    // "flat" is one of the offered presets — so the chain was materialised to
    // run ten biquads computing the identity, with nothing on screen saying so.
    let mut s = AppSettings::default();
    cascade_mic_off(&mut s);
    assert!(!mic_chain_wanted(&s));

    s.equalizer.enabled = true;
    s.equalizer.bands = vec![0.0; EQ_BAND_COUNT];
    assert!(!mic_chain_wanted(&s), "a flat equalizer shapes nothing");
    assert!(!build_mic_conf(&s).contains("param_eq"));

    s.equalizer.bands[4] = 3.0;
    assert!(mic_chain_wanted(&s), "one moved band is a real equalizer");
    assert!(build_mic_conf(&s).contains("param_eq"));
}
