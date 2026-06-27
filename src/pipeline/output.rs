//! Output (playback) filter chain generator.
//!
//! Use case: the user is on a meeting / podcast / call and wants to
//! suppress noise on the audio they *hear*. The filter-chain is exposed
//! as a WirePlumber 0.5 **smart filter** on the audio sink direction —
//! mirroring how the mic chain attaches to the default audio source.
//! WirePlumber transparently inserts the filter between every stream
//! and the user's current default sink, so enabling the toggle does
//! not require changing the default audio device or per-app routing.
//!
//! The output chain runs in its own `biglinux-microphone-pwloader`
//! process (managed by `biglinux-microphone-output.service`) — but
//! that loader connects as a *client* of the main PipeWire daemon, so
//! every filter graph in the package shares the daemon's clock. No
//! cross-process drift, no `spa.alsa: front:1p ... resync` events.
//! Keeping mic and output in separate loaders is still cheap and it
//! lets each unit have an independent lifecycle (toggling the output
//! filter on/off does not touch the mic chain).
//!
//! Because GTCRN is mono, the incoming stereo is first summed via the
//! `mixer` builtin, processed through a single mono chain, and then
//! fanned out with two `copy` nodes to the `FL`/`FR` playback ports:
//!
//! ```text
//! FL,FR → mixer → hpf → gtcrn → gate → compressor → param_eq
//!                                                  ├→ copy_l → FL
//!                                                  └→ copy_r → FR
//! ```
//!
//! GTCRN is **permanently** wired into the output graph: its `Enable`
//! port is flipped between 0 and 1 by the live-controls path on master
//! / NR toggle. Dropping it from the topology would mean rewriting the
//! conf and reloading the unit, which yanks the smart-filter sink out
//! from under any active stream — Chromium-based browsers pause
//! HTMLMediaElement playback the moment their target sink disappears.
//! Keeping the node alive trades a constant STFT/iSTFT + ONNX cost for
//! gap-free toggling. The standalone SWH gate is independent and also
//! stays always-instantiated — `Output select = 1.0` is a true bypass.

use std::fmt::Write as _;

use crate::config::{
    deepfilter_attenuation_db, eq_preset_bands, gtcrn_speech_strength, AppSettings,
    CompressorDerived, GateDerived, EQ_BANDS_HZ, EQ_BAND_COUNT,
};

use super::graph::{Graph, Link, RenderMode};
use super::nodes::{
    Node, LABEL_BQ_HIGHPASS, LABEL_COPY, LABEL_DEEPFILTER_MONO, LABEL_GTCRN_MONO, LABEL_MIXER,
    LABEL_PARAM_EQ, LABEL_SC4_MONO, LABEL_SWH_GATE, LADSPA_DEEPFILTER, LADSPA_GTCRN,
    LADSPA_SC4_MONO, LADSPA_SWH_GATE,
};

/// Stem used as the `node.name` of the output virtual sink.
pub const OUTPUT_NODE_NAME: &str = "output-biglinux";
pub const OUTPUT_DESCRIPTION: &str = "BigLinux Output Filter";
/// File name of the output args body, consumed by
/// `biglinux-microphone-pwloader` (started by
/// `biglinux-microphone-output.service`).
pub const OUTPUT_CONF_FILE: &str = "output.args";

/// Render the output filter-chain config text for the current settings.
#[must_use]
pub fn build_output_conf(settings: &AppSettings) -> String {
    let nodes = output_nodes(settings);
    let links = output_links(&nodes);

    let graph = Graph {
        description: OUTPUT_DESCRIPTION.into(),
        media_name: OUTPUT_DESCRIPTION.into(),
        nodes,
        links,
        // Apps write into the mixer's two `In` ports (FL and FR).
        inputs: vec!["mixer:In 1".into(), "mixer:In 2".into()],
        outputs: vec!["copy_l:Out".into(), "copy_r:Out".into()],
        capture_props: capture_props(settings.output_filter.target_sink_name.as_deref()),
        playback_props: playback_props(),
    };

    graph.render(RenderMode::ModuleArgs)
}

