//! Microphone filter chain generator.
//!
//! Produces a single `.args` file consumed by
//! `biglinux-microphone-pwloader` (started via
//! `biglinux-microphone-mic.service`). The loader is a regular client
//! of the main PipeWire daemon and instantiates
//! `libpipewire-module-filter-chain` with this args body inside its
//! local context. The filter graph's audio nodes are exported to the
//! daemon and driven by the daemon's data-loop — single clock, no
//! cross-process drift. WirePlumber's smart-filter policy then attaches
//! `mic-biglinux` to the user's default audio source. Every application
//! that records from the default microphone reads the processed signal
//! transparently — the physical hardware node stays reachable but
//! deprioritised.
//!
//! Pipeline order, mono:
//!
//! ```text
//! [hpf_pre →] hpf [→ denoiser [→ gate]] [→ compressor] [→ param_eq]
//!     [→ pitch → pitch_gain]   ← only when voice changer is on
//!     → copy_L, copy_R (fan-out for downstream stereo consumers)
//! ```
//!
//! `hpf_pre` is the second stage of the HPF cascade and only exists
//! while the high-pass is on; the standalone `gate` only exists when
//! the selected denoiser has no integrated one. Whichever node comes
//! first is the graph input — see [`build_mic_conf`].
//!
//! Every bracketed node is conditional and skipped from the graph when
//! its flag is off — the mic chain is read by recording apps via the
//! WirePlumber smart-filter policy, so reloading the unit on topology
//! changes does not cause the audible dropouts the output side has to
//! avoid (different contract; see `output.rs`).
//!
//! Conditional nodes:
//!
//! - **`gtcrn`** — neural denoiser; the LADSPA wrapper runs the full
//!   STFT → ONNX inference → iSTFT pipeline every block regardless of
//!   the `Enable` control, and `LookaheadMs` adds latency
//!   unconditionally. The integrated gate shares the GTCRN node, so
//!   the node is kept in the chain whenever **either** noise reduction
//!   or the gate is on.
//! - **`compressor`** (`sc4m_1916`) — runs RMS detection + envelope per
//!   block even at unity ratio. Skipped entirely when disabled.
//! - **`param_eq`** — ten cascaded `bq_peaking` biquads run every
//!   block regardless of gain. Skipped entirely when disabled.
//! - **`pitch_scale_1193`** + **`pitch_gain`** — phase-vocoder; only
//!   present when voice changer is on. STFT/iSTFT smears transients
//!   even at unity coefficient.
//!
//! Toggling any of the topology flags reloads the filter-chain (same
//! tier as voice-changer topology changes); slider drags inside an
//! enabled sub-effect still go through the live-controls fast path.

use std::fmt::Write as _;

use crate::config::{
    AppSettings, EQ_BAND_COUNT, EQ_BANDS_HZ, EchoMode, StereoMode, deepfilter_attenuation_db,
    eq_preset_bands, gtcrn_speech_strength,
};

