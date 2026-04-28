//! Live control updates on running filter-chain nodes.
//!
//! The filter-chain module exposes every LADSPA / builtin control as an
//! SPA property under the node's `Props` parameter, keyed by the control
//! name we declared in the `.conf` file (`"Strength"`, `"Enable"`,
//! `"Freq"`, …). `pw-cli s <id> Props '{ params = [ … ] }'` pushes a new
//! value without touching the module graph, so slider drags update the
//! running audio pipeline in real time — no `filter-chain.service`
//! restart, no pop, no drop-out.
//!
//! The helper intentionally stays shell-based:
//!
//! - Transparent to debug (`sh -x` or `strace`).
//! - Uses the same parser PipeWire itself ships, so property-name typos
//!   surface with a clear error message instead of silently corrupt POD.
//! - Zero-cost to replace later with a native `libspa` POD builder when
//!   we decide the subprocess overhead matters.
//!
//! Gracefully no-ops when the target node is absent (e.g. the first run
//! before `filter-chain.service` has loaded the drop-in config). A
//! separate service restart is still required whenever the **graph
//! structure** changes — adding/removing a filter, swapping the routing
//! mode — because only the control values can be updated live.

use std::io;
use std::process::{Command, Stdio};

use log::{debug, trace, warn};

use crate::config::{
    deepfilter_attenuation_db, AppSettings, CompressorDerived, GateDerived, EQ_BANDS_HZ,
    EQ_BAND_COUNT,
};
use crate::pipeline::{
    ai_node_in_mic_chain, output_ai_processing, MIC_CAPTURE_NODE_NAME, OUTPUT_NODE_NAME,
};

/// Result of a [`apply_live`] call.
///
/// `*_pushed` is `true` when the corresponding filter-chain node was
/// found in the PipeWire graph and its controls were updated. A
/// `false` tells the caller the running graph is stale — typically
/// the first time the user edits a setting in a session where the
/// `filter-chain.service` has not yet been (re)loaded. The caller
/// should then trigger a full service restart to bring the freshly
/// written `.conf` drop-ins into effect.
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
        self.mic_pushed && (!settings.output_filter.enabled || self.output_pushed)
    }
}

