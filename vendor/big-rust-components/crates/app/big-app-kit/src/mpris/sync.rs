use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gtk::{gio, glib};

use super::metadata::build_metadata;
use super::signals::{emit_launcher_entry, emit_properties_changed};
use super::types::{MprisState, PlayerState, StateCallback, playback_status};

#[derive(Debug, Clone, PartialEq)]
struct LauncherEntryUpdate {
    desktop_uri: String,
    is_visible: bool,
    progress: f64,
}

struct SyncStateUpdate {
    changed_properties: Option<glib::Variant>,
    launcher_entry: Option<LauncherEntryUpdate>,
}

fn calculate_mpris_sync_update(
    state: PlayerState,
    previous_state: &mut MprisState,
    is_taskbar_progress_enabled: bool,
    desktop_uri: &str,
) -> SyncStateUpdate {
    let status = playback_status(state.is_paused).to_string();
    let volume = state.volume_pct / 100.0;
    let duration_us = (state.duration_secs * 1_000_000.0) as i64;

    let dict = glib::VariantDict::new(None);
    let mut changed = false;

    if status != previous_state.status {
        dict.insert("PlaybackStatus", &status);
        previous_state.status.clone_from(&status);
        changed = true;
    }
    if (volume - previous_state.volume).abs() > 0.005 {
        dict.insert("Volume", volume);
        previous_state.volume = volume;
        changed = true;
    }
    if state.title != previous_state.title
        || state.artist != previous_state.artist
        || state.album != previous_state.album
        || duration_us != previous_state.duration_us
        || state.art_url != previous_state.art_url
    {
        let art = if state.art_url.is_empty() {
            None
        } else {
            Some(state.art_url.as_str())
        };
        dict.insert_value(
            "Metadata",
            &build_metadata(
                &state.title,
                &state.artist,
                &state.album,
                state.duration_secs,
                art,
            ),
        );
        previous_state.title = state.title;
        previous_state.artist = state.artist;
        previous_state.album = state.album;
        previous_state.duration_us = duration_us;
        previous_state.art_url = state.art_url;
        changed = true;
    }
    if state.loop_status != previous_state.loop_status {
        dict.insert("LoopStatus", &state.loop_status);
        previous_state.loop_status = state.loop_status;
        changed = true;
    }
    if state.shuffle != previous_state.shuffle {
        dict.insert("Shuffle", state.shuffle);
        previous_state.shuffle = state.shuffle;
        changed = true;
    }

    let changed_properties = changed.then(|| dict.end());

    let launcher_entry = if is_taskbar_progress_enabled {
        let wants_progress = !state.is_paused && state.duration_secs > 0.0;
        if wants_progress {
            previous_state.progress_visible = true;
            Some(LauncherEntryUpdate {
                desktop_uri: desktop_uri.to_string(),
                is_visible: true,
                progress: (state.position_secs / state.duration_secs).clamp(0.0, 1.0),
            })
        } else if previous_state.progress_visible {
            previous_state.progress_visible = false;
            Some(LauncherEntryUpdate {
                desktop_uri: desktop_uri.to_string(),
                is_visible: false,
                progress: 0.0,
            })
        } else {
            None
        }
    } else if previous_state.progress_visible {
        previous_state.progress_visible = false;
        Some(LauncherEntryUpdate {
            desktop_uri: desktop_uri.to_string(),
            is_visible: false,
            progress: 0.0,
        })
    } else {
        None
    };

    SyncStateUpdate {
        changed_properties,
        launcher_entry,
    }
}

pub(super) fn sync_state(
    connection: &gio::DBusConnection,
    state_cb: &StateCallback,
    prev: &Arc<Mutex<MprisState>>,
    taskbar_flag: &AtomicBool,
    desktop_uri: &str,
) {
    let state = state_cb();
    let mut previous_state = prev
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let update = calculate_mpris_sync_update(
        state,
        &mut previous_state,
        taskbar_flag.load(Ordering::Relaxed),
        desktop_uri,
    );

    if let Some(changed_properties) = update.changed_properties {
        emit_properties_changed(connection, changed_properties);
    }
    if let Some(launcher_entry) = update.launcher_entry {
        emit_launcher_entry(
            connection,
            &launcher_entry.desktop_uri,
            launcher_entry.is_visible,
            launcher_entry.progress,
        );
    }
}

