//! Discover and manage hardware audio capture devices.
//!
//! The mic-picker UI needs three things: enumerate every real
//! `Audio/Source`, know which one is the system default, and let the
//! user switch + adjust its volume. PipeWire exposes all three through
//! `pw-cli ls Node` (enumeration) and `wpctl` (default + volume).
//!
//! Our own virtual sources (`mic-biglinux*`, `output-biglinux*`,
//! `pw-loopback`) are filtered out — selecting them would create a
//! routing loop and they're not "the real microphone in use" the user
//! is choosing from.

use std::io;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use log::{debug, warn};

use crate::pipeline::{
    EC_CAPTURE_NODE_NAME, EC_SOURCE_NAME, MIC_CAPTURE_NODE_NAME, MIC_NODE_NAME, OUTPUT_NODE_NAME,
};
use crate::services::audio_monitor::METER_NODE_NAME;

/// One hardware capture source visible in the PipeWire graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub node_id: u32,
    /// A global ID can be recycled. Serial + core cookie identify its generation.
    pub object_serial: Option<String>,
    pub core_cookie: Option<u32>,
    /// Stable internal identifier (`alsa_input.usb-…`). Used to match
    /// the value of `default.audio.source` returned by `pw-metadata`.
    pub node_name: String,
    /// Human-friendly name (`AKG C44-USB Microphone …`). Falls back to
    /// `node_name` when no description is published.
    pub description: String,
}

impl Source {
    #[must_use]
    pub fn same_generation(&self, other: &Self) -> bool {
        self.node_id == other.node_id
            && self.node_name == other.node_name
            && self.object_serial.is_some()
            && self.object_serial == other.object_serial
            && self.core_cookie.is_some()
            && self.core_cookie == other.core_cookie
    }

    /// Revalidate immediately before a write. This narrows the CLI TOCTOU
    /// window; only a bound native proxy can eliminate that window entirely.
    #[must_use]
    pub fn is_current_default(&self) -> bool {
        let (sources, default_id) = snapshot();
        sources
            .iter()
            .any(|current| Some(current.node_id) == default_id && self.same_generation(current))
    }

    /// Read volume and mute together. A muted source must not be "rescued" by AGC.
    /// Kept on Source so consumers of snapshot() need no new module re-export.
    #[must_use]
    pub fn volume_and_mute(&self) -> Option<(f32, bool)> {
        read_volume_and_mute(self.node_id)
    }
}

/// Our own nodes link to each other whether or not anybody is listening, so
/// membership here is what separates "the chain is wired up" from "somebody is
/// actually recording". The AGC's own analyser belongs in this list for a
/// sharper reason: if it counted as a consumer it would be its own justification
/// for staying open, and the microphone would never be released.
pub(super) const CHAIN_NODES: [&str; 5] = [
    MIC_CAPTURE_NODE_NAME,
    MIC_NODE_NAME,
    EC_CAPTURE_NODE_NAME,
    EC_SOURCE_NAME,
    METER_NODE_NAME,
];

/// Dump the graph's links (`pw-link -l`). Empty on failure — callers treat an
/// unreadable graph as "no consumer", which only ever errs toward doing less.
#[must_use]
pub fn link_dump() -> String {
    BigSubprocessSpec::builder()
        .program("/usr/bin/pw-link")
        .arg("-l")
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/pw-link"])
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .run()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| o.stdout_lossy())
        .unwrap_or_default()
}

/// Is an application reading the microphone chain?
///
/// A consumer is a link *out* of the chain's source ports into a node that is
/// not itself part of the chain. Decided from a `pw-link -l` dump alone, so the
/// rule can be tested without a graph.
#[must_use]
pub fn has_capture_consumer(lines: &[&str]) -> bool {
    lines.iter().enumerate().any(|(index, line)| {
        let source = line.starts_with(&format!("{MIC_NODE_NAME}:"))
            || line.starts_with(&format!("{EC_SOURCE_NAME}:"));
        source
            && lines
                .iter()
                .skip(index + 1)
                .take_while(|next| next.starts_with(' '))
                .any(|next| {
                    next.contains("|->")
                        && !CHAIN_NODES
                            .iter()
                            .any(|node| next.contains(&format!("{node}:")))
                })
    })
}

