//! Thread-safe handle over a running PipeWire main loop.
//!
//! The PipeWire client types are `!Send`, so we can't hand them to the UI
//! thread directly. [`PwService`] hides them inside a dedicated worker
//! thread and exposes only plain-data channels:
//!
//! - `events()` — an [`async_channel::Receiver`] that yields graph updates.
//! - `send(Command)` — posts a command into the worker via
//!   [`pipewire::channel`], which wakes the PW main loop immediately.
//!
//! The service is started once per process and dropped on shutdown. Drop
//! sends `Command::Shutdown` synchronously and waits for the worker to
//! exit so the PipeWire state is fully torn down before the process ends.

mod default_sink;
mod live;
mod module;
mod sources;
mod types;
mod worker;

use std::io;
use std::process::Stdio;
use std::thread::{self, JoinHandle};

use log::{debug, warn};
use pipewire as pw;

pub use default_sink::default_sink_name;
pub use live::{apply_live, LiveOutcome};
pub use module::{
    reload_mic_chain, restart_echo_cancel_service, restart_filter_chain_service,
    restart_output_service, start_echo_cancel_service, start_output_service,
    stop_echo_cancel_service, stop_filter_chain_service, stop_output_service,
};
pub use sources::{
    set_default_source, set_source_volume, snapshot as snapshot_sources, source_volume, Source,
};
pub use types::{AppStream, Command, Event, StreamDirection};

/// Running PipeWire service. See module docs.
pub struct PwService {
    command_tx: pw::channel::Sender<Command>,
    events_rx: async_channel::Receiver<Event>,
    worker: Option<JoinHandle<Result<(), pw::Error>>>,
}

impl PwService {
    /// Spawn the PipeWire worker thread and connect to the daemon.
    ///
    /// Returns as soon as the worker thread is spawned — initialisation of
    /// `MainLoop` / `Context` / `Core` happens inside the thread, so any
    /// failure there surfaces via [`Event::Fatal`] on the events channel
    /// rather than here. This keeps the call site unconditional: the
    /// service can be created even without a running daemon (useful for
    /// tests and for letting the UI come up before the user logs in).
    #[must_use]
    pub fn start() -> Self {
        let (command_tx, command_rx) = pw::channel::channel::<Command>();
        let (events_tx, events_rx) = async_channel::unbounded::<Event>();

        let worker = thread::Builder::new()
            .name("biglinux-microphone/pipewire".into())
            .spawn(move || worker::run(command_rx, events_tx))
            .expect("OS refused to spawn pipewire worker thread");

        Self {
            command_tx,
            events_rx,
            worker: Some(worker),
        }
    }

    /// Snapshot of every stream currently visible in the graph.
    ///
    /// Implemented as a one-shot `pw-cli ls Node` parse rather than a
    /// long-running listener so it never competes with the UI's own
    /// `events()` subscription for messages on the shared channel.
    /// Called at most once per settings change, so the ~30 ms parse
    /// cost is not on the live slider path.
    #[must_use]
    pub fn current_streams(&self) -> Vec<AppStream> {
        query_streams_via_pw_cli().unwrap_or_else(|e| {
            warn!("pipewire: pw-cli enumeration failed: {e}");
            Vec::new()
        })
    }

    /// Subscribe to graph events. Cloning the receiver is cheap — every
    /// subscriber sees every event exactly once, so duplicate subscribers
    /// compete for messages. The UI keeps a single receiver.
    #[must_use]
    pub fn events(&self) -> async_channel::Receiver<Event> {
        self.events_rx.clone()
    }

    /// Post a command into the worker. On failure the command is returned
    /// back; this only happens when the worker has already exited.
    pub fn send(&self, cmd: Command) -> Result<(), Command> {
        self.command_tx.send(cmd)
    }

    /// Signal the worker to exit and wait for it. Safe to call more than
    /// once; subsequent calls are no-ops.
    pub fn shutdown(mut self) {
        self.shutdown_internal();
    }

    fn shutdown_internal(&mut self) {
        if self.worker.is_none() {
            return;
        }
        if let Err(e) = self.command_tx.send(Command::Shutdown) {
            warn!("pipewire: shutdown send failed: {e:?} (worker likely already exited)");
        }
        if let Some(handle) = self.worker.take() {
            match handle.join() {
                Ok(Ok(())) => debug!("pipewire worker exited cleanly"),
                Ok(Err(e)) => warn!("pipewire worker exited with error: {e}"),
                Err(_) => warn!("pipewire worker panicked"),
            }
        }
    }
}

/// Enumerate every `Stream/*/Audio` node in the graph via the
/// standard `pw-cli ls Node` command. Parser intentionally conservative
/// — it only emits an [`AppStream`] for entries that expose both a
/// `media.class` we can route and a numeric id header.
fn query_streams_via_pw_cli() -> io::Result<Vec<AppStream>> {
    let output = std::process::Command::new("pw-cli")
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
    Ok(parse_pw_cli_nodes(&String::from_utf8_lossy(&output.stdout)))
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

impl Drop for PwService {
    fn drop(&mut self) {
        self.shutdown_internal();
    }
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
    fn parse_pw_cli_nodes_emits_one_entry_per_stream_object() {
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
