//! Microphone filter chain generator.
//!
//! Produces a single `.conf` file dropped into
//! `~/.config/pipewire/filter-chain.conf.d/` that WirePlumber's smart-filter
//! policy attaches to the user's default audio source. Every application
//! that records from the default microphone reads the processed signal
//! transparently — the original hardware node stays reachable but
//! deprioritised.
//!
//! Pipeline order, mono:
//!
//! ```text
//! hpf [→ gtcrn(+integrated gate)] → compressor → param_eq
//!     [→ pitch → pitch_gain]   ← only when voice changer is on
//!     → copy_L, copy_R (fan-out for downstream stereo consumers)
//! ```
//!
//! Two nodes are conditional rather than always-instantiated:
//!
//! - **`pitch_scale_1193`** — phase-vocoder; STFT/iSTFT smears
//!   transients even at unity coefficient (1.0×).
//! - **`gtcrn`** — neural denoiser; the LADSPA wrapper runs the full
//!   STFT → ONNX inference → iSTFT pipeline every block regardless of
//!   the `Enable` control, and `LookaheadMs` adds latency
//!   unconditionally. Same transient-smearing class as the pitch node.
//!   The integrated gate shares the GTCRN node, so the node is kept in
//!   the chain whenever **either** noise reduction or the gate is on.
//!
//! Toggling either condition reloads the filter-chain (same tier as
//! EQ/voice-changer topology changes); slider drags inside an enabled
//! sub-effect still go through the live-controls fast path.

use std::fmt::Write as _;

use crate::config::{
    eq_preset_bands, AppSettings, CompressorDerived, StereoMode, EQ_BANDS_HZ, EQ_BAND_COUNT,
};

use super::graph::{Graph, Link, RenderMode};
use super::nodes::{
    Node, LABEL_AMP, LABEL_BQ_HIGHPASS, LABEL_COPY, LABEL_GTCRN_MONO, LABEL_PARAM_EQ,
    LABEL_PITCH_SCALE, LABEL_SC4_MONO, LADSPA_AMP, LADSPA_GTCRN, LADSPA_PITCH_SCALE,
    LADSPA_SC4_MONO,
};

/// Stem used for the smart filter name — the outward-facing
/// `Audio/Source` node that recording apps see.
pub const MIC_NODE_NAME: &str = "mic-biglinux";
/// The internal capture-side node name. This is where PipeWire
/// exposes the LADSPA/builtin **filter-chain controls** (`ai:*`,
/// `hpf:*`, `compressor:*`). The playback-side node only carries the
/// generic audio-adapter properties, so any live parameter update has
/// to target *this* name instead.
pub const MIC_CAPTURE_NODE_NAME: &str = "mic-biglinux-capture";
pub const MIC_DESCRIPTION: &str = "BigLinux Microphone";
/// File name inside `filter-chain.conf.d/`.
pub const MIC_CONF_FILE: &str = "10-biglinux-microphone.conf";

/// Render the complete mic filter-chain config file text.
#[must_use]
pub fn build_mic_conf(settings: &AppSettings) -> String {
    let nodes = mic_nodes(settings);
    let links = mic_links(&nodes);

    let graph = Graph {
        description: MIC_DESCRIPTION.into(),
        media_name: MIC_DESCRIPTION.into(),
        nodes,
        links,
        inputs: vec!["hpf:In".into()],
        outputs: vec!["copy_l:Out".into(), "copy_r:Out".into()],
        capture_props: capture_props(settings),
        playback_props: playback_props(settings),
    };

    graph.render(RenderMode::DropIn)
}

/// Tear down every mic-side flag in one go. Called by the simple-view
/// master switch and the Plasma applet `toggle-mic` action so a single
/// "off" click reaches all reasons `filter-chain.service` would stay
/// alive — default-on flags (`echo_cancel`, `stereo`) would otherwise
/// keep the worker running silently. Advanced view leaves the flags
/// independent and does not call this.
pub fn cascade_mic_off(settings: &mut AppSettings) {
    settings.noise_reduction.enabled = false;
    settings.echo_cancel.enabled = false;
    settings.gate.enabled = false;
    settings.hpf.enabled = false;
    settings.stereo.enabled = false;
    settings.equalizer.enabled = false;
    settings.compressor.enabled = false;
}

