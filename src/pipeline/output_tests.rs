use super::*;
use crate::config::AppSettings;

fn enabled_settings() -> AppSettings {
    AppSettings {
        output_filter: crate::config::OutputFilterSettings {
            enabled: true,
            ..crate::config::OutputFilterSettings::default()
        },
        ..mono_settings()
    }
}

#[test]
fn conf_declares_smart_filter_audio_sink() {
    // The output sink registers as a WirePlumber smart-filter on
    // the sink direction. The user's hardware sink stays the
    // visible default; the policy transparently inserts our chain
    // between every Stream/Output/Audio and that sink.
    let conf = build_output_conf(&enabled_settings());
    assert!(conf.contains("media.class = Audio/Sink"));
    assert!(conf.contains(&format!("node.name = \"{OUTPUT_NODE_NAME}\"")));
    assert!(conf.contains("filter.smart = true"));
    assert!(conf.contains(&format!("filter.smart.name = \"{OUTPUT_NODE_NAME}\"")));
}

#[test]
fn conf_playback_is_passive_stereo() {
    let conf = build_output_conf(&enabled_settings());
    assert!(conf.contains("node.name = \"output-biglinux-out\""));
    assert!(conf.contains("node.passive = true"));
    assert!(conf.contains("audio.position = [ FL FR ]"));
}

#[test]
fn conf_ignores_legacy_target_and_follows_current_default() {
    let mut settings = enabled_settings();
    settings.output_filter.target_sink_name = Some("alsa_output.old".to_owned());
    let conf = build_output_conf(&settings);
    assert!(!conf.contains("filter.smart.target ="));
}

#[test]
fn conf_mono_downmix_mixes_both_inputs_equally() {
    let conf = build_output_conf(&enabled_settings());
    assert!(conf.contains("\"Gain 1\" = 0.5"));
    assert!(conf.contains("\"Gain 2\" = 0.5"));
}

#[test]
fn conf_full_chain_is_linked() {
    let conf = build_output_conf(&enabled_settings());
    for link in [
        r#"{ output = "mixer:Out" input = "hpf:In" }"#,
        r#"{ output = "hpf:Out" input = "ai:Input" }"#,
        r#"{ output = "ai:Output" input = "gate:Input" }"#,
        r#"{ output = "gate:Output" input = "compressor:Input" }"#,
        r#"{ output = "compressor:Output" input = "eq:In 1" }"#,
        r#"{ output = "eq:Out 1" input = "copy_l:In" }"#,
        r#"{ output = "eq:Out 1" input = "copy_r:In" }"#,
    ] {
        assert!(conf.contains(link), "missing link: {link}");
    }
}

#[test]
fn output_conf_is_a_bare_module_args_body() {
    // The pwloader passes the file contents straight to
    // `pw_context_load_module(libpipewire-module-filter-chain, …)`
    // — bootstrap modules come from the daemon `client.conf`, never
    // from our args. Verifying the absence keeps a regression that
    // would re-introduce cross-process clock duplication visible.
    let s = AppSettings {
        output_filter: crate::config::OutputFilterSettings {
            enabled: true,
            noise_reduction: crate::config::NoiseReductionConfig {
                enabled: true,
                ..crate::config::NoiseReductionConfig::default()
            },
            ..crate::config::OutputFilterSettings::default()
        },
        ..mono_settings()
    };
    let conf = build_output_conf(&s);
    assert!(conf.starts_with('{'));
    assert!(conf.trim_end().ends_with('}'));
    assert!(!conf.contains("context.properties"));
    assert!(!conf.contains("context.modules"));
    assert!(!conf.contains("libpipewire-module-protocol-native"));
    assert!(!conf.contains("libpipewire-module-adapter"));
    assert!(!conf.contains("libpipewire-module-filter-chain"));
    // Filter graph itself is still rendered.
    assert!(conf.contains("gtcrn_mono"));
    assert!(conf.contains("filter.graph = {"));
}

