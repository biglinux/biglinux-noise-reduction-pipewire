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

use crate::config::dynamics::GateDerived;

use crate::config::{
    AppSettings, EQ_BAND_COUNT, EQ_BANDS_HZ, GATE_INTENSITY_MAX, deepfilter_attenuation_db,
    eq_preset_bands, gtcrn_speech_strength,
};

use super::graph::{Graph, Link};
use super::nodes::{
    LABEL_BQ_HIGHPASS, LABEL_COPY, LABEL_GTCRN_MONO, LABEL_MIXER, LABEL_PARAM_EQ, LABEL_SC4_MONO,
    LABEL_SWH_GATE, LADSPA_GTCRN, LADSPA_SC4_MONO, LADSPA_SWH_GATE, Node,
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
        capture_props: capture_props(),
        playback_props: playback_props(),
    };

    graph.render()
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
    // Backend swap (GTCRN ↔ attenuation-only models) is a topology
    // change — selecting one emits a different LADSPA plugin/control surface
    // and port names. The reconciler treats `model` changes as
    // restart-worthy, so live-toggling between the two is intentionally
    // not graceful (one-shot restart on swap).
    let denoiser = if nr.model.is_attenuation_only() {
        // Attenuation-only plugins have no `Enable` port, so master-off
        // / NR-off renders the node with `Attenuation Limit = 0` to make
        // it a passthrough.
        let atten_db = if ai_processing {
            deepfilter_attenuation_db(nr.strength)
        } else {
            0.0
        };
        let (plugin, label) = nr.model.plugin_and_label();
        Node::ladspa("ai", plugin, label)
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

    let gate_d = GateDerived::from_unit_intensity(
        f64::from(of.gate.intensity.min(GATE_INTENSITY_MAX)) / f64::from(GATE_INTENSITY_MAX),
    );
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

    let comp_d = of.compressor.ladspa_controls();
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

fn capture_props() -> String {
    // Capture side is the virtual sink apps write into. WirePlumber 0.5
    // ships the `filter.smart` policy: when set, WP transparently links
    // every Stream/Output/Audio that targets the default sink through
    // this node first.
    //
    // With no explicit `filter.smart.target`, the upstream policy follows
    // the current default sink. Runtime overrides for JamesDSP are owned by
    // the packaged WirePlumber hook.
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
    [
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
    ]
    .join("\n")
}

fn playback_props() -> String {
    // Playback side rides on the smart-filter policy: WirePlumber links
    // it to the current default sink. `node.passive = true` keeps the chain idle when
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
#[path = "output_tests.rs"]
mod tests;