/// Push live control updates for both the mic and (when enabled) the
/// output filter chain. See [`LiveOutcome`] for the semantics of the
/// returned value.
pub fn apply_live(settings: &AppSettings) -> io::Result<LiveOutcome> {
    // Clear the legacy external override file shipped by the Python
    // implementation. While this file exists the GTCRN plugin reads
    // its values from it and silently ignores the LADSPA port values
    // we push through PipeWire.
    clear_legacy_external_override();

    // Target the **capture-side** node: that's where the filter-chain
    // module exposes its LADSPA control surface. The outward-facing
    // `mic-biglinux` node is just the audio-adapter wrapper and its
    // `Props` only carries channel-mix / resampler settings.
    let mic_pushed = if let Some(id) = find_node_id(MIC_CAPTURE_NODE_NAME)? {
        set_props(id, &mic_params(settings))?;
        true
    } else {
        trace!("live: mic filter-chain not loaded, skipping");
        false
    };

    // Always try to push the output controls — even when the master
    // switch is off, `output_params` returns the bypass values that
    // keep the filter graph loaded but transparent. This avoids the
    // service teardown that would otherwise yank the smart-filter sink
    // out from under any active stream.
    let output_pushed = if let Some(id) = find_node_id(OUTPUT_NODE_NAME)? {
        set_props(id, &output_params(settings))?;
        true
    } else {
        trace!("live: output filter-chain not loaded, skipping");
        false
    };

    Ok(LiveOutcome {
        mic_pushed,
        output_pushed,
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
    let output = Command::new("pw-cli")
        .args(["ls", "Node"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "pw-cli ls Node exited with {}",
            output.status,
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
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
    let status = Command::new("pw-cli")
        .args(["s", &node_id.to_string(), "Props", &payload])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()?;
    if !status.success() {
        warn!("live: pw-cli set-param {node_id} failed with {status}");
    }
    Ok(())
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

/// Delete the legacy `gtcrn-ladspa-controls` override file the Python
/// implementation left on tmpfs. Silently ignored if the file doesn't
/// exist — which is the common case after the first call.
fn clear_legacy_external_override() {
    let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return;
    };
    let path = std::path::Path::new(&dir).join("gtcrn-ladspa-controls");
    match std::fs::remove_file(&path) {
        Ok(()) => debug!("live: removed stale {}", path.display()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => warn!("live: could not remove {}: {e}", path.display()),
    }
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
/// [`crate::pipeline::mic::build_mic_conf`].
///
/// PipeWire exposes filter-chain controls under keys of the form
/// `<graph_node_name>:<control>`. Sending a bare `Strength` (no
/// prefix) would match nothing and get silently discarded — so every
/// entry here is spelled out with the node prefix we declared in
/// `mic.rs`.
fn mic_params(s: &AppSettings) -> Vec<(String, f64)> {
    let nr = &s.noise_reduction;
    let gate = &s.gate;
    let gate_derived = GateDerived::from_config(gate);
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
        if nr.model.is_deepfilter() {
            // DFN3 has a single live-tunable knob — the attenuation cap
            // driven by the user's strength slider. The gate (when
            // enabled) lives in a separate `gate:` SWH-gate node.
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
                ("ai:SpeechStrength".to_owned(), f64::from(nr.strength)),
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
    append_eq_params(&mut params, &s.equalizer.bands, s.equalizer.enabled);
    params
}

/// Controls for the output filter-chain node.
///
/// Master-off (`output_filter.enabled = false`) forces every sub-effect
/// to bypass — same policy as the on-disk conf in `pipeline::output`.
/// The standalone unit stays running so the smart-filter sink keeps
/// streams attached and browsers don't pause playback when the user
/// flips the toggle off.
fn output_params(s: &AppSettings) -> Vec<(String, f64)> {
    let of = &s.output_filter;
    let master = of.enabled;
    let nr = &of.noise_reduction;
    let gate = &of.gate;
    let gate_enabled = master && gate.enabled;
    let hpf_enabled = master && of.hpf.enabled;
    let comp_enabled = master && of.compressor.enabled;
    let gate_derived = GateDerived::from_config(gate);
    let hpf_freq = if hpf_enabled {
        f64::from(of.hpf.frequency)
    } else {
        5.0
    };

    let mut params = vec![("hpf:Freq".to_owned(), hpf_freq)];

    // GTCRN is always wired in the output graph; toggling master or NR
    // flips its `Enable` port between 0 and 1 so the live update path
    // is the one and only path that reflects the user's choice — the
    // standalone unit stays running so streams don't get yanked.
    let ai_processing = output_ai_processing(s);
    if nr.model.is_deepfilter() {
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
            ("ai:SpeechStrength".to_owned(), f64::from(nr.strength)),
            ("ai:LookaheadMs".to_owned(), f64::from(nr.lookahead_ms)),
            ("ai:ModelBlend".to_owned(), f64::from(nr.model_blending)),
            ("ai:VoiceRecovery".to_owned(), f64::from(nr.voice_recovery)),
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
    append_eq_params(
        &mut params,
        &of.equalizer.bands,
        master && of.equalizer.enabled,
    );
    params
}

/// Append SC4 compressor controls with the given graph-node prefix.
/// The same SC4 instance lives on both chains under the `compressor`
/// node name; passing the prefix explicitly keeps the helper reusable
/// if the mic and output chains ever diverge.
fn append_compressor_params(
    params: &mut Vec<(String, f64)>,
    prefix: &str,
    cfg: crate::config::CompressorConfig,
    enabled: bool,
) {
    let d = CompressorDerived::from_intensity(cfg.intensity);
    let keyed = |tail: &str| format!("{prefix}:{tail}");
    params.extend([
        (keyed("RMS/peak"), f64::from(d.rms_peak)),
        (keyed("Attack time (ms)"), f64::from(d.attack_ms)),
        (keyed("Release time (ms)"), f64::from(d.release_ms)),
        (
            keyed("Threshold level (dB)"),
            if enabled {
                f64::from(d.threshold_db)
            } else {
                0.0
            },
        ),
        (
            keyed("Ratio (1:n)"),
            if enabled { f64::from(d.ratio) } else { 1.0 },
        ),
        (keyed("Knee radius (dB)"), f64::from(d.knee_db)),
        (
            keyed("Makeup gain (dB)"),
            if enabled {
                f64::from(d.makeup_gain_db)
            } else {
                0.0
            },
        ),
    ]);
}

fn append_eq_params(_params: &mut [(String, f64)], _bands: &[f32], _enabled: bool) {
    // The `param_eq` builtin takes its filter list as a config block,
    // not as per-band controls, so EQ live updates still require a
    // filter-chain reload. Left as a no-op here to keep the function
    // surface consistent with the other helpers.
    let _ = EQ_BAND_COUNT;
    let _ = EQ_BANDS_HZ;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppSettings;

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
