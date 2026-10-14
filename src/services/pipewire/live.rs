//! Live controls through a persistent, control-thread-owned PipeWire client.
//! A graph snapshot establishes the expected core cookie and node generation;
//! the native client checks them before writing through its bound proxy, then
//! confirms readable Props after a sync barrier. No pw-cli write or shell.
//! Absent nodes may be repaired; unconfirmed controls on existing nodes stay pending.
//! This is per-node confirmation, not a transaction across multiple nodes or
//! a guarantee about which audio frame first sees a new control value.

use std::collections::{HashMap, HashSet};
use std::io;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use serde_json::Value;

use crate::config::{AppSettings, deepfilter_attenuation_db};
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
/// The caller must distinguish absence from a failed write to an existing
/// endpoint. The latter is reported as pending, never an automatic restart.
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
    let mut report = LiveReport::default();
    if targets.microphone {
        report.pushed.mic_pushed = push_one(
            graph,
            MIC_CAPTURE_NODE_NAME,
            || mic_params(settings),
            "microphone",
            &mut report.failures,
        );
    }
    if targets.output {
        report.pushed.output_pushed = push_one(
            graph,
            OUTPUT_NODE_NAME,
            || output_params(settings),
            "output",
            &mut report.failures,
        );
    }
    report
}

fn push_one(
    graph: &[Value],
    name: &str,
    controls: impl FnOnce() -> Vec<(String, f64)>,
    chain: &str,
    failures: &mut Vec<String>,
) -> bool {
    let Some(node) = unique_live_node(graph, name) else {
        return false;
    };
    let Some(id) = node["id"].as_u64().and_then(|id| u32::try_from(id).ok()) else {
        return false;
    };
    let serial = &node["info"]["props"]["object.serial"];
    let serial = serial
        .as_str()
        .map(str::to_owned)
        .or_else(|| serial.as_u64().map(|n| n.to_string()));
    let cookie = graph
        .iter()
        .find(|o| o["type"] == "PipeWire:Interface:Core")
        .and_then(|o| o["info"]["cookie"].as_u64())
        .and_then(|n| u32::try_from(n).ok());
    let (Some(serial), Some(cookie)) = (serial, cookie) else {
        failures.push(format!("{chain}: missing core cookie/node serial"));
        return false;
    };
    match super::native::set_props(name, id, &serial, cookie, &controls()) {
        Ok(()) => true,
        Err(error) => {
            failures.push(format!("{chain} live update: {error}"));
            false
        }
    }
}

/// `node.name` → object id for every node whose filter graph is set up.
///
/// Snapshot for initial identity/readiness, not a receipt for a write.
/// Native control binds the matching generation and verifies exposed Props.
/// Suspended nodes remain excluded conservatively: successful marshalling is
/// not evidence that a filter graph has negotiated or applied its controls.
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

#[cfg(test)]
fn parse_live_filter_nodes(graph: &[Value]) -> HashMap<String, u32> {
    let mut nodes = HashMap::new();
    let mut ambiguous = HashSet::new();
    for object in graph
        .iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Node")
    {
        let Some(name) = object["info"]["props"]["node.name"].as_str() else {
            continue;
        };
        let Some(id) = object["id"].as_u64().and_then(|n| u32::try_from(n).ok()) else {
            continue;
        };
        // A duplicate must never silently replace the identity selected earlier.
        if nodes.insert(name.to_owned(), id).is_some() {
            ambiguous.insert(name.to_owned());
        }
    }
    nodes.retain(|name, id| {
        !ambiguous.contains(name)
            && graph.iter().any(|o| {
                o["id"].as_u64() == Some(u64::from(*id))
                    && matches!(o["info"]["state"].as_str(), Some("running" | "idle"))
            })
    });
    nodes
}

fn unique_live_node<'a>(graph: &'a [Value], name: &str) -> Option<&'a Value> {
    let mut matches = graph.iter().filter(|o| {
        o["type"] == "PipeWire:Interface:Node"
            && o["info"]["props"]["node.name"].as_str() == Some(name)
    });
    let node = matches.next()?;
    if matches.next().is_some()
        || !matches!(node["info"]["state"].as_str(), Some("running" | "idle"))
    {
        return None;
    }
    Some(node)
}

#[cfg(test)]
fn control_values(node: &Value) -> HashMap<&str, f64> {
    let mut values = HashMap::new();
    if let Some(props) = node["info"]["params"]["Props"].as_array() {
        for entry in props {
            if let Some(params) = entry["params"].as_array() {
                for pair in params.as_chunks::<2>().0 {
                    if let (Some(key), Some(value)) = (pair[0].as_str(), pair[1].as_f64()) {
                        values.insert(key, value);
                    }
                }
            }
        }
    }
    values
}