/// Same question, against the live graph.
#[must_use]
pub fn chain_has_consumer() -> bool {
    let dump = link_dump();
    has_capture_consumer(&dump.lines().collect::<Vec<_>>())
}

/// Enumerate every real `Audio/Source` node — i.e. hardware mics,
/// excluding our own virtual nodes and the pw-loopback bridge.
pub fn list_sources() -> io::Result<Vec<Source>> {
    let stdout = pw_cli_ls_node()?;
    Ok(parse_sources(&stdout))
}

/// Return the `node.name` of the current default source, or `None`
/// when `pw-metadata` returns no value (e.g. fresh session).
#[must_use]
pub fn default_source_name() -> Option<String> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-metadata")
        .args(["0", "default.audio.source"])
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/pw-metadata"])
        .build()
        .run()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_default_source(&output.stdout_lossy())
}

/// Promote `node_id` to the system default source. WirePlumber rules
/// pick this up live — every app following `default.audio.source`
/// switches over without restart.
pub fn set_default_source(node_id: u32) -> io::Result<()> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/wpctl")
        .args(["set-default", &node_id.to_string()])
        .stdout(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/wpctl"])
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .run()
        .map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "wpctl set-default {node_id} exited with {:?}",
            output.status.code()
        )));
    }
    debug!("sources: set default → {node_id}");
    Ok(())
}

/// Read the current nonnegative volume fraction (1.0 = 100%, 3.0 = 300%).
/// Returns `None` when `wpctl` fails (node gone, no audio session).
#[must_use]
pub fn source_volume(node_id: u32) -> Option<f32> {
    read_volume_and_mute(node_id).map(|(volume, _muted)| volume)
}

fn read_volume_and_mute(node_id: u32) -> Option<(f32, bool)> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/wpctl")
        .args(["get-volume", &node_id.to_string()])
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/wpctl"])
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .run()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_volume_and_mute(&output.stdout_lossy())
}

/// Set `node_id`'s volume to `volume` (0.0..=3.0). The manual GUI slider only
/// reaches 1.5 (150%); the extra headroom above that is for the AGC to software-
/// boost a naturally weak mic. This function enforces the 3.0 product ceiling;
/// wpctl has an optional --limit, not a guaranteed global 100% ceiling. A successful
/// command is not an acoustic measurement; controllers must read the result back.
pub fn set_source_volume(node_id: u32, volume: f32) -> io::Result<()> {
    if !volume.is_finite() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "volume must be finite",
        ));
    }
    let clamped = volume.clamp(0.0, 3.0);
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/wpctl")
        .args([
            "set-volume",
            &node_id.to_string(),
            &format!("{clamped:.2}"),
            "--limit",
            "3.0",
        ])
        .stdout(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/wpctl"])
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .run()
        .map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "wpctl set-volume {node_id} exited with {:?}",
            output.status.code()
        )));
    }
    Ok(())
}

// ── Parsers ──────────────────────────────────────────────────────────

/// The graph as `pw-cli ls Node` renders it.
///
/// Shared, because four callers had built this same spec by hand and none of
/// the copies carried the reason below — so the next one had no reason not to
/// reach for `std::process::Command`.
///
/// `pw-cli ls Node` output can exceed 64 KiB on a busy graph; the shared spec
/// drains stdout/stderr concurrently, avoiding the pipe-fill deadlock.
pub(crate) fn pw_cli_ls_node() -> io::Result<String> {
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
    Ok(output.stdout_lossy())
}