#[test]
fn master_off_forces_full_bypass_regardless_of_sub_flags() {
    // Sub-effects look enabled in the user's settings, but the
    // master switch is off — every control must render in
    // pass-through. GTCRN stays in the topology with Enable=0 so
    // the live update path can flip it back without restarting the
    // unit (which would yank the smart-filter sink and pause
    // browsers).
    let s = AppSettings {
        output_filter: crate::config::OutputFilterSettings {
            enabled: false,
            noise_reduction: crate::config::NoiseReductionConfig {
                enabled: true,
                strength: 0.9,
                ..crate::config::NoiseReductionConfig::default()
            },
            hpf: crate::config::HpfConfig {
                enabled: true,
                frequency: 200.0,
            },
            gate: crate::config::GateConfig {
                enabled: true,
                intensity: 30,
            },
            compressor: crate::config::CompressorConfig {
                enabled: true,
                intensity: 0.7,
            },
            equalizer: crate::config::EqualizerConfig {
                enabled: true,
                bands: vec![6.0; EQ_BAND_COUNT],
                ..crate::config::EqualizerConfig::default()
            },
            target_sink_name: None,
            channel_mode: crate::config::OutputChannelMode::Mono,
        },
        ..mono_settings()
    };
    let conf = build_output_conf(&s);

    // GTCRN node must remain so the live path can re-enable it.
    assert!(conf.contains("name = \"ai\""));
    assert!(conf.contains(&format!("plugin = \"{LADSPA_GTCRN}\"")));
    assert!(
        conf.contains("\"Enable\" = 0.0"),
        "GTCRN must render with Enable=0 while master is off"
    );
    assert!(conf.contains("\"Freq\" = 5.0"), "HPF must pass through");
    assert!(
        conf.contains("\"Output select (-1 = key listen, 0 = gate, 1 = bypass)\" = 1.0"),
        "gate must bypass",
    );
    assert!(
        conf.contains("\"Ratio (1:n)\" = 1.0"),
        "compressor must run unity"
    );
    assert!(
        conf.contains("\"Makeup gain (dB)\" = 0.0"),
        "compressor must add no gain"
    );
    // EQ bands must read 0.00 dB so the user's preset doesn't bleed
    // through while the master is off.
    assert!(
        conf.matches("gain = 0.00").count() >= EQ_BAND_COUNT,
        "every EQ band should be flat at 0 dB while master is off"
    );
}