/// True when GTCRN should *process* (Enable=1.0) inside the output
/// graph. The node itself is always present in the topology — gating
/// processing via the LADSPA `Enable` port keeps toggling the master
/// switch a live-update operation, with no service restart and no
/// dropout. Otherwise toggling off would drop the node from the conf
/// but leave the running unit untouched (we deliberately keep it up
/// so Chromium-based browsers don't pause playback), and the live
/// AI would keep processing with stale controls.
#[must_use]
pub fn output_ai_processing(settings: &AppSettings) -> bool {
    let of = &settings.output_filter;
    of.enabled && of.noise_reduction.enabled
}

fn output_nodes(settings: &AppSettings) -> Vec<Node> {
    let of = &settings.output_filter;
    // Master switch: when off, every sub-effect is forced to bypass so
    // the graph stays loaded and the smart-filter sink keeps streams
    // attached. Stopping the unit instead would make Chromium-based
    // browsers pause HTMLMediaElement playback the moment their target
    // sink disappears.
    let master = of.enabled;
    let hpf_enabled = master && of.hpf.enabled;
    let gate_enabled = master && of.gate.enabled;
    let comp_enabled = master && of.compressor.enabled;
    let ai_processing = output_ai_processing(settings);

    let hpf_freq = if hpf_enabled {
        f64::from(of.hpf.frequency)
    } else {
        5.0
    };

    let nr = &of.noise_reduction;
    // Backend swap (GTCRN ↔ DFN3) is a topology change — selecting DFN3
    // emits a different LADSPA plugin with a different control surface
    // and port names. The reconciler treats `model` changes as
    // restart-worthy, so live-toggling between the two is intentionally
    // not graceful (one-shot restart on swap).
    let denoiser = if nr.model.is_deepfilter() {
        // DFN3 has no `Enable` port, so master-off / NR-off renders the
        // node with `Attenuation Limit = 0` to make it a passthrough.
        let atten_db = if ai_processing {
            deepfilter_attenuation_db(nr.strength)
        } else {
            0.0
        };
        Node::ladspa("ai", LADSPA_DEEPFILTER, LABEL_DEEPFILTER_MONO)
            .with_ports("Audio In", "Audio Out")
            .with_controls([("Attenuation Limit (dB)", atten_db)])
    } else {
        // GTCRN ships with an integrated noise gate fused into `run()`
        // (port `Threshold (dB)`, default `-60`). On the mic chain that
        // integrated gate is the user-facing one; on the output chain
        // the user-facing gate is a separate SWH `gate_1410` node and
        // GTCRN's internal gate must stay permanently off — otherwise
        // quiet playback gets cut every time the signal dips below
        // -60 dBFS, even with the UI gate toggle disabled.
        //
        // `-80 dB` is the sentinel the rest of the codebase uses to
        // park the integrated gate below the noise floor (the port's
        // valid range is `-80..0`, so -80 is the lowest threshold that
        // never triggers).
        Node::ladspa("ai", LADSPA_GTCRN, LABEL_GTCRN_MONO).with_controls([
            ("Enable", if ai_processing { 1.0 } else { 0.0 }),
            ("Strength", f64::from(nr.strength)),
            ("Model", f64::from(nr.model.ladspa_control())),
            ("SpeechStrength", gtcrn_speech_strength(nr.strength)),
            ("LookaheadMs", f64::from(nr.lookahead_ms)),
            ("ModelBlend", f64::from(nr.model_blending)),
            ("VoiceRecovery", f64::from(nr.voice_recovery)),
            ("Threshold (dB)", -80.0),
        ])
    };

    let gate_d = GateDerived::from_config(&of.gate);
    let gate_threshold = if gate_enabled {
        gate_d.threshold_db
    } else {
        // SWH `gate_1410` also has an "Output select" passthrough control
        // (1.0 = bypass). Using a below-floor threshold keeps the graph
        // edit-local so runtime toggles don't require a reload.
        -80.0
    };

    let gate = Node::ladspa("gate", LADSPA_SWH_GATE, LABEL_SWH_GATE).with_controls([
        ("Threshold (dB)", gate_threshold),
        ("Attack (ms)", gate_d.attack_ms),
        ("Hold (ms)", gate_d.hold_ms),
        ("Decay (ms)", gate_d.release_ms),
        ("Range (dB)", gate_d.range_db),
        ("LF key filter (Hz)", 200.0),
        ("HF key filter (Hz)", 6000.0),
        (
            "Output select (-1 = key listen, 0 = gate, 1 = bypass)",
            if gate_enabled { 0.0 } else { 1.0 },
        ),
    ]);

    let comp_d = CompressorDerived::from_config(&of.compressor);
    let compressor = Node::ladspa("compressor", LADSPA_SC4_MONO, LABEL_SC4_MONO).with_controls([
        ("RMS/peak", f64::from(comp_d.rms_peak)),
        ("Attack time (ms)", f64::from(comp_d.attack_ms)),
        ("Release time (ms)", f64::from(comp_d.release_ms)),
        (
            "Threshold level (dB)",
            if comp_enabled {
                f64::from(comp_d.threshold_db)
            } else {
                0.0
            },
        ),
        (
            "Ratio (1:n)",
            if comp_enabled {
                f64::from(comp_d.ratio)
            } else {
                1.0
            },
        ),
        ("Knee radius (dB)", f64::from(comp_d.knee_db)),
        (
            "Makeup gain (dB)",
            if comp_enabled {
                f64::from(comp_d.makeup_gain_db)
            } else {
                0.0
            },
        ),
    ]);

    vec![
        // FL + FR → mono. Both inputs feed channel 1 of the mixer; channel
        // 2 stays at gain 0 so it contributes nothing.
        Node::builtin("mixer", LABEL_MIXER).with_controls([("Gain 1", 0.5), ("Gain 2", 0.5)]),
        Node::builtin("hpf", LABEL_BQ_HIGHPASS).with_controls([("Freq", hpf_freq), ("Q", 0.707)]),
        // GTCRN is always wired in; toggling master flips its Enable
        // port between 0 and 1, which the live update path can push
        // without restarting the unit. DFN3 has no Enable port, so
        // master-off renders it with Attenuation Limit = 0 instead.
        denoiser,
        gate,
        compressor,
        param_eq_node(settings),
        Node::builtin("copy_l", LABEL_COPY),
        Node::builtin("copy_r", LABEL_COPY),
    ]
}

