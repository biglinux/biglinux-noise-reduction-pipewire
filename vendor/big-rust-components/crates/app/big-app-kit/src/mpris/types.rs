use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub(super) struct MprisState {
    pub(super) status: String,
    pub(super) title: String,
    pub(super) artist: String,
    pub(super) album: String,
    pub(super) volume: f64,
    pub(super) duration_us: i64,
    pub(super) art_url: String,
    pub(super) loop_status: String,
    pub(super) shuffle: bool,
    pub(super) progress_visible: bool,
}

impl Default for MprisState {
    fn default() -> Self {
        Self {
            status: String::new(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            volume: -1.0,
            duration_us: 0,
            art_url: String::new(),
            loop_status: "None".into(),
            shuffle: false,
            progress_visible: false,
        }
    }
}

/// Callback for dispatching MPRIS commands to the Window.
pub type CommandCallback = Box<dyn Fn(&str, Option<i64>) + Send + Sync + 'static>;

/// Snapshot of player state returned by the state callback.
#[derive(Debug, Clone)]
pub struct PlayerState {
    /// `true` when the engine is paused; mapped to MPRIS
    /// `PlaybackStatus`.
    pub is_paused: bool,
    /// Playback position in seconds since track start.
    pub position_secs: f64,
    /// Track duration in seconds; 0 for live streams.
    pub duration_secs: f64,
    /// Volume expressed as a percentage (`0.0..=100.0`).
    pub volume_pct: f64,
    /// Track title for MPRIS `Metadata`.
    pub title: String,
    /// Track artist for MPRIS `Metadata`.
    pub artist: String,
    /// Album name for MPRIS `Metadata`.
    pub album: String,
    /// `true` when "next track" is available; mapped to MPRIS
    /// `CanGoNext`.
    pub can_next: bool,
    /// `true` when "previous track" is available; mapped to MPRIS
    /// `CanGoPrevious`.
    pub can_prev: bool,
    /// MPRIS `LoopStatus` token (`"None"`, `"Track"`, `"Playlist"`).
    pub loop_status: String,
    /// `true` when shuffle is enabled; mapped to MPRIS `Shuffle`.
    pub shuffle: bool,
    /// URL of the album/cover art exposed via MPRIS `Metadata`.
    pub art_url: String,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            is_paused: true,
            position_secs: 0.0,
            duration_secs: 0.0,
            volume_pct: 0.0,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            can_next: false,
            can_prev: false,
            loop_status: "None".into(),
            shuffle: false,
            art_url: String::new(),
        }
    }
}

/// Callback that returns a snapshot of the current player state.
pub type StateCallback = Box<dyn Fn() -> PlayerState + Send + Sync + 'static>;

pub(super) fn playback_status(paused: bool) -> &'static str {
    if paused { "Paused" } else { "Playing" }
}

pub(super) fn previous_state() -> Arc<Mutex<MprisState>> {
    Arc::new(Mutex::new(MprisState::default()))
}

pub(super) fn taskbar_flag(enabled: bool) -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(enabled))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::{playback_status, previous_state, taskbar_flag};

    #[test]
    fn playback_status_maps_paused_and_playing_tokens() {
        assert_eq!(playback_status(true), "Paused");
        assert_eq!(playback_status(false), "Playing");
    }

    #[test]
    fn previous_state_starts_with_empty_mpris_snapshot() {
        let previous_state = previous_state();
        let snapshot = previous_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        assert_eq!(snapshot.status, "");
        assert_eq!(snapshot.title, "");
        assert_eq!(snapshot.artist, "");
        assert_eq!(snapshot.album, "");
        assert_eq!(snapshot.volume, -1.0);
        assert_eq!(snapshot.duration_us, 0);
        assert_eq!(snapshot.art_url, "");
        assert_eq!(snapshot.loop_status, "None");
        assert!(!snapshot.shuffle);
        assert!(!snapshot.progress_visible);
    }

    #[test]
    fn taskbar_flag_stores_initial_visibility() {
        assert!(taskbar_flag(true).load(Ordering::Relaxed));
        assert!(!taskbar_flag(false).load(Ordering::Relaxed));
    }
}
