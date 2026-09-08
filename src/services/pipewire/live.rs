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

use std::io;

use crate::config::dynamics::GateDerived;
use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use log::{debug, trace};

use crate::config::{
    AppSettings, GATE_INTENSITY_MAX, deepfilter_attenuation_db, gtcrn_speech_strength,
};
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

/// Push live control updates for both the mic and (when enabled) the
/// output filter chain. See [`LiveOutcome`] for the semantics of the
/// returned value.
pub fn apply_live(settings: &AppSettings) -> io::Result<LiveOutcome> {
    // Target the **capture-side** node: that's where the filter-chain
    // module exposes its LADSPA control surface. The outward-facing
    // `mic-biglinux` node is just the audio-adapter wrapper and its
    // `Props` only carries channel-mix / resampler settings.
    let mic_pushed = if let Some(id) = find_live_node(MIC_CAPTURE_NODE_NAME)? {
        set_props(id, &mic_params(settings))?;
        true
    } else {
        trace!("live: mic filter-chain not live, skipping");
        false
    };

    // Try the output controls even when the master switch is off. The
    // node may still exist briefly while the service is being stopped,
    // and `output_params` supplies safe bypass values for that transition.
    let output_pushed = if let Some(id) = find_live_node(OUTPUT_NODE_NAME)? {
        set_props(id, &output_params(settings))?;
        true
    } else {
        trace!("live: output filter-chain not live, skipping");
        false
    };

    Ok(LiveOutcome {
        mic_pushed,
        output_pushed,
    })
}

/// Resolve a `node.name` to the id of a node that can actually take a
/// Props update, or `None` when the chain is not there yet.
///
/// A filter-chain node that never had a consumer sits in `suspended`:
/// its filter graph is only set up at format negotiation, and until
/// then PipeWire accepts the `set-param` and drops it — `pw-cli` still
/// exits 0 and the control keeps its load-time value. Reporting `None`
/// makes the caller fall back to a loader restart, which re-reads the
/// args file the reconciler just wrote. Verified on PipeWire 1.6.8:
/// a `Props` push to a suspended node leaves the value unchanged in
/// `pw-dump`, while the same push on a running node takes effect.
fn find_live_node(node_name: &str) -> io::Result<Option<u32>> {
    let Some(id) = find_node_id(node_name)? else {
        return Ok(None);
    };
    if node_state(id)?.as_deref() == Some("suspended") {
        debug!("live: node {node_name} ({id}) is suspended, needs a loader restart");
        return Ok(None);
    }
    Ok(Some(id))
}

/// Read a node's state (`suspended` / `idle` / `running`) from
/// `pw-cli info <id>`. `None` when the object disappeared between the
/// lookup and this call, or when the output has no state line.
fn node_state(node_id: u32) -> io::Result<Option<String>> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-cli")
        .args(["info", &node_id.to_string()])
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/pw-cli"])
        .build()
        .run()
        .map_err(io::Error::other)?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(parse_node_state(&output.stdout_lossy()))
}

/// Pull the state out of `pw-cli info` output. The line is rendered as
/// `*\tstate: "running"`, with the `*` marking a changed field.
fn parse_node_state(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        line.trim_start_matches(['*', ' ', '\t'])
            .strip_prefix("state: ")
            .map(|state| state.trim().trim_matches('"').to_owned())
    })
}

/// Resolve a `node.name` to its current PipeWire object id by parsing
/// `pw-cli ls Node`. Returns `None` when no matching node exists.
///
/// The expected output shape is:
///
/// ```text
///         id 123, type PipeWire:Interface:Node/3
///   …
///                 node.name = "mic-biglinux"
///   …
/// ```
///
/// The parser tracks the most recent id header so we can associate the
/// `node.name` line that follows it with the right object.
fn find_node_id(node_name: &str) -> io::Result<Option<u32>> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-cli")
        .args(["ls", "Node"])
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/pw-cli"])
        .build()
        .run()
        .map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "pw-cli ls Node exited with {:?}",
            output.status.code(),
        )));
    }
    let stdout = output.stdout_lossy();
    Ok(parse_node_id(&stdout, node_name))
}

