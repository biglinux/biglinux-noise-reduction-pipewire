//! Live control updates on running filter-chain nodes.
//!
//! The filter-chain module exposes every LADSPA / builtin control as an
//! SPA property under the node's `Props` parameter, keyed by the control
//! name we declared in the `.conf` file (`"Strength"`, `"Enable"`,
//! `"Freq"`, …). `pw-cli s <id> Props '{ params = [ … ] }'` pushes a new
//! value without touching the module graph, so slider drags update the
//! running audio pipeline in real time — no loader restart, no pop, no
//! drop-out.
//!
//! The helper intentionally stays argv-based: it invokes `pw-cli`
//! through the shared subprocess boundary with an argument array, never
//! through a shell (`sh -c`), so there is no shell-quoting or injection
//! surface. Rationale:
//!
//! - Transparent to debug (`strace` the spawned process).
//! - Uses the same parser PipeWire itself ships, so property-name typos
//!   surface with a clear error message instead of silently corrupt POD.
//! - Zero-cost to replace later with a native `libspa` POD builder when
//!   we decide the subprocess overhead matters.
//!
//! Gracefully no-ops when the target node is absent (e.g. the first run
//! before the loader units have started) or still `suspended`, which is
//! where PipeWire drops Props silently — see [`find_live_node`]. Both
//! cases report "not pushed" so the caller restarts the loader instead.
//! A loader restart is also required whenever the **graph structure**
//! changes — adding/removing a filter, swapping the model — because
//! only the control values can be updated live.

use std::collections::HashMap;
use std::io;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use log::debug;
use serde_json::Value;

use crate::config::{AppSettings, deepfilter_attenuation_db, gtcrn_speech_strength};
use crate::pipeline::{
    MIC_CAPTURE_NODE_NAME, OUTPUT_NODE_NAME, ai_node_in_mic_chain, mic_chain_wanted,
    output_ai_processing,
};

/// Result of a [`apply_live`] call.
///
/// `*_pushed` is `true` when the corresponding filter-chain node was
/// live in the PipeWire graph and its controls were updated. A `false`
/// tells the caller the running graph is stale — the loader is not up
/// yet, or its node is still `suspended` and would swallow the update.
/// The caller should then restart that loader so it picks up the args
/// file the reconciler just wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveOutcome {
    pub mic_pushed: bool,
    pub output_pushed: bool,
}

impl LiveOutcome {
    /// `true` when every chain the user currently wants running has
    /// received its control update.
    #[must_use]
    pub fn fully_applied(self, settings: &AppSettings) -> bool {
        (!mic_chain_wanted(settings) || self.mic_pushed)
            && (!settings.output_filter.enabled || self.output_pushed)
    }
}

/// Select only chains whose effective parameters changed.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct UpdateTargets {
    pub microphone: bool,
    pub output: bool,
}

/// A failed fast path belongs to one chain, not to the whole transaction.
#[derive(Debug, Default)]
pub(crate) struct LiveReport {
    pub pushed: LiveOutcome,
    pub failures: Vec<String>,
}

pub fn apply_live(settings: &AppSettings) -> io::Result<LiveOutcome> {
    let effective = settings.runtime_settings();
    let graph = graph_snapshot()?;
    let targets = UpdateTargets {
        microphone: mic_chain_wanted(&effective),
        output: effective.output_filter.enabled,
    };
    let report = apply_live_to_graph(&effective, &graph, targets);
    if report.failures.is_empty() {
        Ok(report.pushed)
    } else {
        Err(io::Error::other(report.failures.join("; ")))
    }
}

/// The reconciler reuses its graph snapshot and may repair failed pushes.
pub(crate) fn apply_live_to_graph(
    settings: &AppSettings,
    graph: &[Value],
    targets: UpdateTargets,
) -> LiveReport {
    let nodes = parse_live_filter_nodes(graph);
    let mut report = LiveReport::default();
    if targets.microphone {
        report.pushed.mic_pushed = push_one(
            nodes.get(MIC_CAPTURE_NODE_NAME).copied(),
            || mic_params(settings),
            "microphone",
            &mut report.failures,
        );
    }
    if targets.output {
        report.pushed.output_pushed = push_one(
            nodes.get(OUTPUT_NODE_NAME).copied(),
            || output_params(settings),
            "output",
            &mut report.failures,
        );
    }
    report
}

fn push_one(
    id: Option<u32>,
    controls: impl FnOnce() -> Vec<(String, f64)>,
    chain: &str,
    failures: &mut Vec<String>,
) -> bool {
    let Some(id) = id else {
        return false;
    };
    match set_props(id, &controls()) {
        Ok(()) => true,
        Err(error) => {
            failures.push(format!("{chain} live update: {error}"));
            false
        }
    }
}