/// Does the current settings snapshot ask for *any* microphone
/// processing? When this returns `false` we skip writing the mic chain
/// config entirely so the user doesn't see a "BigLinux Microphone"
/// virtual source while every filter is off.
#[must_use]
pub fn mic_chain_wanted(settings: &AppSettings) -> bool {
    // EC alone is enough to keep the chain materialised: without the
    // smart filter we'd produce an `echo-cancel-source` that no app
    // would pick up, since recording apps target the default source.
    settings.noise_reduction.enabled
        || settings.gate.enabled
        || settings.hpf.enabled
        || settings.stereo.enabled
        || settings.equalizer.enabled
        || settings.compressor.enabled
        || settings.echo_cancel.enabled
}

/// True when the GTCRN node should be present in the mic graph.
///
/// The integrated gate shares the GTCRN LADSPA instance, so we keep
/// the node alive when **either** noise reduction or the silence gate
/// is wanted. Skipping it otherwise drops the STFT/iSTFT and ONNX
/// pipeline entirely — both expensive and a transient-smearing source.
#[must_use]
pub fn ai_node_in_mic_chain(settings: &AppSettings) -> bool {
    settings.noise_reduction.enabled || settings.gate.enabled
}

fn mic_nodes(settings: &AppSettings) -> Vec<Node> {
    let hpf_freq = if settings.hpf.enabled {
        f64::from(settings.hpf.frequency)
    } else {
        // A vanishingly low cutoff makes the biquad pass-through without the
        // cost (or graph restart) of removing the node.
        5.0
    };

    let comp_d = CompressorDerived::from_config(&settings.compressor);
    let comp_enabled = settings.compressor.enabled;
    let compressor = Node::ladspa("compressor", LADSPA_SC4_MONO, LABEL_SC4_MONO).with_controls([
        ("RMS/peak", f64::from(comp_d.rms_peak)),
        ("Attack time (ms)", f64::from(comp_d.attack_ms)),
        ("Release time (ms)", f64::from(comp_d.release_ms)),
        (
            "Threshold level (dB)",
            // Neutral pass-through threshold keeps graph structurally stable
            // when the user toggles the compressor off.
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

    let mut nodes =
        vec![Node::builtin("hpf", LABEL_BQ_HIGHPASS)
            .with_controls([("Freq", hpf_freq), ("Q", 0.707)])];

    if ai_node_in_mic_chain(settings) {
        nodes.push(gtcrn_node(settings));
    }

    nodes.push(compressor);
    nodes.push(param_eq_node(settings));

    if let Some((coeff, gain_db)) = pitch_controls(settings) {
        nodes.push(
            Node::ladspa("pitch", LADSPA_PITCH_SCALE, LABEL_PITCH_SCALE)
                .with_controls([("Pitch co-efficient", coeff)]),
        );
        nodes.push(
            Node::ladspa("pitch_gain", LADSPA_AMP, LABEL_AMP)
                .with_controls([("Amps gain (dB)", gain_db)]),
        );
    }

    nodes.push(Node::builtin("copy_l", LABEL_COPY));
    nodes.push(Node::builtin("copy_r", LABEL_COPY));
    nodes
}

fn gtcrn_node(settings: &AppSettings) -> Node {
    let nr = &settings.noise_reduction;
    let gate = &settings.gate;
    let gate_derived = crate::config::GateDerived::from_config(gate);
    let threshold_db = if gate.enabled {
        gate_derived.threshold_db
    } else {
        // Threshold below the noise floor disables the integrated gate
        // while the GTCRN inference itself stays active for noise
        // reduction.
        -80.0
    };
    Node::ladspa("ai", LADSPA_GTCRN, LABEL_GTCRN_MONO).with_controls([
        ("Enable", if nr.enabled { 1.0 } else { 0.0 }),
        ("Strength", f64::from(nr.strength)),
        ("Model", f64::from(nr.model.ladspa_control())),
        ("SpeechStrength", f64::from(nr.strength)),
        ("LookaheadMs", f64::from(nr.lookahead_ms)),
        ("ModelBlend", f64::from(nr.model_blending)),
        ("VoiceRecovery", f64::from(nr.voice_recovery)),
        ("Threshold (dB)", threshold_db),
        ("Attack (ms)", gate_derived.attack_ms),
        ("Hold (ms)", gate_derived.hold_ms),
        ("Release (ms)", gate_derived.release_ms),
        ("Range (dB)", gate_derived.range_db),
        ("LF Key Filter (Hz)", 200.0),
        ("HF Key Filter (Hz)", 5000.0),
    ])
}

/// Pitch shifter coefficient + gain-compensation amplifier value when
/// the voice changer is engaged. `None` skips both nodes entirely so
/// the audio path stays free of the phase-vocoder STFT/iSTFT pass.
///
/// `width` is exponential: width=0.0 → 0.5x (deep), width=0.5 → 1.0x
/// (passthrough), width=1.0 → 2.0x (high). Gain compensation matches
/// the legacy curve — deep voices need +dB to keep loudness; high
/// voices need a small attenuation to avoid clipping.
fn pitch_controls(settings: &AppSettings) -> Option<(f64, f64)> {
    let st = &settings.stereo;
    if !st.enabled || st.mode != StereoMode::VoiceChanger {
        return None;
    }
    let width = f64::from(st.width).clamp(0.0, 1.0);
    let coeff = (0.5 * 4.0_f64.powf(width)).clamp(0.5, 2.0);
    let gain_db = if coeff < 1.0 {
        (1.0 - coeff) * 20.0
    } else if coeff > 1.0 {
        -(coeff - 1.0) * 3.0
    } else {
        0.0
    };
    Some((coeff, gain_db))
}

/// Build the `param_eq` node with one `bq_peaking` filter per UI band.
/// The band list stays stable regardless of whether the equalizer is
/// enabled; disabling the EQ just zeroes every gain.
fn param_eq_node(settings: &AppSettings) -> Node {
    let eq = &settings.equalizer;
    let bands: Vec<f32> = if eq.enabled && eq.bands.len() == EQ_BAND_COUNT {
        eq.bands.clone()
    } else {
        resolve_preset_or_flat(&eq.preset)
    };

    let mut cfg = String::from("config = {\n    filters = [\n");
    for (i, gain) in bands.iter().enumerate() {
        let freq = EQ_BANDS_HZ[i];
        let _ = writeln!(
            cfg,
            "        {{ type = bq_peaking freq = {freq} gain = {gain:.2} q = 1.41 }}",
        );
    }
    cfg.push_str("    ]\n}");

    Node::builtin("eq", LABEL_PARAM_EQ)
        .with_ports("In 1", "Out 1")
        .with_config(cfg)
}

fn resolve_preset_or_flat(preset: &str) -> Vec<f32> {
    eq_preset_bands(preset).map_or_else(|| vec![0.0; EQ_BAND_COUNT], |a| a.to_vec())
}

fn mic_links(nodes: &[Node]) -> Vec<Link> {
    // Wiring is rebuilt from scratch so the structure mirrors the
    // conditional nodes above. PipeWire fans one output out to several
    // inputs, so the last mono node feeds both copies directly without
    // an intermediate splitter.
    let ai_in_chain = nodes.iter().any(|n| n.name == "ai");
    let pitch_in_chain = nodes.iter().any(|n| n.name == "pitch");

    let mut links = Vec::with_capacity(8);
    if ai_in_chain {
        links.push(Link::new("hpf:Out", "ai:Input"));
        links.push(Link::new("ai:Output", "compressor:Input"));
    } else {
        links.push(Link::new("hpf:Out", "compressor:Input"));
    }
    links.push(Link::new("compressor:Output", "eq:In 1"));
    if pitch_in_chain {
        links.push(Link::new("eq:Out 1", "pitch:Input"));
        links.push(Link::new("pitch:Output", "pitch_gain:Input"));
        links.push(Link::new("pitch_gain:Output", "copy_l:In"));
        links.push(Link::new("pitch_gain:Output", "copy_r:In"));
    } else {
        links.push(Link::new("eq:Out 1", "copy_l:In"));
        links.push(Link::new("eq:Out 1", "copy_r:In"));
    }
    links
}

fn capture_props(settings: &AppSettings) -> String {
    // Capture side pulls audio into the graph. Without AEC we leave
    // `target.object` unset so WirePlumber's smart-filter policy anchors
    // `mic-biglinux` to the user's default hardware source. With AEC on,
    // the upstream must be explicit: `echo-cancel-source` is the cleaned
    // mic signal, and leaving this unset lets WirePlumber link the mic
    // chain directly to the hardware source, bypassing AEC.
    //
    // `node.passive = true` keeps PipeWire from spinning the filter
    // when no consumer is reading. Capture is mono: the chain downmixes
    // internally; stereo fan-out is on the playback side.
    //
    // The node name must stay in sync with [`MIC_CAPTURE_NODE_NAME`] —
    // live parameter updates target this name, not the outward-facing
    // `Audio/Source` wrapper.
    let latency = if settings.echo_cancel.enabled {
        super::echo_cancel::AEC_NODE_LATENCY
    } else {
        "1024/48000"
    };

    let mut props = vec![
        "node.name = \"mic-biglinux-capture\"".to_owned(),
        "node.passive = true".to_owned(),
        format!("node.latency = \"{latency}\""),
        "node.pause-on-idle = false".to_owned(),
        "audio.rate = 48000".to_owned(),
        "audio.position = [ MONO ]".to_owned(),
    ];
    if settings.echo_cancel.enabled {
        props.push(format!(
            "target.object = \"{}\"",
            super::echo_cancel::EC_SOURCE_NAME
        ));
    }
    props.join("\n")
}

fn playback_props(settings: &AppSettings) -> String {
    // Always declare `mic-biglinux` as a WirePlumber smart filter so it
    // inserts itself between every default-following recording app and
    // whichever source the user has picked in their audio manager. The
    // visible default stays the hw mic — KDE/pavucontrol show the
    // user's real device — but the audio apps read is filtered.
    //
    // With AEC enabled, the capture side is pinned to
    // `echo-cancel-source`. The EC source is deliberately not a smart
    // filter; WirePlumber only sorts `mic-biglinux`, and the explicit
    // capture target gives the stable cascade:
    //
    // ```text
    // app ← mic-biglinux (big.filter-microphone)
    //         ← echo-cancel-source
    //             ← user's selected hw mic
    // ```
    //
    // No `filter.smart.target` is set on playback: apps should keep
    // following the user-visible default source, with `mic-biglinux`
    // inserted transparently.
    //
    // Stereo fan-out (FL / FR via `copy_l` / `copy_r`) keeps apps that
    // require a two-channel source happy; the internal chain is mono
    // and the copies just duplicate the signal.
    let latency = if settings.echo_cancel.enabled {
        super::echo_cancel::AEC_NODE_LATENCY
    } else {
        "1024/48000"
    };

    let props = vec![
        format!("node.name = \"{MIC_NODE_NAME}\""),
        format!("node.description = \"{MIC_DESCRIPTION}\""),
        "media.class = Audio/Source".to_owned(),
        format!("node.latency = \"{latency}\""),
        "node.pause-on-idle = false".to_owned(),
        "audio.rate = 48000".to_owned(),
        "audio.position = [ FL FR ]".to_owned(),
        "filter.smart = true".to_owned(),
        "filter.smart.name = \"big.filter-microphone\"".to_owned(),
    ];
    props.join("\n")
}

#[cfg(test)]
mod tests {
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
        // off leaves filter-chain.service running on the surviving
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
    fn conf_starts_with_comment_and_module_block() {
        let conf = build_mic_conf(&default_settings());
        assert!(conf.starts_with("# BigLinux"));
        assert!(conf.contains("libpipewire-module-filter-chain"));
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
            conf.contains("node.latency = \"960/48000\""),
            "when AEC is upstream, the mic chain must not force WebRTC back to a 1024-frame quantum",
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
        let conf = build_mic_conf(&s);
        assert!(!conf.contains("plugin = \"/usr/lib/ladspa/libgtcrn_ladspa.so\""));
        assert!(!conf.contains("name = \"ai\""));
        // Chain must skip the node and link hpf straight into the
        // compressor — otherwise the graph would have a dangling edge.
        assert!(conf.contains("{ output = \"hpf:Out\" input = \"compressor:Input\" }"));
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
    fn conf_disabled_compressor_has_unity_ratio_zero_makeup() {
        let mut s = default_settings();
        s.compressor.enabled = false;
        let conf = build_mic_conf(&s);
        assert!(conf.contains("\"Ratio (1:n)\" = 1.0"));
        assert!(conf.contains("\"Makeup gain (dB)\" = 0.0"));
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
        let conf = build_mic_conf(&default_settings());
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
        let conf = build_mic_conf(&default_settings());
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
}
