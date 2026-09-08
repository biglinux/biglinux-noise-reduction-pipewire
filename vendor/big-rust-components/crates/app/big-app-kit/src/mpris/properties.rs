use glib::Variant;
use glib::variant::ToVariant;
use gtk::{gio, glib};

use super::metadata::build_metadata;
use super::types::{CommandCallback, StateCallback, playback_status};
use super::{MPRIS_PATH, MPRIS_PLAYER_IFACE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlayerMethodDispatch {
    command: &'static str,
    argument: Option<i64>,
    seeked_position: Option<i64>,
}

fn variant_child_i64(params: &Variant, child_index: usize) -> Option<i64> {
    (params.n_children() > child_index)
        .then(|| params.child_value(child_index).get::<i64>())
        .flatten()
}

fn player_method_dispatch(method: &str, params: &Variant) -> Option<PlayerMethodDispatch> {
    match method {
        "PlayPause" => Some(PlayerMethodDispatch {
            command: "play-pause",
            argument: None,
            seeked_position: None,
        }),
        "Play" => Some(PlayerMethodDispatch {
            command: "play",
            argument: None,
            seeked_position: None,
        }),
        "Pause" => Some(PlayerMethodDispatch {
            command: "pause",
            argument: None,
            seeked_position: None,
        }),
        "Stop" => Some(PlayerMethodDispatch {
            command: "stop",
            argument: None,
            seeked_position: None,
        }),
        "Next" => Some(PlayerMethodDispatch {
            command: "next",
            argument: None,
            seeked_position: None,
        }),
        "Previous" => Some(PlayerMethodDispatch {
            command: "previous",
            argument: None,
            seeked_position: None,
        }),
        "Seek" => variant_child_i64(params, 0).map(|offset| PlayerMethodDispatch {
            command: "seek",
            argument: Some(offset),
            seeked_position: None,
        }),
        "SetPosition" => variant_child_i64(params, 1).map(|position| PlayerMethodDispatch {
            command: "set-position",
            argument: Some(position),
            seeked_position: Some(position),
        }),
        _ => None,
    }
}

pub(super) fn handle_player_method(
    method: &str,
    params: Variant,
    invocation: gio::DBusMethodInvocation,
    command_cb: &CommandCallback,
    connection: &gio::DBusConnection,
) {
    if let Some(dispatch) = player_method_dispatch(method, &params) {
        command_cb(dispatch.command, dispatch.argument);
        if let Some(position) = dispatch.seeked_position {
            let _ = connection.emit_signal(
                None::<&str>,
                MPRIS_PATH,
                MPRIS_PLAYER_IFACE,
                "Seeked",
                Some(&(position,).to_variant()),
            );
        }
    }
    invocation.return_value(None);
}

fn identity_for_desktop_id(desktop_id: &str) -> &'static str {
    if desktop_id.contains("AudioPlayer") {
        "Big Audio Player"
    } else {
        "Big Media Player"
    }
}

pub(super) fn get_root_property(prop: &str, desktop_id: &str) -> Option<Variant> {
    match prop {
        "Identity" => Some(identity_for_desktop_id(desktop_id).to_variant()),
        "DesktopEntry" => Some(desktop_id.to_variant()),
        "CanQuit" | "CanRaise" => Some(true.to_variant()),
        "HasTrackList" => Some(false.to_variant()),
        "SupportedUriSchemes" => Some(vec!["file", "http", "https"].to_variant()),
        "SupportedMimeTypes" => Some(vec!["video/*", "audio/*"].to_variant()),
        _ => None,
    }
}

pub(super) fn get_player_property(prop: &str, state_cb: &StateCallback) -> Option<Variant> {
    let s = state_cb();
    match prop {
        "PlaybackStatus" => Some(playback_status(s.is_paused).to_variant()),
        "CanSeek" | "CanPlay" | "CanPause" | "CanControl" => Some(true.to_variant()),
        "CanGoNext" => Some(s.can_next.to_variant()),
        "CanGoPrevious" => Some(s.can_prev.to_variant()),
        "Volume" => Some((s.volume_pct / 100.0).to_variant()),
        "Position" => Some(((s.position_secs * 1_000_000.0) as i64).to_variant()),
        "LoopStatus" => Some(s.loop_status.to_variant()),
        "Shuffle" => Some(s.shuffle.to_variant()),
        "Rate" | "MinimumRate" | "MaximumRate" => Some(1.0_f64.to_variant()),
        "Metadata" => {
            let art = if s.art_url.is_empty() {
                None
            } else {
                Some(s.art_url.as_str())
            };
            Some(build_metadata(
                &s.title,
                &s.artist,
                &s.album,
                s.duration_secs,
                art,
            ))
        }
        _ => None,
    }
}