#[cfg(test)]
fn confirmed_update(before: &[Value], after: &[Value], name: &str, sent: &[(String, f64)]) -> bool {
    let (Some(old), Some(new)) = (
        unique_live_node(before, name),
        unique_live_node(after, name),
    ) else {
        return false;
    };
    let serial = &old["info"]["props"]["object.serial"];
    if serial.is_null()
        || serial != &new["info"]["props"]["object.serial"]
        || old["id"] != new["id"]
        || !unknown_live_controls(old, sent).is_empty()
    {
        return false;
    }
    let old_values = control_values(old);
    let new_values = control_values(new);
    let mut checked = 0;
    for (key, expected) in sent {
        // Keep the existing optional-node contract, but do not count a batch
        // with no readable controls as a successful write.
        if !old_values.contains_key(key.as_str()) {
            continue;
        }
        checked += 1;
        let Some(actual) = new_values.get(key.as_str()) else {
            return false;
        };
        let tolerance = 1e-5 * expected.abs().max(1.0); // f32 POD readback
        if !expected.is_finite() || !actual.is_finite() || (actual - expected).abs() > tolerance {
            return false;
        }
    }
    checked > 0 || sent.is_empty()
}

/// Filter-graph control keys a node actually exposes, as `prefix -> {controls}`.
///
/// `pw-dump` renders each `Props` param object's `params` field as a flat
/// `[key, value, key, value, …]` list; the filter-graph controls are the keys
/// shaped `<graph_node>:<control>` (e.g. `hpf:Freq`). Audioconvert keys without
/// a `:` prefix are ignored.
fn exposed_graph_controls(node: &Value) -> HashMap<String, HashSet<String>> {
    let mut map: HashMap<String, HashSet<String>> = HashMap::new();
    let Some(props) = node["info"]["params"]["Props"].as_array() else {
        return map;
    };
    for entry in props {
        let Some(list) = entry["params"].as_array() else {
            continue;
        };
        for pair in list.as_chunks::<2>().0 {
            if let Some((prefix, control)) = pair[0].as_str().and_then(|k| k.split_once(':')) {
                map.entry(prefix.to_owned())
                    .or_default()
                    .insert(control.to_owned());
            }
        }
    }
    map
}

/// Sent control keys that address a *present* graph node but a control it does
/// not expose. `pw-cli s <owned-node-name> Props` silently drops such keys, so the value
/// never takes effect — a typo or a stale control name looks like success
/// (review API-01).
///
/// Keys whose whole node prefix is absent are deliberately skipped: several
/// optional nodes (`hpf_pre`, …) are addressed unconditionally and are a
/// harmless no-op when the node is not instantiated. An empty exposed set means
/// the node is suspended / not yet format-negotiated, so there is no schema to
/// check against and nothing is reported.
fn unknown_live_controls(node: &Value, sent: &[(String, f64)]) -> Vec<String> {
    let exposed = exposed_graph_controls(node);
    if exposed.is_empty() {
        return Vec::new();
    }
    sent.iter()
        .filter_map(|(key, _)| {
            let (prefix, control) = key.split_once(':')?;
            let controls = exposed.get(prefix)?;
            (!controls.contains(control)).then(|| key.clone())
        })
        .collect()
}

/// Audit the live controls the daemon would push against what the running node
/// exposes. `None` when the node is absent or not running (nothing to audit);
/// `Some(unknown)` lists sent keys that would be silently dropped.
pub(crate) fn unknown_controls_for(
    graph: &[Value],
    node_name: &str,
    sent: &[(String, f64)],
) -> Option<Vec<String>> {
    let node = graph.iter().find(|object| {
        object["type"] == "PipeWire:Interface:Node"
            && object["info"]["props"]["node.name"].as_str() == Some(node_name)
    })?;
    if !matches!(node["info"]["state"].as_str(), Some("running" | "idle")) {
        return None;
    }
    Some(unknown_live_controls(node, sent))
}

/// Transport health of a filter-chain node, read from a `pw-dump` snapshot.
///
/// Presence alone does not prove a chain is working: a node can be `created`,
/// `suspended`, in `error`, or `running` with no consumer. This separates the
/// node's own state from whether any of its links are actively carrying frames
/// (review HEALTH-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeTransport {
    pub state: String,
    pub active_links: usize,
}

