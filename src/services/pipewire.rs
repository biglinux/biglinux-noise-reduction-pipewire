//! PipeWire graph queries and controls used by the app and CLI.

mod live;
mod module;
mod sources;
mod types;
pub mod user_tweaks;

use std::io;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

pub use live::{LiveOutcome, apply_live};
pub use module::{
    restart_aec_service, restart_mic_service, restart_output_service, start_aec_service,
    start_mic_service, start_output_service, stop_aec_service, stop_mic_service,
    stop_output_service,
};
pub use sources::{
    Source, preview_quantum, set_default_source, set_source_volume, snapshot as snapshot_sources,
    source_volume,
};
pub use types::{AppStream, StreamDirection};

/// Enumerate every `Stream/*/Audio` node in the graph via the
/// standard `pw-cli ls Node` command. Parser intentionally conservative
/// — it only emits an [`AppStream`] for entries that expose both a
/// `media.class` we can route and a numeric id header.
pub fn current_streams() -> io::Result<Vec<AppStream>> {
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
    Ok(parse_pw_cli_nodes(&output.stdout_lossy()))
}

fn parse_pw_cli_nodes(stdout: &str) -> Vec<AppStream> {
    let mut out: Vec<AppStream> = Vec::new();
    let mut id: Option<u32> = None;
    let mut direction: Option<StreamDirection> = None;
    let mut app: Option<String> = None;
    let mut media_name: Option<String> = None;

    let flush = |out: &mut Vec<AppStream>,
                 id: &mut Option<u32>,
                 direction: &mut Option<StreamDirection>,
                 app: &mut Option<String>,
                 media_name: &mut Option<String>| {
        if let (Some(nid), Some(dir)) = (*id, *direction) {
            out.push(AppStream {
                node_id: nid,
                application_name: app.take().unwrap_or_default(),
                media_name: media_name.take(),
                direction: dir,
            });
        } else {
            app.take();
            media_name.take();
        }
        *id = None;
        *direction = None;
    };

    for line in stdout.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("id ") {
            // New object; flush the previous one if it was a stream.
            flush(&mut out, &mut id, &mut direction, &mut app, &mut media_name);
            let id_token = rest.split(',').next().unwrap_or("").trim();
            id = id_token.parse().ok();
        } else if let Some(val) = property_value(trimmed, "media.class") {
            direction = StreamDirection::from_media_class(&val);
        } else if let Some(val) = property_value(trimmed, "application.name") {
            app = Some(val);
        } else if let Some(val) = property_value(trimmed, "media.name") {
            media_name = Some(val);
        }
    }
    flush(&mut out, &mut id, &mut direction, &mut app, &mut media_name);
    out
}

/// Extract the quoted value of a property line of the form
/// `key = "value"`. Returns `None` when the key does not match or the
/// value is not double-quoted.
fn property_value(line: &str, key: &str) -> Option<String> {
    let after_key = line.strip_prefix(key)?.trim_start();
    let after_eq = after_key.strip_prefix('=')?.trim_start();
    let inside = after_eq.strip_prefix('"')?;
    inside.strip_suffix('"').map(str::to_owned)
}

#[cfg(test)]
mod parser_tests {
    use super::*;

    fn fixture_with_two_streams() -> &'static str {
        // Trimmed shape of a real `pw-cli ls Node` capture: one playback
        // (Firefox) and one capture (PipeWire ALSA monitor).
        "\tid 12, type PipeWire:Interface:Node/3\n\
         \t\t  factory.id = \"9\"\n\
         \t\t  application.name = \"Firefox\"\n\
         \t\t  media.name = \"AudioStream\"\n\
         \t\t  media.class = \"Stream/Output/Audio\"\n\
         \tid 17, type PipeWire:Interface:Node/3\n\
         \t\t  application.name = \"PulseAudio Volume Control\"\n\
         \t\t  media.class = \"Stream/Input/Audio\"\n"
    }

    #[test]
    fn property_value_extracts_quoted_payload() {
        let line = "  application.name = \"Firefox\"";
        assert_eq!(
            property_value(line.trim_start(), "application.name"),
            Some("Firefox".to_owned())
        );
    }

    #[test]
    fn property_value_returns_none_on_unquoted_payload() {
        let line = "application.name = Firefox";
        assert_eq!(property_value(line, "application.name"), None);
    }

    #[test]
    fn property_value_returns_none_when_key_does_not_match() {
        let line = "media.name = \"x\"";
        assert_eq!(property_value(line, "application.name"), None);
    }

    #[test]
    fn parse_pw_cli_nodes_emits_one_entry_per_stream_node() {
        let parsed = parse_pw_cli_nodes(fixture_with_two_streams());
        assert_eq!(parsed.len(), 2);

        let firefox = &parsed[0];
        assert_eq!(firefox.node_id, 12);
        assert_eq!(firefox.application_name, "Firefox");
        assert_eq!(firefox.media_name.as_deref(), Some("AudioStream"));
        assert_eq!(firefox.direction, StreamDirection::Playback);

        let pavu = &parsed[1];
        assert_eq!(pavu.node_id, 17);
        assert_eq!(pavu.direction, StreamDirection::Capture);
        assert!(pavu.media_name.is_none());
    }

    #[test]
    fn parse_pw_cli_nodes_skips_objects_without_stream_media_class() {
        // A bare hardware device (no `Stream/*` media.class) must not be
        // exposed as an `AppStream` — the routing UI would try to push
        // metadata against a non-stream node.
        let stdout = "\tid 5, type PipeWire:Interface:Node/3\n\
                      \t\t  application.name = \"alsa\"\n\
                      \t\t  media.class = \"Audio/Sink\"\n";
        assert!(parse_pw_cli_nodes(stdout).is_empty());
    }

    #[test]
    fn parse_pw_cli_nodes_handles_empty_input() {
        assert!(parse_pw_cli_nodes("").is_empty());
    }

    #[test]
    fn parse_pw_cli_nodes_recovers_after_garbled_id_line() {
        // A non-numeric `id` token must not poison the next valid object.
        let stdout = "\tid not-a-number, type X\n\
                      \tid 21, type PipeWire:Interface:Node/3\n\
                      \t\t  application.name = \"OK\"\n\
                      \t\t  media.class = \"Stream/Output/Audio\"\n";
        let parsed = parse_pw_cli_nodes(stdout);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].node_id, 21);
    }
}