pub(super) fn set_player_property(
    prop: &str,
    value: &Variant,
    command_cb: &CommandCallback,
) -> bool {
    match prop {
        "Volume" => {
            if let Some(vol) = value.get::<f64>() {
                command_cb("set-volume", Some((vol * 100.0) as i64));
                return true;
            }
        }
        "LoopStatus" => {
            if let Some(loop_str) = value.get::<String>() {
                match loop_str.as_str() {
                    "None" => command_cb("set-loop", Some(0)),
                    "Track" => command_cb("set-loop", Some(1)),
                    "Playlist" => command_cb("set-loop", Some(2)),
                    _ => return false,
                }
                return true;
            }
        }
        "Shuffle" => {
            if let Some(shuffle) = value.get::<bool>() {
                command_cb("set-shuffle", Some(i64::from(shuffle)));
                return true;
            }
        }
        "Rate" => return true,
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gtk::glib::VariantDict;
    use gtk::glib::variant::{FromVariant, ToVariant};

    use super::{
        PlayerMethodDispatch, get_player_property, get_root_property, player_method_dispatch,
        set_player_property,
    };
    use crate::mpris::{CommandCallback, PlayerState, StateCallback};

    type RecordedCommands = Arc<Mutex<Vec<(String, Option<i64>)>>>;

    fn player_property<T: FromVariant + 'static>(property: &str, state: PlayerState) -> Option<T> {
        let state_callback: StateCallback = Box::new(move || state.clone());
        get_player_property(property, &state_callback).and_then(|variant| variant.get::<T>())
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn player_method_dispatch_maps_standard_commands() {
        let empty_params = ().to_variant();
        let expected_dispatches = [
            (
                "PlayPause",
                PlayerMethodDispatch {
                    command: "play-pause",
                    argument: None,
                    seeked_position: None,
                },
            ),
            (
                "Play",
                PlayerMethodDispatch {
                    command: "play",
                    argument: None,
                    seeked_position: None,
                },
            ),
            (
                "Pause",
                PlayerMethodDispatch {
                    command: "pause",
                    argument: None,
                    seeked_position: None,
                },
            ),
            (
                "Stop",
                PlayerMethodDispatch {
                    command: "stop",
                    argument: None,
                    seeked_position: None,
                },
            ),
            (
                "Next",
                PlayerMethodDispatch {
                    command: "next",
                    argument: None,
                    seeked_position: None,
                },
            ),
            (
                "Previous",
                PlayerMethodDispatch {
                    command: "previous",
                    argument: None,
                    seeked_position: None,
                },
            ),
        ];

        for (method, expected_dispatch) in expected_dispatches {
            assert_eq!(
                player_method_dispatch(method, &empty_params),
                Some(expected_dispatch)
            );
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn player_method_dispatch_maps_seek_arguments() {
        let seek_params = (12_345_i64,).to_variant();
        assert_eq!(
            player_method_dispatch("Seek", &seek_params),
            Some(PlayerMethodDispatch {
                command: "seek",
                argument: Some(12_345),
                seeked_position: None,
            })
        );

        let set_position_params = ("/org/mpris/MediaPlayer2/Track/1", 67_890_i64).to_variant();
        assert_eq!(
            player_method_dispatch("SetPosition", &set_position_params),
            Some(PlayerMethodDispatch {
                command: "set-position",
                argument: Some(67_890),
                seeked_position: Some(67_890),
            })
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn player_method_dispatch_rejects_unknown_or_invalid_arguments() {
        assert_eq!(player_method_dispatch("Raise", &().to_variant()), None);
        assert_eq!(player_method_dispatch("Seek", &().to_variant()), None);
        assert_eq!(
            player_method_dispatch("SetPosition", &("track", "invalid").to_variant()),
            None
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn root_identity_matches_desktop_id() {
        assert_eq!(
            get_root_property("Identity", "br.com.biglinux.AudioPlayer")
                .and_then(|v| v.get::<String>())
                .as_deref(),
            Some("Big Audio Player")
        );
        assert_eq!(
            get_root_property("Identity", "br.com.biglinux.VideoPlayer")
                .and_then(|v| v.get::<String>())
                .as_deref(),
            Some("Big Media Player")
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn root_properties_match_mpris_capabilities() {
        let desktop_entry = get_root_property("DesktopEntry", "br.com.biglinux.VideoPlayer")
            .and_then(|variant| variant.get::<String>());
        assert_eq!(
            desktop_entry.as_deref(),
            Some("br.com.biglinux.VideoPlayer")
        );

        for property in ["CanQuit", "CanRaise"] {
            assert_eq!(
                get_root_property(property, "br.com.biglinux.VideoPlayer")
                    .and_then(|variant| variant.get::<bool>()),
                Some(true)
            );
        }
        assert_eq!(
            get_root_property("HasTrackList", "br.com.biglinux.VideoPlayer")
                .and_then(|variant| variant.get::<bool>()),
            Some(false)
        );
        assert_eq!(
            get_root_property("SupportedUriSchemes", "br.com.biglinux.VideoPlayer")
                .and_then(|variant| variant.get::<Vec<String>>()),
            Some(vec!["file".into(), "http".into(), "https".into()])
        );
        assert_eq!(
            get_root_property("SupportedMimeTypes", "br.com.biglinux.VideoPlayer")
                .and_then(|variant| variant.get::<Vec<String>>()),
            Some(vec!["video/*".into(), "audio/*".into()])
        );
        assert!(get_root_property("Unknown", "br.com.biglinux.VideoPlayer").is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn player_property_metadata_wraps_snapshot() {
        let state_cb: StateCallback = Box::new(|| PlayerState {
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            duration_secs: 42.0,
            art_url: "file:///tmp/art.png".into(),
            ..PlayerState::default()
        });
        let meta = get_player_property("Metadata", &state_cb).expect("metadata");
        let dict = VariantDict::from_variant(&meta).expect("dict");
        assert_eq!(
            dict.lookup::<String>("xesam:title").expect("title"),
            Some("Song".into())
        );
        assert_eq!(
            dict.lookup::<String>("mpris:artUrl").expect("art"),
            Some("file:///tmp/art.png".into())
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn player_properties_wrap_snapshot_scalars() {
        let state = PlayerState {
            is_paused: false,
            position_secs: 2.5,
            volume_pct: 42.0,
            can_next: true,
            can_prev: false,
            loop_status: "Track".into(),
            shuffle: true,
            ..PlayerState::default()
        };

        assert_eq!(
            player_property::<String>("PlaybackStatus", state.clone()).as_deref(),
            Some("Playing")
        );
        for property in ["CanSeek", "CanPlay", "CanPause", "CanControl"] {
            assert_eq!(player_property::<bool>(property, state.clone()), Some(true));
        }
        assert_eq!(
            player_property::<bool>("CanGoNext", state.clone()),
            Some(true)
        );
        assert_eq!(
            player_property::<bool>("CanGoPrevious", state.clone()),
            Some(false)
        );
        let volume = player_property::<f64>("Volume", state.clone()).expect("volume");
        assert!((volume - 0.42).abs() < f64::EPSILON);
        assert_eq!(
            player_property::<i64>("Position", state.clone()),
            Some(2_500_000)
        );
        assert_eq!(
            player_property::<String>("LoopStatus", state.clone()).as_deref(),
            Some("Track")
        );
        assert_eq!(
            player_property::<bool>("Shuffle", state.clone()),
            Some(true)
        );
        for property in ["Rate", "MinimumRate", "MaximumRate"] {
            assert_eq!(player_property::<f64>(property, state.clone()), Some(1.0));
        }
        let default_state_callback: StateCallback = Box::new(PlayerState::default);
        assert!(get_player_property("Unknown", &default_state_callback).is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn set_player_property_dispatches_expected_commands() {
        let recorded_commands: RecordedCommands = Arc::new(Mutex::new(Vec::new()));
        let recorded_commands_for_callback = Arc::clone(&recorded_commands);
        let command_callback: CommandCallback = Box::new(move |command_name, argument| {
            recorded_commands_for_callback
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((command_name.to_string(), argument));
        });

        assert!(set_player_property(
            "Volume",
            &0.5_f64.to_variant(),
            &command_callback
        ));
        assert!(set_player_property(
            "LoopStatus",
            &"None".to_string().to_variant(),
            &command_callback
        ));
        assert!(set_player_property(
            "LoopStatus",
            &"Track".to_string().to_variant(),
            &command_callback
        ));
        assert!(set_player_property(
            "LoopStatus",
            &"Playlist".to_string().to_variant(),
            &command_callback
        ));
        assert!(set_player_property(
            "Shuffle",
            &true.to_variant(),
            &command_callback
        ));
        assert!(set_player_property(
            "Rate",
            &1.25_f64.to_variant(),
            &command_callback
        ));

        let recorded_commands = recorded_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            recorded_commands.as_slice(),
            &[
                ("set-volume".into(), Some(50)),
                ("set-loop".into(), Some(0)),
                ("set-loop".into(), Some(1)),
                ("set-loop".into(), Some(2)),
                ("set-shuffle".into(), Some(1)),
            ]
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn set_player_property_rejects_invalid_values() {
        let command_callback: CommandCallback = Box::new(|_, _| {});
        assert!(!set_player_property(
            "LoopStatus",
            &"Invalid".to_string().to_variant(),
            &command_callback
        ));
        assert!(!set_player_property(
            "Volume",
            &"invalid".to_string().to_variant(),
            &command_callback
        ));
        assert!(!set_player_property(
            "Shuffle",
            &"invalid".to_string().to_variant(),
            &command_callback
        ));
        assert!(!set_player_property(
            "Unknown",
            &true.to_variant(),
            &command_callback
        ));
    }
}