/// `node.name` → object id for every node whose filter graph is set up.
///
/// One `pw-dump` for the whole question. Resolving a name and then its state
/// through `pw-cli` cost four spawns per apply — `ls Node` plus `info <id>`,
/// once per chain — and `ls Node` renders the entire graph as text anyway,
/// which the module below warns can exceed 64 KiB on a busy graph.
///
/// Nodes still `suspended` are deliberately absent: their filter graph is
/// only set up at format negotiation, and until then PipeWire accepts the
/// `set-param` and drops it — `pw-cli` still exits 0 and the control keeps
/// its load-time value. Leaving them out makes the caller fall back to a
/// loader restart, which re-reads the args file the reconciler just wrote.
/// Verified on PipeWire 1.6.8: a `Props` push to a suspended node leaves the
/// value unchanged in `pw-dump`, while the same push on a running node takes
/// effect.
pub(crate) fn graph_snapshot() -> io::Result<Vec<Value>> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-dump")
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/pw-dump"])
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .run()
        .map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "pw-dump exited with {:?}",
            output.status.code(),
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(io::Error::other)
}

fn parse_live_filter_nodes(graph: &[Value]) -> HashMap<String, u32> {
    graph
        .iter()
        .filter(|object| object["type"] == "PipeWire:Interface:Node")
        .filter(|object| matches!(object["info"]["state"].as_str(), Some("running" | "idle")))
        .filter_map(|object| {
            let name = object["info"]["props"]["node.name"].as_str()?;
            let id = u32::try_from(object["id"].as_u64()?).ok()?;
            Some((name.to_owned(), id))
        })
        .collect()
}

/// Push a set of control values to a node via `pw-cli s <id> Props`.
fn set_props(node_id: u32, controls: &[(String, f64)]) -> io::Result<()> {
    if controls.is_empty() {
        return Ok(());
    }
    let payload = format_params(controls);
    debug!("live: pw-cli s {node_id} Props {payload}");
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-cli")
        .args(["s", &node_id.to_string(), "Props", &payload])
        .timeout(std::time::Duration::from_secs(2))
        .stdout(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/pw-cli"])
        .build()
        .run()
        .map_err(io::Error::other)?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = output.stderr_lossy().trim().to_owned();
    Err(io::Error::other(format!(
        "pw-cli set-param {node_id} exited with {:?}: {stderr}",
        output.status.code()
    )))
}

fn format_params(controls: &[(String, f64)]) -> String {
    let mut out = String::from("{ params = [ ");
    for (i, (k, v)) in controls.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push('"');
        out.push_str(k);
        out.push_str("\" ");
        out.push_str(&format_f64(*v));
    }
    out.push_str(" ] }");
    out
}