fn param_eq_node(settings: &AppSettings) -> Node {
    let of = &settings.output_filter;
    let eq = &of.equalizer;
    // The `param_eq` node stays in the topology even when EQ is off so
    // the smart-filter sink doesn't drop and Chromium-based browsers
    // don't pause playback. Render flat (0 dB on every band) whenever
    // either the master switch or the EQ sub-toggle is off — the preset
    // must not bleed through in either case.
    let bands: Vec<f32> = if !of.enabled || !eq.enabled {
        vec![0.0; EQ_BAND_COUNT]
    } else if eq.bands.len() == EQ_BAND_COUNT {
        eq.bands.clone()
    } else {
        eq_preset_bands(&eq.preset).map_or_else(|| vec![0.0; EQ_BAND_COUNT], |a| a.to_vec())
    };

    let mut filter_config = String::from("config = {\n    filters = [\n");
    for (i, gain) in bands.iter().enumerate() {
        let freq = EQ_BANDS_HZ[i];
        let _ = writeln!(
            filter_config,
            "        {{ type = bq_peaking freq = {freq} gain = {gain:.2} q = 1.41 }}",
        );
    }
    filter_config.push_str("    ]\n}");

    Node::builtin("eq", LABEL_PARAM_EQ)
        .with_ports("In 1", "Out 1")
        .with_config(filter_config)
}