fn parse_node_id(stdout: &str, node_name: &str) -> Option<u32> {
    let mut current_id: Option<u32> = None;
    let needle = format!("node.name = \"{node_name}\"");
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("id ") {
            // "id 123, type …"
            let id_token = rest.split(',').next().unwrap_or("").trim();
            current_id = id_token.parse().ok();
        } else if trimmed.contains(&needle) {
            return current_id;
        }
    }
    None
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
    let gate_derived = GateDerived::from_unit_intensity(
        f64::from(gate.intensity.min(GATE_INTENSITY_MAX)) / f64::from(GATE_INTENSITY_MAX),
    );
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
            if s.gate.enabled {
                params.extend([
                    ("gate:Threshold (dB)".to_owned(), gate_derived.threshold_db),
                    ("gate:Attack (ms)".to_owned(), gate_derived.attack_ms),
                    ("gate:Hold (ms)".to_owned(), gate_derived.hold_ms),
                    ("gate:Decay (ms)".to_owned(), gate_derived.release_ms),
                    ("gate:Range (dB)".to_owned(), gate_derived.range_db),
                ]);
            }
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

    append_compressor_params(
        &mut params,
        "compressor",
        s.compressor,
        s.compressor.enabled,
    );
    params
}

/// Controls for the output filter-chain node.
///
/// Master-off (`output_filter.enabled = false`) forces every sub-effect
/// to bypass — the same policy used by the on-disk configuration while
/// the output service is stopping or before it is reconciled.
fn output_params(s: &AppSettings) -> Vec<(String, f64)> {
    let of = &s.output_filter;
    let master = of.enabled;
    let nr = &of.noise_reduction;
    let gate = &of.gate;
    let gate_enabled = master && gate.enabled;
    let hpf_enabled = master && of.hpf.enabled;
    let comp_enabled = master && of.compressor.enabled;
    let gate_derived = GateDerived::from_unit_intensity(
        f64::from(gate.intensity.min(GATE_INTENSITY_MAX)) / f64::from(GATE_INTENSITY_MAX),
    );
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
    fn parse_node_id_extracts_correct_id() {
        let stdout = "\t\tid 12, type PipeWire:Interface:Node/3\n\
                      \t\t\tfactory.id = \"9\"\n\
                      \t\t\tnode.name = \"alsa_input.usb\"\n\
                      \t\tid 42, type PipeWire:Interface:Node/3\n\
                      \t\t\tnode.name = \"mic-biglinux\"\n\
                      \t\t\tmedia.class = \"Audio/Source\"\n";
        assert_eq!(parse_node_id(stdout, "mic-biglinux"), Some(42));
        assert_eq!(parse_node_id(stdout, "alsa_input.usb"), Some(12));
        assert_eq!(parse_node_id(stdout, "does-not-exist"), None);
    }

    #[test]
    fn parse_node_id_handles_empty_output() {
        assert_eq!(parse_node_id("", "mic-biglinux"), None);
    }

    #[test]
    fn parse_node_state_reads_the_starred_state_line() {
        // `pw-cli info <id>` marks changed fields with a leading `*`
        // and quotes the state value.
        let stdout = "\tid: 79\n\
                      \tpermissions: rwxm-\n\
                      \ttype: PipeWire:Interface:Node/3\n\
                      *\tinput ports: 1/129\n\
                      *\tstate: \"running\"\n";
        assert_eq!(parse_node_state(stdout), Some("running".to_owned()));

        let suspended = "*\tstate: \"suspended\"\n";
        assert_eq!(parse_node_state(suspended), Some("suspended".to_owned()));
    }

    #[test]
    fn parse_node_state_handles_output_without_a_state_line() {
        assert_eq!(parse_node_state(""), None);
        assert_eq!(parse_node_state("\tid: 79\n\tpermissions: rwxm-\n"), None);
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