use super::graph::{Graph, Link};
use super::nodes::{
    LABEL_AMP, LABEL_BQ_HIGHPASS, LABEL_COPY, LABEL_GTCRN_MONO, LABEL_PARAM_EQ, LABEL_PITCH_SCALE,
    LABEL_SC4_MONO, LABEL_SWH_GATE, LADSPA_AMP, LADSPA_GTCRN, LADSPA_PITCH_SCALE, LADSPA_SC4_MONO,
    LADSPA_SWH_GATE, Node,
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
pub const MIC_DESCRIPTION: &str = "Filter noise";
/// File name of the mic args body inside the bigmic state dir.
pub const MIC_CONF_FILE: &str = "mic.args";

/// Render the complete mic filter-chain config file text.
#[must_use]
pub fn build_mic_conf(settings: &AppSettings) -> String {
    let nodes = mic_nodes(settings);
    let links = mic_links(&nodes);

    // The graph input has to be the head of the linear chain, which is
    // `hpf_pre` once the HPF cascade is on. Naming a port that a link
    // already consumes makes `spa.filter-graph` refuse the whole graph
    // ("input port hpf[0]:In already used by link, use mixer", -EBUSY),
    // so the virtual source never starts.
    let head = nodes
        .first()
        .expect("mic graph always starts with an hpf node");
    let inputs = vec![format!("{}:{}", head.name, head.input_port)];

    let graph = Graph {
        description: MIC_DESCRIPTION.into(),
        media_name: MIC_DESCRIPTION.into(),
        nodes,
        links,
        inputs,
        outputs: vec!["copy_l:Out".into(), "copy_r:Out".into()],
        capture_props: capture_props(settings),
        playback_props: playback_props(),
    };

    graph.render()
}

/// Tear down every mic-side flag in one go. Called by the simple-view
/// master switch and the Plasma applet `toggle-mic` action so a single
/// "off" click reaches all reasons the mic loader would stay alive —
/// default-on flags (`echo_cancel`, `stereo`) would otherwise keep it
/// running silently. Advanced view leaves the flags independent and
/// does not call this.
pub fn cascade_mic_off(settings: &mut AppSettings) {
    settings.noise_reduction.enabled = false;
    // `echo_cancel.enabled` is derived from the mode and the current output
    // route, and `services::echo::settle` re-derives it before the graph is
    // written — so clearing only the flag was a write nothing read. The mode
    // is the intent, and `mic_chain_wanted` counts the flag, so without this
    // one "off" click left the virtual source and the AEC loader up on every
    // machine with speakers. The mode row in the mic view is how it comes back.
    settings.echo_cancel.mode = EchoMode::Never;
    settings.echo_cancel.enabled = false;
    settings.gate.enabled = false;
    settings.hpf.enabled = false;
    settings.stereo.enabled = false;
    settings.equalizer.enabled = false;
    settings.compressor.enabled = false;
}

/// Does the current settings snapshot ask for *any* microphone
/// processing? When this returns `false` we skip writing the mic chain
/// config entirely so the user doesn't see a "Filter noise"
/// virtual source while every filter is off.
#[must_use]
pub fn mic_chain_wanted(settings: &AppSettings) -> bool {
    // EC alone is enough to keep the chain materialised: without the
    // smart filter we'd produce an `echo-cancel-source` that no app
    // would pick up, since recording apps target the default source.
    !settings.mic_bypass
        && (settings.noise_reduction.enabled
            || settings.gate.enabled
            || settings.hpf.enabled
            || (settings.stereo.enabled && settings.stereo.mode == StereoMode::VoiceChanger)
            || settings.equalizer.enabled
            || settings.compressor.enabled
            || settings.echo_cancel.enabled)
}

/// True when the GTCRN node should be present in the mic graph.
///
/// The integrated gate shares the GTCRN LADSPA instance, so we keep
/// the node alive when **either** noise reduction or the silence gate
/// is wanted. Skipping it otherwise drops the STFT/iSTFT and ONNX
/// pipeline entirely — both expensive and a transient-smearing source.
#[must_use]
pub fn ai_node_in_mic_chain(settings: &AppSettings) -> bool {
    !settings.mic_bypass
        && (settings.noise_reduction.enabled
            || (settings.gate.enabled && !settings.noise_reduction.model.is_attenuation_only()))
}

fn mic_nodes(settings: &AppSettings) -> Vec<Node> {
    // When enabled we cascade two identical Butterworth biquads
    // (Linkwitz-Riley 4th order, 24 dB/oct) so the rumble rolloff is
    // steep enough to matter on laptop mics. When disabled a single
    // node at a vanishingly low cutoff stays in place to avoid graph
    // restarts and keep the downstream link target ("hpf:Out") stable
    // for tests and live-update code paths.
    let mut nodes: Vec<Node> = if settings.hpf.enabled {
        let f = f64::from(settings.hpf.frequency);
        vec![
            Node::builtin("hpf_pre", LABEL_BQ_HIGHPASS).with_controls([("Freq", f), ("Q", 0.707)]),
            Node::builtin("hpf", LABEL_BQ_HIGHPASS).with_controls([("Freq", f), ("Q", 0.707)]),
        ]
    } else {
        vec![Node::builtin("hpf", LABEL_BQ_HIGHPASS).with_controls([("Freq", 5.0), ("Q", 0.707)])]
    };

    if ai_node_in_mic_chain(settings) {
        // The pad protects the network from clipping inside its own inference;
        // the makeup returns the level before anything calibrated in dB.
        let padded = super::gain_safety::pads_neural_input(settings, false);
        if padded {
            nodes.push(super::gain_safety::neural_pad());
        }
        nodes.push(denoiser_node(settings));
        if padded {
            nodes.push(super::gain_safety::neural_makeup());
        }
    }
    // Attenuation-only backends have an independent gate. Keeping the gate
    // enabled must not keep the neural network running after NR is disabled.
    if settings.noise_reduction.model.is_attenuation_only() && settings.gate.enabled {
        nodes.push(standalone_gate_node(settings));
    }

    if settings.compressor.enabled {
        nodes.push(compressor_node(settings));
    }

    if settings.equalizer.enabled {
        nodes.push(param_eq_node(settings));
    }

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

    nodes.extend(super::gain_safety::nodes(settings, false));
    nodes.push(Node::builtin("copy_l", LABEL_COPY));
    nodes.push(Node::builtin("copy_r", LABEL_COPY));
    nodes
}

fn compressor_node(settings: &AppSettings) -> Node {
    let comp_d = settings.compressor.ladspa_controls();
    Node::ladspa("compressor", LADSPA_SC4_MONO, LABEL_SC4_MONO).with_controls([
        ("RMS/peak", f64::from(comp_d.rms_peak)),
        ("Attack time (ms)", f64::from(comp_d.attack_ms)),
        ("Release time (ms)", f64::from(comp_d.release_ms)),
        ("Threshold level (dB)", f64::from(comp_d.threshold_db)),
        ("Ratio (1:n)", f64::from(comp_d.ratio)),
        ("Knee radius (dB)", f64::from(comp_d.knee_db)),
        ("Makeup gain (dB)", f64::from(comp_d.makeup_gain_db)),
    ])
}

fn denoiser_node(settings: &AppSettings) -> Node {
    if settings.noise_reduction.model.is_attenuation_only() {
        attenuation_denoiser_node(settings)
    } else {
        gtcrn_node(settings)
    }
}

fn gtcrn_node(settings: &AppSettings) -> Node {
    let nr = &settings.noise_reduction;
    let gate = &settings.gate;
    let gate_derived = gate.ladspa_controls();
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
        ("SpeechStrength", gtcrn_speech_strength(nr.strength)),
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

/// Attenuation-only denoiser node (DeepFilterNet / DPDFNet families).
/// These plugins have no `Enable` port — toggling noise-reduction off
/// while one is selected drops the node from the graph (a reload, not
/// a live update). The strength→cap mapping lives in
/// [`deepfilter_attenuation_db`] (quadratic curve aligned with the
/// upstream cap-as-perceptual-knob guidance).
fn attenuation_denoiser_node(settings: &AppSettings) -> Node {
    let nr = &settings.noise_reduction;
    let atten_db = deepfilter_attenuation_db(nr.strength);
    let (plugin, label) = nr.model.plugin_and_label();
    Node::ladspa("ai", plugin, label)
        .with_ports("Audio In", "Audio Out")
        .with_controls([("Attenuation Limit (dB)", atten_db)])
}

fn standalone_gate_node(settings: &AppSettings) -> Node {
    let gate_d = settings.gate.ladspa_controls();
    Node::ladspa("gate", LADSPA_SWH_GATE, LABEL_SWH_GATE).with_controls([
        ("Threshold (dB)", gate_d.threshold_db),
        ("Attack (ms)", gate_d.attack_ms),
        ("Hold (ms)", gate_d.hold_ms),
        ("Decay (ms)", gate_d.release_ms),
        ("Range (dB)", gate_d.range_db),
        ("LF key filter (Hz)", 200.0),
        ("HF key filter (Hz)", 6000.0),
        ("Output select (-1 = key listen, 0 = gate, 1 = bypass)", 0.0),
    ])
}

/// Pitch shifter coefficient + gain-compensation amplifier value when
/// the voice changer is engaged. `None` skips both nodes entirely so
/// the audio path stays free of the phase-vocoder STFT/iSTFT pass.
///
/// `width` is exponential: width=0.0 → 0.5x (deep), width=0.5 → 1.0x
/// (passthrough), width=1.0 → 2.0x (high). The calibrated gain curve gives
/// deep voices +dB to keep loudness and high voices a small attenuation to
/// avoid clipping.
fn pitch_controls(settings: &AppSettings) -> Option<(f64, f64)> {
    let st = &settings.stereo;
    if !st.enabled || st.mode != StereoMode::VoiceChanger {
        return None;
    }
    let width = f64::from(st.width).clamp(0.0, 1.0);
    let coeff = if width == 0.0 {
        0.5
    } else if width == 0.5 {
        1.0
    } else if width == 1.0 {
        2.0
    } else {
        (0.5 * 4.0_f64.powf(width)).clamp(0.5, 2.0)
    };
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
/// Only invoked when the equalizer is enabled — when off, the node is
/// dropped from the graph entirely so its 10 cascaded biquads stop
/// running.
fn param_eq_node(settings: &AppSettings) -> Node {
    let eq = &settings.equalizer;
    let bands: Vec<f32> = if eq.bands.len() == EQ_BAND_COUNT {
        eq.bands.clone()
    } else {
        resolve_preset_or_flat(&eq.preset)
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

fn resolve_preset_or_flat(preset: &str) -> Vec<f32> {
    eq_preset_bands(preset).map_or_else(|| vec![0.0; EQ_BAND_COUNT], |a| a.to_vec())
}

fn mic_links(nodes: &[Node]) -> Vec<Link> {
    // Walk the linear part of the chain (every node except the stereo
    // fan-out copies) and emit a pairwise link using each node's
    // declared `input_port` / `output_port`. PipeWire fans one output
    // to many inputs, so the last linear node feeds both copies
    // directly.
    let linear: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.name != "copy_l" && n.name != "copy_r")
        .collect();

    let mut links = Vec::with_capacity(linear.len() + 1);
    for pair in linear.windows(2) {
        let from = pair[0];
        let to = pair[1];
        links.push(Link::new(
            format!("{}:{}", from.name, from.output_port),
            format!("{}:{}", to.name, to.input_port),
        ));
    }
    if let Some(last) = linear.last() {
        let out = format!("{}:{}", last.name, last.output_port);
        links.push(Link::new(out.clone(), "copy_l:In"));
        links.push(Link::new(out, "copy_r:In"));
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
    // `node.lock-quantum = true` keeps PipeWire from re-negotiating the
    // graph quantum while this chain is active. Without the lock, the
    // graph re-negotiates every time an app connects/disconnects (call
    // clients hot-plug capture streams), and the resulting buffer
    // re-allocations on a USB driver under load show up as the
    // crackle-correlated-with-remote-talk symptom. Per pipewire-props(7)
    // the lock auto-releases once this node deactivates, so devices
    // unrelated to this chain stay on their negotiated defaults. The
    // existing `node.pause-on-idle = false` is enough to ride out
    // brief unlink/relink cycles during a call without falling back
    // to `node.always-process` (which would burn CPU running GTCRN
    // inference 24/7 even when no app is capturing).
    let mut props = vec![
        "node.name = \"mic-biglinux-capture\"".to_owned(),
        "node.passive = true".to_owned(),
        format!(
            "node.latency = \"{}\"",
            super::echo_cancel::AEC_NODE_LATENCY
        ),
        "node.pause-on-idle = false".to_owned(),
        "node.lock-quantum = true".to_owned(),
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

fn playback_props() -> String {
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
    let props = vec![
        format!("node.name = \"{MIC_NODE_NAME}\""),
        format!("node.description = \"{MIC_DESCRIPTION}\""),
        "media.class = Audio/Source".to_owned(),
        format!(
            "node.latency = \"{}\"",
            super::echo_cancel::AEC_NODE_LATENCY
        ),
        "node.pause-on-idle = false".to_owned(),
        "node.lock-quantum = true".to_owned(),
        "audio.rate = 48000".to_owned(),
        "audio.position = [ FL FR ]".to_owned(),
        "filter.smart = true".to_owned(),
        "filter.smart.name = \"big.filter-microphone\"".to_owned(),
    ];
    props.join("\n")
}

#[cfg(test)]
mod tests;