fn output_links(nodes: &[Node]) -> Vec<Link> {
    // The denoiser node's port names depend on the backend (GTCRN uses
    // "Input"/"Output", DFN3 uses "Audio In"/"Audio Out"). Look the
    // active node up so the rendered links stay in sync with whichever
    // plugin was emitted by `output_nodes`.
    let ai = nodes
        .iter()
        .find(|n| n.name == "ai")
        .expect("output graph always wires the `ai` denoiser node");
    let ai_in = format!("ai:{}", ai.input_port);
    let ai_out = format!("ai:{}", ai.output_port);
    vec![
        Link::new("mixer:Out", "hpf:In"),
        Link::new("hpf:Out", ai_in),
        Link::new(ai_out, "gate:Input"),
        Link::new("gate:Output", "compressor:Input"),
        Link::new("compressor:Output", "eq:In 1"),
        Link::new("eq:Out 1", "copy_l:In"),
        Link::new("eq:Out 1", "copy_r:In"),
    ]
}

fn capture_props(target_sink_name: Option<&str>) -> String {
    // Capture side is the virtual sink apps write into. WirePlumber 0.5
    // ships the `filter.smart` policy: when set, WP transparently links
    // every Stream/Output/Audio that targets the default sink through
    // this node first.
    //
    // The default-sink follow alone broke once `echo-cancel-sink`
    // started showing up in the graph as another Audio/Sink — the
    // policy's "follow default" predicate sometimes picked the AEC
    // reference sink, so streams went straight to AEC bypassing GTCRN.
    // Pinning `filter.smart.target = { node.name = "<hw>" }` removes
    // the ambiguity: the reconciler captures the user's hardware sink
    // before enabling and persists it in `target_sink_name`, so this
    // filter inserts unambiguously between apps and that exact sink.
    // The user's chosen default device stays the visible default in
    // every volume control — only the routing changes.
    // The output filter runs inside `biglinux-microphone-pwloader`,
    // which connects as a regular client of the main PipeWire daemon.
    // The filter graph's nodes are exported to the daemon and driven
    // by the daemon's data-loop — single clock, no cross-process
    // drift. We still pass `node.async = true` as belt-and-braces:
    // it lets PipeWire insert an adaptive resampler if a downstream
    // sink ends up with a slightly different negotiated rate (Bluetooth
    // links and some USB mics renegotiate when codecs switch), without
    // any cost in the common case where rates agree.
    //
    // No explicit `node.latency` / `node.lock-quantum` — pinning a
    // quantum here forces a buffer size the hw sink may not have
    // negotiated. Letting both nodes negotiate freely is the
    // portable answer across diverse hardware. `pause-on-idle =
    // false` is kept so the chain rides brief unlink/relink cycles
    // during a call without the loader exiting on idle and forcing a
    // unit restart.
    let mut props = vec![
        format!("node.name = \"{OUTPUT_NODE_NAME}\""),
        format!("node.description = \"{OUTPUT_DESCRIPTION}\""),
        "media.class = Audio/Sink".to_owned(),
        "node.pause-on-idle = false".to_owned(),
        "node.async = true".to_owned(),
        "audio.rate = 48000".to_owned(),
        "audio.channels = 2".to_owned(),
        "audio.position = [ FL FR ]".to_owned(),
        "filter.smart = true".to_owned(),
        format!("filter.smart.name = \"{OUTPUT_NODE_NAME}\""),
    ];
    if let Some(name) = target_sink_name.and_then(sanitize_node_name) {
        // Disambiguates from `echo-cancel-sink` and any other
        // Audio/Sink in the graph; the policy will not auto-pick the
        // AEC reference sink as the smart-filter destination.
        props.push(format!(
            "filter.smart.target = {{ node.name = \"{name}\" }}"
        ));
    }
    props.join("\n")
}

