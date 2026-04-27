//! Shared types exchanged between the PipeWire worker thread and any
//! consumer (UI, CLI, tests).
//!
//! Everything in here is plain data: `Send + Sync`, no pipewire-rs types
//! leak across the thread boundary.

/// Snapshot of an application stream discovered in the PipeWire graph.
///
/// Streams are identified by their PipeWire node id, which is the same
/// value `pw-metadata` and `pw-link` accept. `application_name` is the
/// value advertised under the `application.name` property — the key used
/// by WirePlumber rules to decide whether to route the stream through the
/// output filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStream {
    pub node_id: u32,
    pub application_name: String,
    /// `media.name` — the title shown by mixers. Optional because not
    /// every stream provides one.
    pub media_name: Option<String>,
    pub direction: StreamDirection,
}

impl AppStream {
    /// Filter predicate for the output-routing UI: only playback streams
    /// with a readable application name are actionable.
    #[must_use]
    pub fn is_routable_output(&self) -> bool {
        matches!(self.direction, StreamDirection::Playback) && !self.application_name.is_empty()
    }
}

/// Direction of a [`AppStream`], mirroring the `media.class` of the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamDirection {
    /// Playback (the app is producing audio that will be routed to a sink).
    /// `media.class = Stream/Output/Audio`.
    Playback,
    /// Capture (the app is recording from a source).
    /// `media.class = Stream/Input/Audio`.
    Capture,
}

impl StreamDirection {
    /// Parse from the `media.class` property string. Non-stream classes
    /// (devices, virtual sinks, sources themselves) return `None`.
    #[must_use]
    pub fn from_media_class(media_class: &str) -> Option<Self> {
        match media_class {
            "Stream/Output/Audio" => Some(Self::Playback),
            "Stream/Input/Audio" => Some(Self::Capture),
            _ => None,
        }
    }
}

/// Events the worker emits as the graph changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A new stream became visible in the graph.
    StreamAppeared(AppStream),
    /// A previously-announced stream has been removed.
    StreamDisappeared { node_id: u32 },
    /// The worker thread encountered a fatal error and shut the main loop
    /// down. Consumers should treat the service as dead after this.
    Fatal(String),
}

/// Commands the consumer sends *into* the worker thread.
#[derive(Debug, Clone)]
pub enum Command {
    /// Gracefully shut down the main loop. The worker thread exits after
    /// emitting any pending events.
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_direction_parses_playback_and_capture() {
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
    fn stream_direction_rejects_non_stream_classes() {
        assert_eq!(StreamDirection::from_media_class("Audio/Sink"), None);
        assert_eq!(StreamDirection::from_media_class("Audio/Source"), None);
        assert_eq!(StreamDirection::from_media_class(""), None);
    }

    #[test]
    fn is_routable_output_requires_playback_and_name() {
        let base = AppStream {
            node_id: 1,
            application_name: "Firefox".into(),
            media_name: None,
            direction: StreamDirection::Playback,
        };
        assert!(base.is_routable_output());

        let capture = AppStream {
            direction: StreamDirection::Capture,
            ..base.clone()
        };
        assert!(!capture.is_routable_output());

        let unnamed = AppStream {
            application_name: String::new(),
            ..base
        };
        assert!(!unnamed.is_routable_output());
    }
}