fn parse_sources(stdout: &str) -> Vec<Source> {
    let mut out: Vec<Source> = Vec::new();
    let mut id: Option<u32> = None;
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut nick: Option<String> = None;
    let mut media_class: Option<String> = None;

    let mut flush = |id: &mut Option<u32>,
                     name: &mut Option<String>,
                     description: &mut Option<String>,
                     nick: &mut Option<String>,
                     media_class: &mut Option<String>| {
        let take_id = id.take();
        let take_name = name.take();
        let take_desc = description.take();
        let take_nick = nick.take();
        let take_class = media_class.take();
        let (Some(node_id), Some(node_name), Some(class)) = (take_id, take_name, take_class) else {
            return;
        };
        if class != "Audio/Source" {
            return;
        }
        if is_virtual_source(&node_name) {
            return;
        }
        let description = take_desc
            .or(take_nick)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| node_name.clone());
        out.push(Source {
            node_id,
            object_serial: None,
            core_cookie: None,
            node_name,
            description,
        });
    };

    for line in stdout.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("id ") {
            flush(
                &mut id,
                &mut name,
                &mut description,
                &mut nick,
                &mut media_class,
            );
            let token = rest.split(',').next().unwrap_or("").trim();
            id = token.parse().ok();
        } else if let Some(v) = property_value(trimmed, "node.name") {
            name = Some(v);
        } else if let Some(v) = property_value(trimmed, "node.description") {
            description = Some(v);
        } else if let Some(v) = property_value(trimmed, "node.nick") {
            nick = Some(v);
        } else if let Some(v) = property_value(trimmed, "media.class") {
            media_class = Some(v);
        }
    }
    flush(
        &mut id,
        &mut name,
        &mut description,
        &mut nick,
        &mut media_class,
    );
    out
}

/// Identify nodes the picker must hide: our two virtual filter-chain
/// endpoints and any pw-loopback monitor port.
pub(super) fn is_virtual_source(node_name: &str) -> bool {
    node_name == MIC_NODE_NAME
        || node_name == MIC_CAPTURE_NODE_NAME
        || node_name == EC_SOURCE_NAME
        || node_name == OUTPUT_NODE_NAME
        || node_name.starts_with("input.pw-loopback")
        || node_name.starts_with("output.pw-loopback")
}

fn property_value(line: &str, key: &str) -> Option<String> {
    let after_key = line.strip_prefix(key)?.trim_start();
    let after_eq = after_key.strip_prefix('=')?.trim_start();
    let inside = after_eq.strip_prefix('"')?;
    inside.strip_suffix('"').map(str::to_owned)
}

/// `pw-metadata 0 default.audio.source` prints lines like
/// `update: id:0 key:'default.audio.source' value:'{ "name": "alsa_input.…" }' type:'Spa:String:JSON'`.
/// Pull the JSON `name` field out without dragging in a json crate.
fn parse_default_source(stdout: &str) -> Option<String> {
    let needle = "\"name\":";
    let pos = stdout.find(needle)?;
    let after = &stdout[pos + needle.len()..];
    let after = after.trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    Some(after[..end].to_owned())
}

/// `wpctl get-volume <id>` prints `Volume: 1.00` (optionally followed
/// by ` [MUTED]`). Extract the floating-point value.
fn parse_volume(stdout: &str) -> Option<f32> {
    let after = stdout
        .trim()
        .strip_prefix("Volume:")?
        .split_whitespace()
        .next()?;
    let volume: f32 = after.parse().ok()?;
    (volume.is_finite() && volume >= 0.0).then_some(volume)
}

fn parse_volume_and_mute(stdout: &str) -> Option<(f32, bool)> {
    let volume = parse_volume(stdout)?;
    let mut words = stdout.trim().strip_prefix("Volume:")?.split_whitespace();
    words.next()?;
    let muted = match words.next() {
        None => false,
        Some("[MUTED]") => true,
        Some(_) => return None, // An unknown status is not proof of "unmuted".
    };
    words.next().is_none().then_some((volume, muted))
}