// PipeWire `node.name` is a dotted/dashed identifier in practice
// (`alsa_output.pci-...`, `bluez_output.XX_XX...`). Reject anything
// that could break the conf parser or smuggle structure into the
// embedding `format!`. `target_sink_name` originates from `settings.json`
// in the user's home, so this is defense-in-depth, not a trust boundary.
fn sanitize_node_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty()
        && name.len() <= 256
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'));
    ok.then_some(name)
}

fn playback_props() -> String {
    // Playback side rides on the smart-filter policy: WirePlumber links
    // it to whichever sink `filter.smart.target` resolves to (the user's
    // hardware sink). `node.passive = true` keeps the chain idle when
    // no app is producing audio so it doesn't hold the hw sink awake.
    // Same rationale as the capture side: no explicit latency / no
    // lock-quantum, plus `node.async = true` as belt-and-braces in
    // case a downstream sink negotiates a slightly different rate
    // (Bluetooth codec switches, USB renegotiation). Cross-process
    // drift is no longer a concern — every filter graph in the
    // package now runs as a client of the main PipeWire daemon, all
    // sharing one clock.
    [
        format!("node.name = \"{OUTPUT_NODE_NAME}-out\""),
        "node.passive = true".to_owned(),
        "node.pause-on-idle = false".to_owned(),
        "node.async = true".to_owned(),
        "audio.rate = 48000".to_owned(),
        "audio.channels = 2".to_owned(),
        "audio.position = [ FL FR ]".to_owned(),
        "stream.dont-remix = true".to_owned(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppSettings;

    fn enabled_settings() -> AppSettings {
        AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
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
    fn conf_pins_smart_filter_target_when_known() {
        // With the user's hw sink captured, the smart-filter target
        // must be pinned to that node.name. This is what disambiguates
        // us from `echo-cancel-sink` and any other Audio/Sink in the
        // graph.
        let s = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                target_sink_name: Some("alsa_output.pci-0000_00_1f.3.analog-stereo".into()),
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        let conf = build_output_conf(&s);
        assert!(
            conf.contains(
                "filter.smart.target = { node.name = \"alsa_output.pci-0000_00_1f.3.analog-stereo\" }"
            ),
            "smart-filter must pin to captured hardware sink to bypass AEC sink ambiguity",
        );
    }

    #[test]
    fn conf_omits_smart_filter_target_when_unknown() {
        // First boot — no target captured yet. Without the pin the
        // policy falls back to the current default sink (still the
        // user's hw sink unless something else takes it over). The
        // conf must remain renderable.
        let conf = build_output_conf(&enabled_settings());
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
    fn conf_does_not_use_optional_zeroramp_builtin() {
        let conf = build_output_conf(&enabled_settings());
        assert!(
            !conf.contains("zeroramp"),
            "output chain must not depend on the optional `zeroramp` builtin",
        );
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
            ..AppSettings::default()
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
            },
            ..AppSettings::default()
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
            ..AppSettings::default()
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
            ..AppSettings::default()
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
            ..AppSettings::default()
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
            ..AppSettings::default()
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

    #[test]
    fn sanitize_rejects_target_with_forbidden_chars() {
        // A target sink name with spaces / structure chars must be rejected so
        // it cannot smuggle SPA-JSON into the embedding format! (defense in
        // depth: the name originates from the user's settings.json).
        let s = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                target_sink_name: Some("bad name; node.name = evil".into()),
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        let conf = build_output_conf(&s);
        assert!(
            !conf.contains("filter.smart.target ="),
            "forbidden chars must reject the smart-filter target: {conf}",
        );
    }

    #[test]
    fn sanitize_rejects_empty_target() {
        let s = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                target_sink_name: Some(String::new()),
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        let conf = build_output_conf(&s);
        assert!(
            !conf.contains("filter.smart.target ="),
            "an empty target name must be rejected, not pinned"
        );
    }
}