fn format_f64(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

// ── Parameter extraction ─────────────────────────────────────────────

/// Controls for the mic filter-chain node, mirrored from
/// [`crate::pipeline::build_mic_conf_for`].
///
/// PipeWire exposes filter-chain controls under keys of the form
/// `<graph_node_name>:<control>`. Sending a bare `Strength` (no
/// prefix) would match nothing and get silently discarded — so every
/// entry here is spelled out with the node prefix we declared in
/// `mic.rs`.
fn mic_params(s: &AppSettings) -> Vec<(String, f64)> {
    let nr = &s.noise_reduction;
    let gate = &s.gate;
    let gate_derived = gate.ladspa_controls();
    let threshold_db = if gate.enabled {
        gate_derived.threshold_db
    } else {
        -80.0
    };
    let hpf_freq = if s.hpf.enabled {
        f64::from(s.hpf.frequency)
    } else {
        5.0
    };

    let mut params = vec![
        // HPF — the filter-chain builtin `bq_highpass` node. When the
        // user enables HPF the chain is a cascade of two identical
        // biquads (`hpf_pre` → `hpf`); both controls are addressed
        // here so a frequency change applies to the full slope.
        // `hpf_pre` only exists when HPF is enabled — sending the key
        // when the node is absent is a no-op (pw-cli silently drops
        // unknown control names) so it's safe to always include.
        ("hpf:Freq".to_owned(), hpf_freq),
        ("hpf_pre:Freq".to_owned(), hpf_freq),
    ];

    // GTCRN controls only matter when the node is in the graph; when
    // it isn't, sending the keys would just be no-ops (pw-cli silently
    // drops unknown control names) but pruning them keeps the trace
    // log honest about what the running graph actually accepts.
    if ai_node_in_mic_chain(s) {
        if nr.model.is_attenuation_only() {
            // Attenuation-only models have a single live-tunable knob:
            // the attenuation cap driven by the user's strength slider.
            // The gate (when enabled) lives in a separate `gate:` SWH-gate node.
            let atten_db = deepfilter_attenuation_db(nr.strength);
            params.push(("ai:Attenuation Limit (dB)".to_owned(), atten_db));
        } else {
            params.extend([
                ("ai:Enable".to_owned(), if nr.enabled { 1.0 } else { 0.0 }),
                ("ai:Strength".to_owned(), f64::from(nr.strength)),
                ("ai:Model".to_owned(), f64::from(nr.model.ladspa_control())),
                (
                    "ai:SpeechStrength".to_owned(),
                    gtcrn_speech_strength(nr.strength),
                ),
                ("ai:LookaheadMs".to_owned(), f64::from(nr.lookahead_ms)),
                ("ai:ModelBlend".to_owned(), f64::from(nr.model_blending)),
                ("ai:VoiceRecovery".to_owned(), f64::from(nr.voice_recovery)),
                // Integrated gate (same LADSPA plugin as GTCRN).
                ("ai:Threshold (dB)".to_owned(), threshold_db),
                ("ai:Attack (ms)".to_owned(), gate_derived.attack_ms),
                ("ai:Hold (ms)".to_owned(), gate_derived.hold_ms),
                ("ai:Release (ms)".to_owned(), gate_derived.release_ms),
                ("ai:Range (dB)".to_owned(), gate_derived.range_db),
            ]);
        }
    }

    if nr.model.is_attenuation_only() && gate.enabled {
        params.extend([
            ("gate:Threshold (dB)".to_owned(), gate_derived.threshold_db),
            ("gate:Attack (ms)".to_owned(), gate_derived.attack_ms),
            ("gate:Hold (ms)".to_owned(), gate_derived.hold_ms),
            ("gate:Decay (ms)".to_owned(), gate_derived.release_ms),
            ("gate:Range (dB)".to_owned(), gate_derived.range_db),
        ]);
    }
    append_compressor_params(
        &mut params,
        "compressor",
        s.compressor,
        s.compressor.enabled,
    );
    if s.gain_safety == crate::config::GainSafety::Automatic {
        params.push(("headroom:Mult".into(), crate::pipeline::mic_headroom(s)));
    }
    params
}

/// Controls for the output filter-chain node.
///
/// Master-off (`output_filter.enabled = false`) forces every sub-effect
/// to bypass — the same policy used by the on-disk configuration while
/// the output service is stopping or before it is reconciled.
fn output_params(s: &AppSettings) -> Vec<(String, f64)> {
    let mut parameters = output_params_mono(s);
    if s.output_filter.channel_mode == crate::config::OutputChannelMode::Stereo {
        let right: Vec<_> = parameters
            .iter()
            .map(|(key, value)| (format!("right_{key}"), *value))
            .collect();
        parameters.extend(right);
    }
    parameters
}

fn output_params_mono(s: &AppSettings) -> Vec<(String, f64)> {
    let of = &s.output_filter;
    let master = of.enabled;
    let nr = &of.noise_reduction;
    let gate = &of.gate;
    let gate_enabled = master && gate.enabled;
    let hpf_enabled = master && of.hpf.enabled;
    let comp_enabled = master && of.compressor.enabled;
    let gate_derived = gate.ladspa_controls();
    let hpf_freq = if hpf_enabled {
        f64::from(of.hpf.frequency)
    } else {
        5.0
    };

    let mut params = vec![("hpf:Freq".to_owned(), hpf_freq)];

    // GTCRN is always wired in the output graph; toggling master or NR
    // flips its `Enable` port between 0 and 1 while the node is running.
    let ai_processing = output_ai_processing(s);
    if nr.model.is_attenuation_only() {
        let atten_db = if ai_processing {
            deepfilter_attenuation_db(nr.strength)
        } else {
            0.0
        };
        params.push(("ai:Attenuation Limit (dB)".to_owned(), atten_db));
    } else {
        params.extend([
            (
                "ai:Enable".to_owned(),
                if ai_processing { 1.0 } else { 0.0 },
            ),
            ("ai:Strength".to_owned(), f64::from(nr.strength)),
            ("ai:Model".to_owned(), f64::from(nr.model.ladspa_control())),
            // Same strength→speech-strength curve the static conf uses
            // (`output.rs`); sending the raw strength here made the
            // output chain sound different live vs after a reload.
            (
                "ai:SpeechStrength".to_owned(),
                gtcrn_speech_strength(nr.strength),
            ),
            ("ai:LookaheadMs".to_owned(), f64::from(nr.lookahead_ms)),
            ("ai:ModelBlend".to_owned(), f64::from(nr.model_blending)),
            ("ai:VoiceRecovery".to_owned(), f64::from(nr.voice_recovery)),
            // Park GTCRN's integrated gate below the noise floor. The
            // user-facing gate on the output chain is the separate SWH
            // `gate_1410` node; the integrated one must never fire on
            // playback, otherwise quiet audio dips trigger it even with
            // the UI gate toggle off (default port value is -60 dB).
            ("ai:Threshold (dB)".to_owned(), -80.0),
        ]);
    }

    params.extend([
        // Standalone SWH gate — "Output select" = 1.0 bypasses.
        (
            "gate:Threshold (dB)".to_owned(),
            if gate_enabled {
                gate_derived.threshold_db
            } else {
                -80.0
            },
        ),
        ("gate:Attack (ms)".to_owned(), gate_derived.attack_ms),
        ("gate:Hold (ms)".to_owned(), gate_derived.hold_ms),
        ("gate:Decay (ms)".to_owned(), gate_derived.release_ms),
        ("gate:Range (dB)".to_owned(), gate_derived.range_db),
        (
            "gate:Output select (-1 = key listen, 0 = gate, 1 = bypass)".to_owned(),
            if gate_enabled { 0.0 } else { 1.0 },
        ),
    ]);

    append_compressor_params(&mut params, "compressor", of.compressor, comp_enabled);
    if s.gain_safety == crate::config::GainSafety::Automatic {
        params.push(("headroom:Mult".into(), crate::pipeline::output_headroom(s)));
    }
    params
}

/// Append SC4 compressor controls with the given graph-node prefix.
/// The same SC4 instance lives on both chains under the `compressor`
/// node name; passing the prefix explicitly keeps the helper reusable
/// if the mic and output chains ever diverge.
fn append_compressor_params(
    params: &mut Vec<(String, f64)>,
    prefix: &str,
    compressor_config: crate::config::CompressorConfig,
    enabled: bool,
) {
    let compressor_derived = compressor_config.ladspa_controls();
    let keyed = |tail: &str| format!("{prefix}:{tail}");
    params.extend([
        (keyed("RMS/peak"), f64::from(compressor_derived.rms_peak)),
        (
            keyed("Attack time (ms)"),
            f64::from(compressor_derived.attack_ms),
        ),
        (
            keyed("Release time (ms)"),
            f64::from(compressor_derived.release_ms),
        ),
        (
            keyed("Threshold level (dB)"),
            if enabled {
                f64::from(compressor_derived.threshold_db)
            } else {
                0.0
            },
        ),
        (
            keyed("Ratio (1:n)"),
            if enabled {
                f64::from(compressor_derived.ratio)
            } else {
                1.0
            },
        ),
        (
            keyed("Knee radius (dB)"),
            f64::from(compressor_derived.knee_db),
        ),
        (
            keyed("Makeup gain (dB)"),
            if enabled {
                f64::from(compressor_derived.makeup_gain_db)
            } else {
                0.0
            },
        ),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppSettings;

    #[test]
    fn fully_applied_ignores_chains_that_are_disabled() {
        let mut settings = AppSettings::default();
        crate::pipeline::cascade_mic_off(&mut settings);

        assert!(LiveOutcome::default().fully_applied(&settings));
    }

    #[test]
    fn fully_applied_requires_every_enabled_chain() {
        let settings = AppSettings::default();
        assert!(!LiveOutcome::default().fully_applied(&settings));
        assert!(
            LiveOutcome {
                mic_pushed: true,
                output_pushed: false,
            }
            .fully_applied(&settings)
        );

        let settings = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..settings
        };
        assert!(
            !LiveOutcome {
                mic_pushed: true,
                output_pushed: false,
            }
            .fully_applied(&settings)
        );
    }

    #[test]
    fn format_f64_canonical_for_integers_and_floats() {
        assert_eq!(format_f64(1.0), "1.0");
        assert_eq!(format_f64(0.5), "0.5");
        assert_eq!(format_f64(-20.0), "-20.0");
    }

    #[test]
    fn format_params_quotes_keys_and_separates_by_space() {
        let out = format_params(&[
            ("ai:Strength".to_owned(), 0.8),
            ("ai:Enable".to_owned(), 1.0),
        ]);
        assert_eq!(out, r#"{ params = [ "ai:Strength" 0.8 "ai:Enable" 1.0 ] }"#);
    }

    #[test]
    fn format_params_empty_still_valid() {
        let out = format_params(&[]);
        assert_eq!(out, r"{ params = [  ] }");
    }

    #[test]
    fn parse_live_filter_nodes_indexes_names_and_skips_suspended() {
        let graph = serde_json::json!([
            {"id":12,"type":"PipeWire:Interface:Node",
             "info":{"state":"running","props":{"node.name":"alsa_input.usb"}}},
            {"id":42,"type":"PipeWire:Interface:Node",
             "info":{"state":"idle","props":{"node.name":"mic-biglinux-capture"}}},
            {"id":43,"type":"PipeWire:Interface:Node",
             "info":{"state":"suspended","props":{"node.name":"output-biglinux"}}},
            // Not a node, and carries no props: must not land in the map.
            {"id":31,"type":"PipeWire:Interface:Metadata",
             "metadata":[{"key":"default.audio.sink"}]}
        ]);
        let live = parse_live_filter_nodes(graph.as_array().unwrap());

        assert_eq!(live.get("mic-biglinux-capture"), Some(&42));
        assert_eq!(live.get("alsa_input.usb"), Some(&12));
        // `idle` is set up and takes a push; `suspended` silently drops it.
        assert_eq!(
            live.get("output-biglinux"),
            None,
            "a suspended node must look absent so the caller restarts the loader"
        );
        assert_eq!(live.len(), 2);
    }

    #[test]
    fn parse_live_filter_nodes_handles_an_empty_graph() {
        assert!(parse_live_filter_nodes(&[]).is_empty());
    }

    #[test]
    fn attenuation_gate_updates_without_a_neural_node() {
        let mut settings = AppSettings::default();
        settings.noise_reduction.model = crate::config::NoiseModel::DeepFilterNet3;
        settings.noise_reduction.enabled = false;
        settings.gate.enabled = true;
        let controls = mic_params(&settings);
        assert!(!controls.iter().any(|(key, _)| key.starts_with("ai:")));
        assert!(controls.iter().any(|(key, _)| key == "gate:Threshold (dB)"));
    }

    #[test]
    fn mic_params_includes_prefixed_controls() {
        let s = AppSettings::default();
        let p = mic_params(&s);
        // Every control must carry its graph-node prefix.
        assert!(p.iter().any(|(k, _)| k == "ai:Enable"));
        assert!(p.iter().any(|(k, _)| k == "ai:Strength"));
        assert!(p.iter().any(|(k, _)| k == "hpf:Freq"));
        assert!(p.iter().any(|(k, _)| k == "compressor:Ratio (1:n)"));
    }

    #[test]
    fn mic_params_bypass_hpf_when_disabled() {
        let s = AppSettings {
            hpf: crate::config::HpfConfig {
                enabled: false,
                frequency: 200.0,
            },
            ..AppSettings::default()
        };
        let p = mic_params(&s);
        let (_, freq) = p.iter().find(|(k, _)| k == "hpf:Freq").unwrap();
        assert!((*freq - 5.0).abs() < 1e-9);
    }

    #[test]
    fn mic_params_threshold_drops_when_gate_disabled() {
        let s = AppSettings {
            gate: crate::config::GateConfig {
                enabled: false,
                intensity: 30,
            },
            ..AppSettings::default()
        };
        let p = mic_params(&s);
        let (_, th) = p.iter().find(|(k, _)| k == "ai:Threshold (dB)").unwrap();
        assert!((*th - -80.0).abs() < 1e-9);
    }

    #[test]
    fn output_params_uses_swh_output_select_bypass() {
        let s = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        let p = output_params(&s);
        let key = "gate:Output select (-1 = key listen, 0 = gate, 1 = bypass)";
        let (_, sel) = p.iter().find(|(k, _)| k == key).unwrap();
        // default gate is disabled on output
        assert!((*sel - 1.0).abs() < 1e-9);
    }

    #[test]
    fn mic_params_does_not_push_pitch_controls() {
        // Pitch nodes are conditional on the voice changer being on, so
        // the live updater stays out of that lane — topology changes
        // already trigger a chain reload that picks the new values up.
        let s = AppSettings::default();
        let p = mic_params(&s);
        assert!(!p.iter().any(|(k, _)| k.starts_with("pitch")));
    }

    #[test]
    fn output_params_includes_ai_prefix() {
        let s = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        let p = output_params(&s);
        assert!(p.iter().any(|(k, _)| k == "ai:Strength"));
        assert!(p.iter().any(|(k, _)| k == "ai:Enable"));
        assert!(p.iter().any(|(k, _)| k == "hpf:Freq"));
        assert!(p.iter().any(|(k, _)| k == "gate:Threshold (dB)"));
        assert!(p.iter().any(|(k, _)| k == "compressor:Makeup gain (dB)"));
    }
}
