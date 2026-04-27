//! PipeWire main-loop worker running on a dedicated thread.
//!
//! The function [`run`] owns every `!Send` PipeWire handle for the lifetime
//! of the loop. It reads commands from a [`pipewire::channel`] receiver
//! (so the PW main loop wakes up on them) and writes events out through a
//! plain [`async_channel`] sender, which the UI thread reads asynchronously.
//!
//! Errors that happen during initialisation are returned synchronously so
//! [`super::PwService::start`] can surface them before the thread becomes
//! the authoritative owner of the state.

use std::sync::{Arc, Mutex};

use async_channel::Sender as AsyncSender;
use log::{debug, info, warn};
use pipewire as pw;
use pw::types::ObjectType;

use super::types::{AppStream, Command, Event, StreamDirection};

/// Entry point of the worker thread. Blocks until the main loop quits,
/// either because a `Shutdown` command arrived or because PipeWire itself
/// tore down the connection.
pub fn run(
    cmd_rx: pw::channel::Receiver<Command>,
    events_tx: AsyncSender<Event>,
) -> Result<(), pw::Error> {
    pw::init();
    let result = run_inner(cmd_rx, events_tx.clone());
    // SAFETY: `pw::deinit` is safe to call after every `pw::init` on the
    // thread that initialised. We are that thread.
    unsafe {
        pw::deinit();
    }
    if let Err(ref e) = result {
        let _ = events_tx.try_send(Event::Fatal(format!("pipewire: {e}")));
    }
    result
}

fn run_inner(
    cmd_rx: pw::channel::Receiver<Command>,
    events_tx: AsyncSender<Event>,
) -> Result<(), pw::Error> {
    let main_loop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;

    // Track which streams we've already reported so `StreamDisappeared`
    // doesn't fire for objects we never announced in the first place
    // (ports, metadata, devices, …).
    let known_streams = Arc::new(Mutex::new(std::collections::HashSet::<u32>::new()));

    let tx_global = events_tx.clone();
    let tx_remove = events_tx;
    let known_global = Arc::clone(&known_streams);
    let known_remove = Arc::clone(&known_streams);
    let _registry_listener = registry
        .add_listener_local()
        .global(move |obj| {
            if obj.type_ != ObjectType::Node {
                return;
            }
            let Some(props) = &obj.props else {
                return;
            };
            let Some(stream) = parse_app_stream(obj.id, props) else {
                return;
            };
            if let Ok(mut known) = known_global.lock() {
                known.insert(stream.node_id);
            }
            if let Err(e) = tx_global.try_send(Event::StreamAppeared(stream)) {
                warn!("events channel full/closed: {e}");
            }
        })
        .global_remove(move |id| {
            let known_now = {
                let Ok(mut known) = known_remove.lock() else {
                    return;
                };
                known.remove(&id)
            };
            if known_now {
                let _ = tx_remove.try_send(Event::StreamDisappeared { node_id: id });
            }
        })
        .register();

    // Bridge UI commands into the main loop. `attach()` hooks the channel
    // into the same event source PipeWire already polls, so commands are
    // processed in-thread without busy waiting.
    let main_weak = main_loop.downgrade();
    let _cmd_attach = cmd_rx.attach(main_loop.loop_(), move |cmd| match cmd {
        Command::Shutdown => {
            debug!("pipewire worker: shutdown requested");
            if let Some(loop_) = main_weak.upgrade() {
                loop_.quit();
            }
        }
    });

    info!("pipewire worker: main loop running");
    main_loop.run();
    info!("pipewire worker: main loop exited");
    Ok(())
}

/// Translate a PipeWire global's property dictionary into an
/// [`AppStream`], or `None` if it does not describe a routable audio
/// application stream.
fn parse_app_stream(node_id: u32, props: &pw::spa::utils::dict::DictRef) -> Option<AppStream> {
    let media_class = props.get("media.class")?;
    let direction = StreamDirection::from_media_class(media_class)?;

    // `application.name` is the stable identifier WirePlumber rules match
    // on. Fall back to an empty string when missing so the downstream
    // `is_routable_output` filter can reject it without panicking.
    let application_name = props.get("application.name").unwrap_or("").to_owned();
    let media_name = props.get("media.name").map(str::to_owned);

    Some(AppStream {
        node_id,
        application_name,
        media_name,
        direction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // These are the pure-logic paths from the closures. Anything touching
    // the PipeWire daemon is exercised by the `list-apps` integration run
    // in the CLI layer — it needs a live session bus, which CI doesn't
    // have.

    #[test]
    fn directions_are_symmetric_with_classifier() {
        assert_eq!(
            StreamDirection::from_media_class("Stream/Output/Audio"),
            Some(StreamDirection::Playback)
        );
        assert_eq!(
            StreamDirection::from_media_class("Stream/Input/Audio"),
            Some(StreamDirection::Capture)
        );
    }

    #[test]
    fn app_stream_rejects_unnamed_playback_from_routing() {
        let s = AppStream {
            node_id: 42,
            application_name: String::new(),
            media_name: None,
            direction: StreamDirection::Playback,
        };
        assert!(!s.is_routable_output());
    }
}
