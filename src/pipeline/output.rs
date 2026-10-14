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
//! Because the denoisers are mono, the incoming stereo is first summed via the
//! `mixer` builtin, processed through a single mono chain, and then
//! fanned out with two `copy` nodes to the `FL`/`FR` playback ports:
//!
//! ```text
//! FL,FR → mixer → hpf → ai → gate → compressor → param_eq
//!                                                  ├→ copy_l → FL
//!                                                  └→ copy_r → FR
//! ```
//!
//! The denoiser is **permanently** wired into the output graph while noise
//! reduction is on: master off renders it with `Attenuation Limit = 0`, a live
//! update. Dropping it from the topology would mean rewriting the conf and
//! reloading the unit, which yanks the smart-filter sink out from under any
//! active stream — Chromium-based browsers pause HTMLMediaElement playback the
//! moment their target sink disappears. The standalone SWH gate is independent
//! and also stays always-instantiated — `Output select = 1.0` is a true bypass.

use std::fmt::Write as _;

use crate::config::{AppSettings, EQ_BAND_COUNT, EQ_BANDS_HZ, eq_preset_bands};

use super::graph::{Graph, Link};
use super::nodes::{
    LABEL_BQ_HIGHPASS, LABEL_COPY, LABEL_MIXER, LABEL_PARAM_EQ, LABEL_SC4_MONO, LABEL_SWH_GATE,
    LADSPA_SC4_MONO, LADSPA_SWH_GATE, Node,
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
    if settings.output_filter.channel_mode == crate::config::OutputChannelMode::Stereo {
        return build_stereo_conf(settings);
    }
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

/// Two independent mono processors preserve left/right phase and panning.
/// The economical mono variant below is an explicit user choice, not a hidden
/// side effect of enabling an equalizer or turning neural processing off.
fn build_stereo_conf(settings: &AppSettings) -> String {
    let mono: Vec<Node> = output_nodes(settings)
        .into_iter()
        .filter(|node| !matches!(node.name.as_str(), "mixer" | "copy_l" | "copy_r"))
        .collect();
    let mut nodes = Vec::with_capacity(mono.len() * 2);
    let mut links = Vec::with_capacity(mono.len().saturating_sub(1) * 2);
    let mut inputs = Vec::with_capacity(2);
    let mut outputs = Vec::with_capacity(2);
    for prefix in ["", "right_"] {
        let chain: Vec<Node> = mono
            .iter()
            .cloned()
            .map(|mut node| {
                node.name = format!("{prefix}{}", node.name);
                node
            })
            .collect();
        if let (Some(first), Some(last)) = (chain.first(), chain.last()) {
            inputs.push(format!("{}:{}", first.name, first.input_port));
            outputs.push(format!("{}:{}", last.name, last.output_port));
        }
        for pair in chain.windows(2) {
            links.push(Link::new(
                format!("{}:{}", pair[0].name, pair[0].output_port),
                format!("{}:{}", pair[1].name, pair[1].input_port),
            ));
        }
        nodes.extend(chain);
    }
    Graph {
        description: OUTPUT_DESCRIPTION.into(),
        media_name: OUTPUT_DESCRIPTION.into(),
        nodes,
        links,
        inputs,
        outputs,
        capture_props: capture_props(),
        playback_props: playback_props(),
    }
    .render()
}

/// True when the output denoiser should attenuate. While noise reduction is on
/// the node stays in the topology and master off sets its attenuation to 0, so
/// toggling the master switch is a live update with no service restart and no
/// dropout.
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
    // A model change emits a different plugin, controls and port names; the
    // reconciler treats it as a restart.
    let denoiser = if nr.enabled {
        let atten_db = if ai_processing {
            nr.model.attenuation_db(nr.strength)
        } else {
            0.0
        };
        let (plugin, label) = nr.model.plugin_and_label();
        let (input, output) = nr.model.attenuation_ports();
        Node::ladspa("ai", plugin, label)
            .with_ports(input, output)
            .with_controls(nr.attenuation_controls(atten_db, nr.min_snr_gate_db()))
    } else {
        Node::builtin("ai", LABEL_COPY)
    };

    let gate_d = of.gate.ladspa_controls();
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

    let mut nodes = vec![
        // FL + FR → mono. Both inputs feed channel 1 of the mixer; channel
        // 2 stays at gain 0 so it contributes nothing.
        Node::builtin("mixer", LABEL_MIXER).with_controls([("Gain 1", 0.5), ("Gain 2", 0.5)]),
        Node::builtin("hpf", LABEL_BQ_HIGHPASS).with_controls([("Freq", hpf_freq), ("Q", 0.707)]),
    ];
    // Same pad/makeup pair as the mic chain, for the same reason.
    let padded = super::gain_safety::pads_neural_input(settings, true);
    if padded {
        nodes.push(super::gain_safety::neural_pad());
    }
    nodes.push(denoiser);
    if padded {
        nodes.push(super::gain_safety::neural_makeup());
    }
    nodes.extend([gate, compressor, param_eq_node(settings)]);
    nodes.extend(super::gain_safety::nodes(settings, true));
    nodes.push(Node::builtin("copy_l", LABEL_COPY));
    nodes.push(Node::builtin("copy_r", LABEL_COPY));
    nodes
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
    let linear: Vec<&Node> = nodes
        .iter()
        .filter(|node| !matches!(node.name.as_str(), "mixer" | "copy_l" | "copy_r"))
        .collect();
    let mut links = Vec::with_capacity(linear.len() + 2);
    if let Some(first) = linear.first() {
        links.push(Link::new(
            "mixer:Out",
            format!("{}:{}", first.name, first.input_port),
        ));
    }
    for pair in linear.windows(2) {
        links.push(Link::new(
            format!("{}:{}", pair[0].name, pair[0].output_port),
            format!("{}:{}", pair[1].name, pair[1].input_port),
        ));
    }
    if let Some(last) = linear.last() {
        let out = format!("{}:{}", last.name, last.output_port);
        links.push(Link::new(out.clone(), "copy_l:In"));
        links.push(Link::new(out, "copy_r:In"));
    }
    links
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
    // drift.
    //
    // No `node.async`: async scheduling is *not* the resampler. A
    // downstream sink with a different negotiated rate (Bluetooth, USB)
    // is reconciled by PipeWire's own adapter/resampler regardless of
    // this flag, so async bought nothing for rate safety — but when the
    // wrapped sink is another process's node (e.g. `jamesdsp_sink`), the
    // async producer keeps that consumer's data-loop fd hot every cycle,
    // spinning its RT thread to ~100% of a core with no extra work done.
    // Measured: dropping this flag took the downstream JamesDSP loop from
    // 99% to 10% with identical audio and zero xruns.
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
    // lock-quantum, and no `node.async` — the adapter/resampler, not
    // async, reconciles a downstream sink on a different rate (Bluetooth
    // codec switches, USB renegotiation), while async on this playback
    // node feeding a cross-process sink (`jamesdsp_sink`) spun that
    // consumer's RT loop to ~100%. Cross-process drift is a non-issue:
    // every filter graph in the package runs as a client of the main
    // PipeWire daemon, all sharing one clock.
    [
        format!("node.name = \"{OUTPUT_NODE_NAME}-out\""),
        "node.passive = true".to_owned(),
        "node.pause-on-idle = false".to_owned(),
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