/// Convenience wrapper for the UI: read sources + the current default
/// id in one pass, logging any errors instead of bubbling them up.
#[must_use]
pub fn snapshot() -> (Vec<Source>, Option<u32>) {
    match super::live::graph_snapshot() {
        Ok(graph) => sources_from_graph(&graph),
        Err(error) => {
            warn!("sources: graph snapshot failed: {error}");
            (Vec::new(), None)
        }
    }
}

/// One coherent JSON snapshot replaces two unrelated process snapshots. The
/// metadata selector rejects ambiguous defaults; a node without generation
/// information can be displayed but cannot authorize an AGC write.
fn sources_from_graph(graph: &[serde_json::Value]) -> (Vec<Source>, Option<u32>) {
    let mut cores = graph
        .iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Core");
    let cookie = cores
        .next()
        .and_then(|o| o["info"]["cookie"].as_u64())
        .and_then(|n| u32::try_from(n).ok());
    let cookie = if cores.next().is_none() { cookie } else { None };
    let refs: Vec<_> = graph.iter().collect();
    let default = crate::services::echo::default_name(&refs, "default.audio.source");
    let mut sources = Vec::new();
    for node in graph
        .iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Node")
    {
        let props = &node["info"]["props"];
        let Some(name) = props["node.name"].as_str().filter(|n| !n.is_empty()) else {
            continue;
        };
        if props["media.class"] != "Audio/Source" || is_virtual_source(name) {
            continue;
        }
        let Some(id) = node["id"].as_u64().and_then(|n| u32::try_from(n).ok()) else {
            continue;
        };
        let serial = props["object.serial"]
            .as_u64()
            .map(|n| n.to_string())
            .or_else(|| {
                props["object.serial"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|n| n.to_string())
            });
        let description = props["node.description"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| props["node.nick"].as_str().filter(|s| !s.is_empty()))
            .unwrap_or(name);
        sources.push(Source {
            node_id: id,
            node_name: name.to_owned(),
            description: description.to_owned(),
            object_serial: serial,
            core_cookie: cookie,
        });
    }
    let mut matches = sources
        .iter()
        .filter(|s| Some(s.node_name.as_str()) == default.as_deref());
    let first = matches.next().map(|s| s.node_id);
    let default_id = if matches.next().is_none() {
        first
    } else {
        None
    };
    (sources, default_id)
}