/// Read a node's state and active-link count. `None` when the node is not in
/// the graph — presence is the caller's separate concern.
pub(crate) fn node_transport(graph: &[Value], node_name: &str) -> Option<NodeTransport> {
    let node = graph.iter().find(|object| {
        object["type"] == "PipeWire:Interface:Node"
            && object["info"]["props"]["node.name"].as_str() == Some(node_name)
    })?;
    let id = node["id"].as_u64()?;
    let state = node["info"]["state"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    let active_links = graph
        .iter()
        .filter(|object| object["type"] == "PipeWire:Interface:Link")
        .filter(|link| link["info"]["state"].as_str() == Some("active"))
        .filter(|link| {
            link["info"]["output-node-id"].as_u64() == Some(id)
                || link["info"]["input-node-id"].as_u64() == Some(id)
        })
        .count();
    Some(NodeTransport {
        state,
        active_links,
    })
}

#[cfg(test)]
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

#[cfg(test)]
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
pub(crate) fn mic_params(s: &AppSettings) -> Vec<(String, f64)> {
    let nr = &s.noise_reduction;
    let gate = &s.gate;
    let gate_derived = gate.ladspa_controls();
    let hpf_freq = if s.hpf.enabled {
        f64::from(s.hpf.frequency)
    } else {
        5.0
    };

    let mut params = vec![("hpf:Freq".to_owned(), hpf_freq)];
    // Address only nodes actually emitted by the renderer. Native read-back
    // still validates the generation and the writable control set.
    if s.hpf.enabled {
        params.push(("hpf_pre:Freq".to_owned(), hpf_freq));
    }

    // Address only the denoiser that is in the graph; pw-cli silently drops
    // unknown control names, but pruning them keeps the trace log honest.
    if ai_node_in_mic_chain(s) {
        let atten_db = deepfilter_attenuation_db(nr.strength);
        params.extend(
            nr.attenuation_controls(atten_db, crate::pipeline::mic_min_snr_gate_db(s))
                .into_iter()
                .map(|(k, v)| (format!("ai:{k}"), v)),
        );
    }

    if gate.enabled {
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
pub(crate) fn output_params(s: &AppSettings) -> Vec<(String, f64)> {
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

    // Master or NR off sets the attenuation to 0 while the node keeps running.
    let ai_processing = output_ai_processing(s);
    let atten_db = if ai_processing {
        deepfilter_attenuation_db(nr.strength)
    } else {
        0.0
    };
    params.extend(
        nr.attenuation_controls(atten_db, nr.min_snr_gate_db())
            .into_iter()
            .map(|(k, v)| (format!("ai:{k}"), v)),
    );

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

    #[test]
    fn live_mic_gate_matches_renderer_with_and_without_aec() {
        for aec in [false, true] {
            for depth in [0, 20, 30, 40] {
                let mut s = AppSettings::default();
                s.echo_cancel.enabled = aec;
                s.noise_reduction.silence_depth = depth;
                let params = mic_params(&s);
                let actual = params
                    .iter()
                    .find(|(k, _)| k == "ai:Min SNR gate (dB)")
                    .unwrap()
                    .1;
                assert_eq!(actual, crate::pipeline::mic_min_snr_gate_db(&s));
                if aec {
                    assert!(actual <= -20.0);
                }
            }
        }
    }

    #[test]
    fn live_hpf_control_set_matches_optional_node() {
        let mut s = AppSettings::default();
        for enabled in [false, true] {
            s.hpf.enabled = enabled;
            assert_eq!(
                mic_params(&s).iter().any(|(k, _)| k == "hpf_pre:Freq"),
                enabled
            );
        }
    }

    use crate::config::AppSettings;

    fn node_with_controls(state: &str, keys: &[&str]) -> Value {
        let params: Vec<Value> = keys
            .iter()
            .flat_map(|k| [Value::from(*k), Value::from(1.0)])
            .collect();
        serde_json::json!({
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": state,
                "props": { "node.name": "mic-biglinux-capture" },
                "params": { "Props": [
                    { "params": ["channelmix.disable", false, "resample.quality", 4] },
                    { "params": params },
                ] },
            },
        })
    }

    #[test]
    fn unknown_controls_flags_typos_under_present_nodes_only() {
        let node = node_with_controls("running", &["hpf:Freq", "ai:Attenuation Limit (dB)"]);
        let sent = vec![
            ("hpf:Freq".to_owned(), 40.0), // present control — accepted
            ("ai:Attenuation Limit (dB)".to_owned(), 90.0), // present control — accepted
            ("ai:Typo".to_owned(), 1.0),   // present node, unknown control — dropped
            ("hpf_pre:Freq".to_owned(), 40.0), // absent optional node — skipped, not flagged
        ];
        assert_eq!(
            unknown_live_controls(&node, &sent),
            vec!["ai:Typo".to_owned()]
        );
    }

    #[test]
    fn suspended_node_has_no_schema_to_audit() {
        // No graph controls exposed yet (empty second Props): report nothing
        // rather than flag every key as unknown.
        let node = node_with_controls("suspended", &[]);
        assert!(unknown_live_controls(&node, &[("hpf:Freq".to_owned(), 40.0)]).is_empty());
        // …and unknown_controls_for skips a non-running node entirely.
        assert!(
            unknown_controls_for(
                std::slice::from_ref(&node),
                "mic-biglinux-capture",
                &[("hpf:Freq".to_owned(), 40.0)]
            )
            .is_none()
        );
    }

    fn graph_with_link(node_state: &str, link_state: Option<&str>) -> Vec<Value> {
        let mut graph = vec![serde_json::json!({
            "id": 78,
            "type": "PipeWire:Interface:Node",
            "info": { "state": node_state, "props": { "node.name": "mic-biglinux-capture" } },
        })];
        if let Some(ls) = link_state {
            graph.push(serde_json::json!({
                "id": 190,
                "type": "PipeWire:Interface:Link",
                "info": { "state": ls, "output-node-id": 190, "input-node-id": 78 },
            }));
        }
        graph
    }

    #[test]
    fn node_transport_separates_state_from_active_links() {
        // Running with an active link → transporting.
        let t = node_transport(
            &graph_with_link("running", Some("active")),
            "mic-biglinux-capture",
        )
        .unwrap();
        assert_eq!(t.state, "running");
        assert_eq!(t.active_links, 1);

        // Running but no consumer → idle, not an error.
        let t = node_transport(&graph_with_link("running", None), "mic-biglinux-capture").unwrap();
        assert_eq!(t.active_links, 0);

        // A link that is not active does not count as transporting.
        let t = node_transport(
            &graph_with_link("running", Some("inactive")),
            "mic-biglinux-capture",
        )
        .unwrap();
        assert_eq!(t.active_links, 0);

        // Error state is surfaced verbatim for the caller to fail on.
        let t = node_transport(&graph_with_link("error", None), "mic-biglinux-capture").unwrap();
        assert_eq!(t.state, "error");

        // Absent node → None (presence is a separate check).
        assert!(node_transport(&graph_with_link("running", None), "output-biglinux").is_none());
    }

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
        // Every control must carry its graph-node prefix. The default is an
        // attenuation-only plugin: one AI knob, gate in its own `gate:` node.
        assert!(p.iter().any(|(k, _)| k == "ai:Attenuation Limit (dB)"));
        assert!(p.iter().any(|(k, _)| k == "hpf:Freq"));
        assert!(p.iter().any(|(k, _)| k == "compressor:Ratio (1:n)"));
        // A plugin build without this port would fail the whole live push.
        assert!(!p.iter().any(|(k, _)| k.starts_with("ai:Voice sensitivity")));
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
    fn mic_params_omits_gate_controls_when_gate_disabled() {
        // The default plugin is attenuation-only: the gate is a separate
        // `gate:` node, emitted only when the gate is on. Off means no gate
        // keys reach the live graph at all.
        let s = AppSettings {
            gate: crate::config::GateConfig {
                enabled: false,
                intensity: 30,
            },
            ..AppSettings::default()
        };
        let p = mic_params(&s);
        assert!(!p.iter().any(|(k, _)| k.starts_with("gate:")));
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
        assert!(p.iter().any(|(k, _)| k == "ai:Attenuation Limit (dB)"));
        assert!(p.iter().any(|(k, _)| k == "hpf:Freq"));
        assert!(p.iter().any(|(k, _)| k == "gate:Threshold (dB)"));
        assert!(p.iter().any(|(k, _)| k == "compressor:Makeup gain (dB)"));
    }
    #[test]
    fn readback_requires_generation_and_actual_control_value() {
        let graph = |serial, value| {
            vec![serde_json::json!({
                "id": 40, "type": "PipeWire:Interface:Node",
                "info": { "state": "running", "props": {
                    "node.name": MIC_CAPTURE_NODE_NAME, "object.serial": serial
                }, "params": {"Props": [{"params": ["ai:Strength", value]}]} }
            })]
        };
        let before = graph(100, 0.3);
        let sent = vec![("ai:Strength".into(), 0.7)];
        assert!(confirmed_update(
            &before,
            &graph(100, 0.7),
            MIC_CAPTURE_NODE_NAME,
            &sent
        ));
        assert!(!confirmed_update(
            &before,
            &graph(101, 0.7),
            MIC_CAPTURE_NODE_NAME,
            &sent
        ));
        assert!(!confirmed_update(
            &before,
            &graph(100, 0.3),
            MIC_CAPTURE_NODE_NAME,
            &sent
        ));
        let mut duplicate = graph(100, 0.7);
        duplicate.extend(graph(101, 0.7));
        assert!(!confirmed_update(
            &before,
            &duplicate,
            MIC_CAPTURE_NODE_NAME,
            &sent
        ));
        assert!(!parse_live_filter_nodes(&duplicate).contains_key(MIC_CAPTURE_NODE_NAME));
    }
}
