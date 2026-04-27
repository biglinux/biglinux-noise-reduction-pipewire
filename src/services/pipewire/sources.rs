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
use std::process::{Command, Stdio};

use log::{debug, warn};

use crate::pipeline::{EC_SOURCE_NAME, MIC_CAPTURE_NODE_NAME, MIC_NODE_NAME, OUTPUT_NODE_NAME};

/// One hardware capture source visible in the PipeWire graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub node_id: u32,
    /// Stable internal identifier (`alsa_input.usb-…`). Used to match
    /// the value of `default.audio.source` returned by `pw-metadata`.
    pub node_name: String,
    /// Human-friendly name (`AKG C44-USB Microphone …`). Falls back to
    /// `node_name` when no description is published.
    pub description: String,
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
    let output = Command::new("pw-metadata")
        .args(["0", "default.audio.source"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_default_source(&String::from_utf8_lossy(&output.stdout))
}

/// Promote `node_id` to the system default source. WirePlumber rules
/// pick this up live — every app following `default.audio.source`
/// switches over without restart.
pub fn set_default_source(node_id: u32) -> io::Result<()> {
    let status = Command::new("wpctl")
        .args(["set-default", &node_id.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "wpctl set-default {node_id} exited with {status}"
        )));
    }
    debug!("sources: set default → {node_id}");
    Ok(())
}

/// Read the current volume of `node_id` as a 0.0..=1.5 fraction.
/// Returns `None` when `wpctl` fails (node gone, no audio session).
#[must_use]
pub fn source_volume(node_id: u32) -> Option<f32> {
    let output = Command::new("wpctl")
        .args(["get-volume", &node_id.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_volume(&String::from_utf8_lossy(&output.stdout))
}

/// Set `node_id`'s volume to `volume` (0.0..=1.5). `wpctl` clamps to
/// its own configured ceiling, so values above the system maximum are
/// silently capped — no error returned.
pub fn set_source_volume(node_id: u32, volume: f32) -> io::Result<()> {
    let clamped = volume.clamp(0.0, 1.5);
    let status = Command::new("wpctl")
        .args(["set-volume", &node_id.to_string(), &format!("{clamped:.2}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "wpctl set-volume {node_id} exited with {status}"
        )));
    }
    Ok(())
}

// ── Parsers ──────────────────────────────────────────────────────────

fn pw_cli_ls_node() -> io::Result<String> {
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
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
fn is_virtual_source(node_name: &str) -> bool {
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
    let after = stdout.split_whitespace().nth(1)?;
    after.parse().ok()
}

/// Convenience wrapper for the UI: read sources + the current default
/// id in one pass, logging any errors instead of bubbling them up.
#[must_use]
pub fn snapshot() -> (Vec<Source>, Option<u32>) {
    let sources = list_sources().unwrap_or_else(|e| {
        warn!("sources: enumeration failed: {e}");
        Vec::new()
    });
    let default_name = default_source_name();
    let default_id = default_name
        .as_deref()
        .and_then(|n| sources.iter().find(|s| s.node_name == n).map(|s| s.node_id));
    (sources, default_id)
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
    fn parse_sources_skips_object_without_node_name() {
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
    fn parse_volume_returns_none_on_garbage() {
        assert_eq!(parse_volume(""), None);
        assert_eq!(parse_volume("Volume:"), None);
    }
}