/// Hear a buffer size before saving it, and give it back when you are done.
///
/// PipeWire keeps a live override beside the configured value: `clock.force-quantum` is
/// obeyed at once, by every stream, with no restart and no file — and it is forgotten when
/// the session ends. So the way to choose a number that is measured in milliseconds
/// is to listen to it while a song is playing, rather than to save, restart, judge, and
/// start again.
///
/// `0` releases the override and the configured value takes over again. That is also the
/// safety net: nothing written down can be wrong, because nothing was written down.
pub fn preview_quantum(frames: u32) -> bool {
    BigSubprocessSpec::builder()
        .program("/usr/bin/pw-metadata")
        .args([
            "-n",
            "settings",
            "0",
            "clock.force-quantum",
            &frames.to_string(),
        ])
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/pw-metadata"])
        .build()
        .run()
        .is_ok_and(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static str {
        "\tid 67, type PipeWire:Interface:Node/3\n\
         \t\tnode.name = \"alsa_input.lenovo\"\n\
         \t\tnode.description = \"Lenovo FHD Webcam Audio\"\n\
         \t\tmedia.class = \"Audio/Source\"\n\
         \tid 72, type PipeWire:Interface:Node/3\n\
         \t\tnode.name = \"alsa_input.usb-AKG\"\n\
         \t\tnode.description = \"AKG C44-USB Microphone\"\n\
         \t\tnode.nick = \"AKG C44\"\n\
         \t\tmedia.class = \"Audio/Source\"\n\
         \tid 88, type PipeWire:Interface:Node/3\n\
         \t\tnode.name = \"mic-biglinux\"\n\
         \t\tmedia.class = \"Audio/Source\"\n\
         \tid 89, type PipeWire:Interface:Node/3\n\
         \t\tnode.name = \"echo-cancel-source\"\n\
         \t\tmedia.class = \"Audio/Source\"\n\
         \tid 94, type PipeWire:Interface:Node/3\n\
         \t\tnode.name = \"input.pw-loopback-1\"\n\
         \t\tmedia.class = \"Audio/Source\"\n\
         \tid 74, type PipeWire:Interface:Node/3\n\
         \t\tnode.name = \"alsa_output.pch\"\n\
         \t\tmedia.class = \"Audio/Sink\"\n"
    }

    #[test]
    fn parse_sources_keeps_only_real_audio_sources() {
        let parsed = parse_sources(fixture());
        let names: Vec<_> = parsed.iter().map(|s| s.node_name.as_str()).collect();
        assert_eq!(names, ["alsa_input.lenovo", "alsa_input.usb-AKG"]);
    }

    #[test]
    fn parse_sources_prefers_description_over_nick_over_name() {
        let parsed = parse_sources(fixture());
        assert_eq!(parsed[0].description, "Lenovo FHD Webcam Audio");
        assert_eq!(parsed[1].description, "AKG C44-USB Microphone");
    }

    #[test]
    fn parse_sources_falls_back_to_nick_when_description_missing() {
        let stdout = "\tid 5, type PipeWire:Interface:Node\n\
                      \t\tnode.name = \"alsa_input.x\"\n\
                      \t\tnode.nick = \"Webcam\"\n\
                      \t\tmedia.class = \"Audio/Source\"\n";
        let parsed = parse_sources(stdout);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].description, "Webcam");
    }

    #[test]
    fn parse_sources_falls_back_to_node_name_when_both_missing() {
        let stdout = "\tid 5, type PipeWire:Interface:Node\n\
                      \t\tnode.name = \"alsa_input.x\"\n\
                      \t\tmedia.class = \"Audio/Source\"\n";
        let parsed = parse_sources(stdout);
        assert_eq!(parsed[0].description, "alsa_input.x");
    }

    #[test]
    fn parse_sources_skips_node_without_node_name() {
        let stdout = "\tid 5, type PipeWire:Interface:Node\n\
                      \t\tmedia.class = \"Audio/Source\"\n";
        assert!(parse_sources(stdout).is_empty());
    }

    #[test]
    fn parse_default_source_extracts_name_field() {
        let stdout = "Found \"settings\" metadata 30\nupdate: id:0 \
                      key:'default.audio.source' value:'{ \"name\": \
                      \"alsa_input.usb-AKG\" }' type:'Spa:String:JSON'\n";
        assert_eq!(
            parse_default_source(stdout),
            Some("alsa_input.usb-AKG".into())
        );
    }

    #[test]
    fn parse_default_source_handles_missing_value() {
        assert_eq!(parse_default_source("nothing here"), None);
    }

    #[test]
    fn parse_volume_extracts_float() {
        assert!((parse_volume("Volume: 0.75\n").unwrap() - 0.75).abs() < f32::EPSILON);
        assert!((parse_volume("Volume: 1.00 [MUTED]\n").unwrap() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn capture_readback_preserves_mute_and_rejects_unknown_status() {
        assert_eq!(parse_volume_and_mute("Volume: 3.00\n"), Some((3.0, false)));
        assert_eq!(parse_volume_and_mute("Volume: 0.00\n"), Some((0.0, false)));
        assert_eq!(
            parse_volume_and_mute("Volume: 1.00 [MUTED]\n"),
            Some((1.0, true))
        );
        for text in [
            "Volume: 1.00 unknown",
            "Volume: 1.00 [MUTED] extra",
            "Volume: NaN",
        ] {
            assert_eq!(parse_volume_and_mute(text), None);
        }
    }

    #[test]
    fn volume_rejects_non_finite_values_before_spawning() {
        for volume in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                set_source_volume(0, volume).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        for text in ["Volume: NaN", "Volume: inf", "Volume: -0.1", "Error: 1.0"] {
            assert_eq!(parse_volume(text), None);
        }
    }

    #[test]
    fn parse_volume_returns_none_on_garbage() {
        assert_eq!(parse_volume(""), None);
        assert_eq!(parse_volume("Volume:"), None);
    }

    /// Captured from `pw-link -l` on a live session with the chain loaded and
    /// nothing recording: the hardware feeds the chain's capture side, and the
    /// chain's source has no outgoing link at all.
    const IDLE_GRAPH: &str = "\
alsa_input.usb-AKG_C44-USB_Microphone-00.analog-stereo:capture_FL
  |-> mic-biglinux-capture:input_FL
mic-biglinux-capture:input_FL
  |<- alsa_input.usb-AKG_C44-USB_Microphone-00.analog-stereo:capture_FL";

    fn consumer(dump: &str) -> bool {
        has_capture_consumer(&dump.lines().collect::<Vec<_>>())
    }

    #[test]
    fn a_wired_chain_with_nobody_recording_is_not_a_consumer() {
        assert!(!consumer(IDLE_GRAPH));
        assert!(!consumer(""));
    }

    #[test]
    fn an_application_reading_the_chain_is_a_consumer() {
        let dump = format!("{IDLE_GRAPH}\nmic-biglinux:capture_FL\n  |-> Firefox:input_FL");
        assert!(consumer(&dump));
    }

    #[test]
    fn our_own_analyser_is_never_its_own_reason_to_keep_capturing() {
        // The AGC releases the microphone when nothing reads the chain. If its
        // meter counted as a reader, enabling it once would pin the mic awake
        // for the rest of the session.
        let dump =
            format!("{IDLE_GRAPH}\nmic-biglinux:capture_FL\n  |-> {METER_NODE_NAME}:input_FL");
        assert!(!consumer(&dump));
    }

    #[test]
    fn the_chain_feeding_itself_is_not_a_consumer() {
        let dump = format!(
            "{IDLE_GRAPH}\n{EC_SOURCE_NAME}:capture_FL\n  |-> {MIC_CAPTURE_NODE_NAME}:input_FL"
        );
        assert!(!consumer(&dump));
    }
    fn graph_fixture() -> Vec<serde_json::Value> {
        serde_json::from_value(serde_json::json!([
            {"id":0,"type":"PipeWire:Interface:Core","info":{"cookie":123}},
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},
             "metadata":[{"subject":0,"key":"default.audio.source","value":{"name":"alsa_input.test"}}]},
            {"id":7,"type":"PipeWire:Interface:Node","info":{"props":{
                "node.name":"alsa_input.test","media.class":"Audio/Source","object.serial":91}}}
        ])).unwrap()
    }

    #[test]
    fn source_snapshot_carries_generation_and_rejects_duplicate_names() {
        let mut graph = graph_fixture();
        let (sources, default) = sources_from_graph(&graph);
        assert_eq!(default, Some(7));
        assert_eq!(sources[0].object_serial.as_deref(), Some("91"));
        assert_eq!(sources[0].core_cookie, Some(123));
        let mut replacement = sources[0].clone();
        replacement.object_serial = Some("92".into());
        assert!(!sources[0].same_generation(&replacement));
        replacement = sources[0].clone();
        replacement.core_cookie = Some(124);
        assert!(!sources[0].same_generation(&replacement));
        graph.push(graph[2].clone());
        assert_eq!(sources_from_graph(&graph).1, None);
    }

    #[test]
    fn absent_generation_never_compares_as_the_same_source() {
        let mut graph = graph_fixture();
        graph[2]["info"]["props"]["object.serial"] = serde_json::Value::Null;
        let source = sources_from_graph(&graph).0.remove(0);
        assert!(!source.same_generation(&source));
    }
}