#[test]
fn nr_off_with_master_on_keeps_gtcrn_with_enable_zero() {
    // Master is on, sub-effects routed normally, but noise
    // reduction is off — the GTCRN node must remain wired with
    // Enable=0 so the user can re-toggle NR via the live path
    // without a service restart.
    let s = AppSettings {
        output_filter: crate::config::OutputFilterSettings {
            enabled: true,
            noise_reduction: crate::config::NoiseReductionConfig {
                enabled: false,
                ..crate::config::NoiseReductionConfig::default()
            },
            ..crate::config::OutputFilterSettings::default()
        },
        ..mono_settings()
    };
    let conf = build_output_conf(&s);
    assert!(conf.contains("name = \"ai\""));
    assert!(conf.contains("\"Enable\" = 0.0"));
    assert!(conf.contains(r#"{ output = "hpf:Out" input = "ai:Input" }"#));
    assert!(conf.contains(r#"{ output = "ai:Output" input = "gate:Input" }"#));
}

#[test]
fn master_on_eq_off_renders_flat_regardless_of_preset() {
    // The EQ sub-toggle is off but the user previously selected a
    // non-flat preset. The `param_eq` node must render flat — the
    // preset shaping must not leak through while EQ is disabled.
    let s = AppSettings {
        output_filter: crate::config::OutputFilterSettings {
            enabled: true,
            equalizer: crate::config::EqualizerConfig {
                enabled: false,
                preset: "vocal-boost".to_owned(),
                bands: vec![6.0; EQ_BAND_COUNT],
            },
            ..crate::config::OutputFilterSettings::default()
        },
        ..mono_settings()
    };
    let conf = build_output_conf(&s);
    assert!(
        conf.matches("gain = 0.00").count() >= EQ_BAND_COUNT,
        "every EQ band should be flat at 0 dB while EQ sub-toggle is off"
    );
}

#[test]
fn output_gtcrn_keeps_integrated_gate_parked_below_noise_floor() {
    // GTCRN's `Threshold (dB)` port (default `-60`) drives an
    // integrated noise gate. On the output chain the user-facing
    // gate is a separate SWH node, so the integrated one must
    // always render at the `-80 dB` sentinel — otherwise quiet
    // playback gets cut even when the UI gate toggle is off.
    let s = enabled_settings();
    let conf = build_output_conf(&s);
    assert!(
        conf.contains(r#""Threshold (dB)" = -80.0"#),
        "GTCRN integrated gate must be parked at -80 dB on the output chain"
    );
}

#[test]
fn conf_disabled_gate_bypasses_via_output_select() {
    let mut s = enabled_settings();
    s.output_filter.gate.enabled = false;
    let conf = build_output_conf(&s);
    assert!(conf.contains("\"Output select (-1 = key listen, 0 = gate, 1 = bypass)\" = 1.0"));
}

#[test]
fn conf_eq_emits_ten_bands() {
    let conf = build_output_conf(&enabled_settings());
    assert_eq!(conf.matches("type = bq_peaking").count(), EQ_BAND_COUNT);
}

#[test]
fn conf_graph_inputs_map_to_mixer() {
    let conf = build_output_conf(&enabled_settings());
    assert!(conf.contains(r#"inputs = [ "mixer:In 1" "mixer:In 2" ]"#));
    assert!(conf.contains(r#"outputs = [ "copy_l:Out" "copy_r:Out" ]"#));
}

#[test]
fn ai_processing_on_renders_gtcrn_enable_one() {
    // Master + NR on → GTCRN actually processes (Enable=1.0). Pins
    // output_ai_processing's true path (the off paths are covered above).
    let s = AppSettings {
        output_filter: crate::config::OutputFilterSettings {
            enabled: true,
            noise_reduction: crate::config::NoiseReductionConfig {
                enabled: true,
                ..crate::config::NoiseReductionConfig::default()
            },
            ..crate::config::OutputFilterSettings::default()
        },
        ..mono_settings()
    };
    let conf = build_output_conf(&s);
    assert!(
        conf.contains("\"Enable\" = 1.0"),
        "GTCRN must process (Enable=1) when master + NR are on: {conf}"
    );
}

#[test]
fn both_integrated_and_swh_gate_thresholds_park_at_floor() {
    // GTCRN's integrated gate is always parked at -80 dB; with the SWH gate
    // sub-toggle off its threshold parks at -80 too. Assert BOTH are present
    // (a count) so a sign flip on either threshold is caught and not masked
    // by the other -80 still being there.
    let mut s = enabled_settings();
    s.output_filter.gate.enabled = false;
    let conf = build_output_conf(&s);
    assert_eq!(
        conf.matches(r#""Threshold (dB)" = -80.0"#).count(),
        2,
        "GTCRN integrated gate + disabled SWH gate must both park at -80 dB: {conf}",
    );
}

#[test]
fn output_eq_prefers_explicit_bands_over_preset() {
    // Master + EQ on with a full explicit band set: the bands win over the
    // named preset (the `==` length check), reaching the conf verbatim.
    let mut bands = vec![0.0_f32; EQ_BAND_COUNT];
    bands[2] = 7.0;
    let s = AppSettings {
        output_filter: crate::config::OutputFilterSettings {
            enabled: true,
            equalizer: crate::config::EqualizerConfig {
                enabled: true,
                preset: "voice_boost".to_owned(),
                bands,
            },
            ..crate::config::OutputFilterSettings::default()
        },
        ..mono_settings()
    };
    let conf = build_output_conf(&s);
    assert!(
        conf.contains("gain = 7.00"),
        "explicit bands must render verbatim, not the preset: {conf}"
    );
    assert!(
        !conf.contains("gain = 20.00"),
        "voice_boost preset must be ignored"
    );
}

fn mono_settings() -> AppSettings {
    let mut settings = AppSettings::default();
    settings.output_filter.channel_mode = crate::config::OutputChannelMode::Mono;
    settings
}