#[cfg(test)]
mod tests {
    use gtk::glib::VariantDict;
    use gtk::glib::variant::FromVariant;

    use super::{LauncherEntryUpdate, calculate_mpris_sync_update};
    use crate::mpris::types::{MprisState, PlayerState, playback_status};

    fn playing_state() -> PlayerState {
        PlayerState {
            is_paused: false,
            position_secs: 25.0,
            duration_secs: 100.0,
            volume_pct: 50.0,
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            loop_status: "Track".into(),
            shuffle: true,
            art_url: "file:///tmp/art.png".into(),
            ..PlayerState::default()
        }
    }

    fn previous_state_matching(player_state: &PlayerState) -> MprisState {
        MprisState {
            status: playback_status(player_state.is_paused).into(),
            title: player_state.title.clone(),
            artist: player_state.artist.clone(),
            album: player_state.album.clone(),
            volume: player_state.volume_pct / 100.0,
            duration_us: (player_state.duration_secs * 1_000_000.0) as i64,
            art_url: player_state.art_url.clone(),
            loop_status: player_state.loop_status.clone(),
            shuffle: player_state.shuffle,
            progress_visible: false,
        }
    }

    fn changed_property_dict(update: &super::SyncStateUpdate) -> VariantDict {
        let changed_properties = update
            .changed_properties
            .as_ref()
            .expect("changed properties");
        VariantDict::from_variant(changed_properties).expect("changed property dict")
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn calculate_mpris_sync_update_records_initial_properties_and_progress() {
        let mut previous_state = MprisState::default();

        let update = calculate_mpris_sync_update(
            playing_state(),
            &mut previous_state,
            true,
            "application://br.com.biglinux.VideoPlayer.desktop",
        );

        let changed_properties = changed_property_dict(&update);
        assert_eq!(
            changed_properties
                .lookup::<String>("PlaybackStatus")
                .expect("playback status")
                .as_deref(),
            Some("Playing")
        );
        assert_eq!(
            changed_properties.lookup::<f64>("Volume").expect("volume"),
            Some(0.5)
        );
        assert_eq!(
            changed_properties
                .lookup::<String>("LoopStatus")
                .expect("loop status")
                .as_deref(),
            Some("Track")
        );
        assert_eq!(
            changed_properties
                .lookup::<bool>("Shuffle")
                .expect("shuffle"),
            Some(true)
        );
        let metadata = changed_properties
            .lookup_value("Metadata", None)
            .expect("metadata");
        let metadata_dict = VariantDict::from_variant(&metadata).expect("metadata dict");
        assert_eq!(
            metadata_dict
                .lookup::<String>("xesam:title")
                .expect("title")
                .as_deref(),
            Some("Song")
        );
        assert_eq!(
            metadata_dict.lookup::<i64>("mpris:length").expect("length"),
            Some(100_000_000)
        );
        assert_eq!(
            update.launcher_entry,
            Some(LauncherEntryUpdate {
                desktop_uri: "application://br.com.biglinux.VideoPlayer.desktop".into(),
                is_visible: true,
                progress: 0.25,
            })
        );
        assert_eq!(previous_state.status, "Playing");
        assert_eq!(previous_state.volume, 0.5);
        assert_eq!(previous_state.duration_us, 100_000_000);
        assert!(previous_state.progress_visible);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn calculate_mpris_sync_update_ignores_small_volume_drift() {
        let baseline_state = playing_state();
        let mut current_state = baseline_state.clone();
        current_state.volume_pct = 50.4;
        let mut previous_state = previous_state_matching(&baseline_state);

        let update = calculate_mpris_sync_update(
            current_state,
            &mut previous_state,
            false,
            "application://br.com.biglinux.VideoPlayer.desktop",
        );

        assert!(update.changed_properties.is_none());
        assert_eq!(previous_state.volume, 0.5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn calculate_mpris_sync_update_ignores_exact_volume_threshold() {
        let mut current_state = playing_state();
        current_state.volume_pct = 0.5;
        let mut previous_state = previous_state_matching(&current_state);
        previous_state.volume = 0.0;

        let update = calculate_mpris_sync_update(
            current_state,
            &mut previous_state,
            false,
            "application://br.com.biglinux.VideoPlayer.desktop",
        );

        assert!(update.changed_properties.is_none());
        assert_eq!(previous_state.volume, 0.0);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn calculate_mpris_sync_update_detects_each_metadata_field_change() {
        let metadata_changes: [(&str, fn(&mut PlayerState)); 5] = [
            ("title", |state| state.title = "New Song".into()),
            ("artist", |state| state.artist = "New Artist".into()),
            ("album", |state| state.album = "New Album".into()),
            ("duration", |state| state.duration_secs = 120.0),
            ("art", |state| {
                state.art_url = "file:///tmp/new-art.png".into()
            }),
        ];

        for (field_name, change_state) in metadata_changes {
            let baseline_state = playing_state();
            let mut current_state = baseline_state.clone();
            change_state(&mut current_state);
            let mut previous_state = previous_state_matching(&baseline_state);

            let update = calculate_mpris_sync_update(
                current_state,
                &mut previous_state,
                false,
                "application://br.com.biglinux.VideoPlayer.desktop",
            );

            let changed_properties = changed_property_dict(&update);
            assert!(
                changed_properties.lookup_value("Metadata", None).is_some(),
                "{field_name} change should publish metadata"
            );
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn calculate_mpris_sync_update_hides_progress_when_paused_zero_duration_or_disabled() {
        let mut paused_state = playing_state();
        paused_state.is_paused = true;
        let mut previous_state = previous_state_matching(&playing_state());
        previous_state.progress_visible = true;
        let paused_update = calculate_mpris_sync_update(
            paused_state,
            &mut previous_state,
            true,
            "application://br.com.biglinux.VideoPlayer.desktop",
        );
        assert_eq!(
            paused_update.launcher_entry,
            Some(LauncherEntryUpdate {
                desktop_uri: "application://br.com.biglinux.VideoPlayer.desktop".into(),
                is_visible: false,
                progress: 0.0,
            })
        );
        assert!(!previous_state.progress_visible);

        let mut zero_duration_state = playing_state();
        zero_duration_state.duration_secs = 0.0;
        let mut previous_state = previous_state_matching(&playing_state());
        previous_state.progress_visible = true;
        let zero_duration_update = calculate_mpris_sync_update(
            zero_duration_state,
            &mut previous_state,
            true,
            "application://br.com.biglinux.VideoPlayer.desktop",
        );
        assert_eq!(
            zero_duration_update
                .launcher_entry
                .as_ref()
                .map(|update| (update.is_visible, update.progress)),
            Some((false, 0.0))
        );

        let mut previous_state = previous_state_matching(&playing_state());
        previous_state.progress_visible = true;
        let disabled_update = calculate_mpris_sync_update(
            playing_state(),
            &mut previous_state,
            false,
            "application://br.com.biglinux.VideoPlayer.desktop",
        );
        assert_eq!(
            disabled_update
                .launcher_entry
                .as_ref()
                .map(|update| (update.is_visible, update.progress)),
            Some((false, 0.0))
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn calculate_mpris_sync_update_clamps_progress_bounds() {
        let mut before_start_state = playing_state();
        before_start_state.position_secs = -10.0;
        let mut previous_state = previous_state_matching(&playing_state());
        let before_start_update = calculate_mpris_sync_update(
            before_start_state,
            &mut previous_state,
            true,
            "application://br.com.biglinux.VideoPlayer.desktop",
        );
        assert_eq!(
            before_start_update
                .launcher_entry
                .as_ref()
                .map(|update| update.progress),
            Some(0.0)
        );

        let mut after_end_state = playing_state();
        after_end_state.position_secs = 150.0;
        let mut previous_state = previous_state_matching(&playing_state());
        let after_end_update = calculate_mpris_sync_update(
            after_end_state,
            &mut previous_state,
            true,
            "application://br.com.biglinux.VideoPlayer.desktop",
        );
        assert_eq!(
            after_end_update
                .launcher_entry
                .as_ref()
                .map(|update| update.progress),
            Some(1.0)
        );
    }
}
