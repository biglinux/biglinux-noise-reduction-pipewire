use glib::variant::ToVariant;
use gtk::{gio, glib};
use log::{info, warn};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::metadata::build_metadata;
use super::properties::{
    get_player_property, get_root_property, handle_player_method, set_player_property,
};
use super::signals::{emit_launcher_entry, emit_properties_changed};
use super::sync::sync_state;
use super::types::{CommandCallback, StateCallback, playback_status, previous_state, taskbar_flag};
use super::{MPRIS_PATH, MPRIS_PLAYER_IFACE, PLAYER_XML, ROOT_XML};

#[derive(Debug, Clone, PartialEq)]
struct LauncherEntryUpdate {
    desktop_uri: String,
    is_visible: bool,
    progress: f64,
}

fn root_method_command(method: &str) -> Option<&'static str> {
    match method {
        "Raise" => Some("raise"),
        "Quit" => Some("quit"),
        _ => None,
    }
}

fn private_session_connection_flags() -> gio::DBusConnectionFlags {
    gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
        .union(gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION)
}

fn desktop_application_uri(desktop_id: &str) -> String {
    format!("application://{desktop_id}.desktop")
}

fn hidden_launcher_entry_update(desktop_id: &str) -> LauncherEntryUpdate {
    LauncherEntryUpdate {
        desktop_uri: desktop_application_uri(desktop_id),
        is_visible: false,
        progress: 0.0,
    }
}

fn taskbar_progress_launcher_entry_update(
    desktop_id: &str,
    is_taskbar_progress_enabled: bool,
    position_secs: f64,
    duration_secs: f64,
) -> Option<LauncherEntryUpdate> {
    if !is_taskbar_progress_enabled {
        return None;
    }

    if duration_secs > 0.0 {
        Some(LauncherEntryUpdate {
            desktop_uri: desktop_application_uri(desktop_id),
            is_visible: true,
            progress: (position_secs / duration_secs).clamp(0.0, 1.0),
        })
    } else {
        Some(hidden_launcher_entry_update(desktop_id))
    }
}

fn playback_status_changed_properties(paused: bool) -> glib::Variant {
    let dict = glib::VariantDict::new(None);
    dict.insert("PlaybackStatus", playback_status(paused));
    dict.end()
}

fn metadata_changed_properties(
    title: &str,
    artist: &str,
    album: &str,
    duration_secs: f64,
    art_url: Option<&str>,
) -> glib::Variant {
    let dict = glib::VariantDict::new(None);
    dict.insert_value(
        "Metadata",
        &build_metadata(title, artist, album, duration_secs, art_url),
    );
    dict.end()
}

fn first_dbus_interface<'a>(
    node: &'a gio::DBusNodeInfo,
    interface_label: &str,
) -> Option<&'a gio::DBusInterfaceInfo> {
    match node.interfaces().iter().next() {
        Some(interface) => Some(interface),
        None => {
            warn!("MPRIS: {interface_label} XML has no interface");
            None
        }
    }
}

/// Mpris.
pub struct Mpris {
    /// Owned bus name; released explicitly in Drop BEFORE the connection
    /// closes (a close-first order fires name-lost with a NULL connection,
    /// which aborts inside gio's FFI closure trampoline).
    owner_id: Option<gio::OwnerId>,
    connection: gio::DBusConnection,
    timer_source: Option<glib::SourceId>,
    taskbar_progress: Arc<AtomicBool>,
    desktop_id: String,
}

impl Mpris {
    /// Build the MPRIS service with the BigVideoPlayer bus name and
    /// desktop id. Returns `None` if the bus connection cannot be
    /// established (no session bus, name taken, ...).
    pub fn new(command_cb: CommandCallback, state_cb: StateCallback) -> Option<Self> {
        Self::new_with_name(
            "org.mpris.MediaPlayer2.BigVideoPlayer",
            "br.com.biglinux.VideoPlayer",
            command_cb,
            state_cb,
            false,
        )
    }

    /// Build the MPRIS service with caller-supplied bus name and
    /// desktop id; used by apps that need a different identity than
    /// BigVideoPlayer. Returns `None` if the bus connection fails.
    pub fn new_with_name(
        bus_name: &str,
        desktop_id: &str,
        command_cb: CommandCallback,
        state_cb: StateCallback,
        taskbar_progress_enabled: bool,
    ) -> Option<Self> {
        let root_node = match gio::DBusNodeInfo::for_xml(ROOT_XML) {
            Ok(info) => info,
            Err(e) => {
                warn!("MPRIS: failed to parse root XML: {e}");
                return None;
            }
        };
        let player_node = match gio::DBusNodeInfo::for_xml(PLAYER_XML) {
            Ok(info) => info,
            Err(e) => {
                warn!("MPRIS: failed to parse player XML: {e}");
                return None;
            }
        };
        let root_iface = first_dbus_interface(&root_node, "root")?;
        let player_iface = first_dbus_interface(&player_node, "player")?;

        // PRIVATE session-bus connection per service instance. The shared
        // process connection (`bus_get_sync`) broke multicall hosts twice:
        // the interfaces stayed exported after the window closed (reopen →
        // "object already exported"), and two players in one process would
        // collide on the same /org/mpris/MediaPlayer2 path. A private
        // connection gives each module its own bus identity, and closing it
        // in Drop releases the name AND the exported objects atomically.
        let address =
            match gio::dbus_address_get_for_bus_sync(gio::BusType::Session, gio::Cancellable::NONE)
            {
                Ok(address) => address,
                Err(e) => {
                    warn!("MPRIS: failed to resolve session bus address: {e}");
                    return None;
                }
            };
        let connection = match gio::DBusConnection::for_address_sync(
            &address,
            private_session_connection_flags(),
            None,
            gio::Cancellable::NONE,
        ) {
            Ok(c) => c,
            Err(e) => {
                warn!("MPRIS: failed to connect a private session bus: {e}");
                return None;
            }
        };

        let owner_id = gio::bus_own_name_on_connection(
            &connection,
            bus_name,
            gio::BusNameOwnerFlags::NONE,
            |_, _| {},
            |_, _| {},
        );

        let command_cb = Arc::new(command_cb);
        let state_cb = Arc::new(state_cb);
        let prev_state = previous_state();

        {
            let cmd = command_cb.clone();
            let desktop_entry = desktop_id.to_string();
            let result = connection
                .register_object(MPRIS_PATH, root_iface)
                .method_call(
                    move |_conn, _sender, _path, _interface, method, _params, invocation| {
                        if let Some(command) = root_method_command(method) {
                            cmd(command, None);
                        }
                        invocation.return_value(None);
                    },
                )
                .property(
                    move |_conn, _sender, _path, _interface, property| -> glib::Variant {
                        get_root_property(property, &desktop_entry)
                            .unwrap_or_else(|| false.to_variant())
                    },
                )
                .build();
            if let Err(e) = result {
                warn!("MPRIS: failed to register root interface: {e}");
            }
        }

        {
            let cmd = command_cb.clone();
            let st_for_prop = state_cb.clone();
            let cmd_for_set = command_cb.clone();
            let conn_clone = connection.clone();

            let result = connection
                .register_object(MPRIS_PATH, player_iface)
                .method_call(
                    move |_conn, _sender, _path, _interface, method, params, invocation| {
                        handle_player_method(method, params, invocation, &cmd, &conn_clone);
                    },
                )
                .property(
                    move |_conn, _sender, _path, _interface, property| -> glib::Variant {
                        get_player_property(property, &st_for_prop)
                            .unwrap_or_else(|| false.to_variant())
                    },
                )
                .set_property(
                    move |_conn, _sender, _path, _interface, property, value| -> bool {
                        set_player_property(property, &value, &cmd_for_set)
                    },
                )
                .build();
            if let Err(e) = result {
                warn!("MPRIS: failed to register player interface: {e}");
            }
        }

        let conn_for_timer = connection.clone();
        let state_for_timer = state_cb;
        let taskbar_flag = taskbar_flag(taskbar_progress_enabled);
        let flag_for_timer = taskbar_flag.clone();
        let desktop_for_timer = desktop_application_uri(desktop_id);
        let timer_source = glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            sync_state(
                &conn_for_timer,
                &state_for_timer,
                &prev_state,
                &flag_for_timer,
                &desktop_for_timer,
            );
            glib::ControlFlow::Continue
        });

        info!("MPRIS2 service started on {bus_name}");
        Some(Self {
            owner_id: Some(owner_id),
            connection,
            timer_source: Some(timer_source),
            taskbar_progress: taskbar_flag,
            desktop_id: desktop_id.to_string(),
        })
    }

    fn cancel_timer(&mut self) {
        if let Some(id) = self.timer_source.take() {
            id.remove();
        }
    }

    /// Update the taskbar progress field in place.
    pub fn set_taskbar_progress(&self, enabled: bool) {
        self.taskbar_progress.store(enabled, Ordering::Relaxed);
        if !enabled {
            let update = hidden_launcher_entry_update(&self.desktop_id);
            emit_launcher_entry(
                &self.connection,
                &update.desktop_uri,
                update.is_visible,
                update.progress,
            );
        }
    }

    /// Return a reference to the `dbus connection` exposed by this [`Mpris`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    pub fn dbus_connection(&self) -> &gio::DBusConnection {
        &self.connection
    }

    /// Return a reference to the `taskbar flag` exposed by this [`Mpris`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    pub fn taskbar_flag(&self) -> Arc<AtomicBool> {
        self.taskbar_progress.clone()
    }

    /// Return a reference to the `desktop entry id` exposed by this [`Mpris`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    pub fn desktop_entry_id(&self) -> &str {
        &self.desktop_id
    }

    /// Clear any active taskbar progress.
    pub fn clear_taskbar_progress(connection: &gio::DBusConnection, desktop_id: &str) {
        let update = hidden_launcher_entry_update(desktop_id);
        emit_launcher_entry(
            connection,
            &update.desktop_uri,
            update.is_visible,
            update.progress,
        );
    }

    /// Emit the seeked signal on the active D-Bus connection.
    pub fn emit_seeked(&self, position_us: i64) {
        let _ = self.connection.emit_signal(
            None::<&str>,
            MPRIS_PATH,
            MPRIS_PLAYER_IFACE,
            "Seeked",
            Some(&(position_us,).to_variant()),
        );
    }

    /// Push a new taskbar progress value through the live state.
    pub fn update_taskbar_progress(&self, position_secs: f64, duration_secs: f64) {
        if let Some(update) = taskbar_progress_launcher_entry_update(
            &self.desktop_id,
            self.taskbar_progress.load(Ordering::Relaxed),
            position_secs,
            duration_secs,
        ) {
            emit_launcher_entry(
                &self.connection,
                &update.desktop_uri,
                update.is_visible,
                update.progress,
            );
        }
    }

    /// Hide the taskbar progress surface.
    pub fn hide_taskbar_progress(&self) {
        let update = hidden_launcher_entry_update(&self.desktop_id);
        emit_launcher_entry(
            &self.connection,
            &update.desktop_uri,
            update.is_visible,
            update.progress,
        );
    }

    /// Emit the playback status signal on the active D-Bus connection.
    pub fn emit_playback_status(&self, paused: bool) {
        emit_properties_changed(&self.connection, playback_status_changed_properties(paused));
    }

    /// Emit the metadata signal on the active D-Bus connection.
    pub fn emit_metadata(&self, title: &str, artist: &str, album: &str, duration_secs: f64) {
        self.emit_metadata_with_art(title, artist, album, duration_secs, None);
    }

    /// Emit the metadata with art signal on the active D-Bus connection.
    pub fn emit_metadata_with_art(
        &self,
        title: &str,
        artist: &str,
        album: &str,
        duration_secs: f64,
        art_url: Option<&str>,
    ) {
        emit_properties_changed(
            &self.connection,
            metadata_changed_properties(title, artist, album, duration_secs, art_url),
        );
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        self.cancel_timer();
        // Order matters: release the bus name FIRST (with the connection
        // alive), THEN close the private connection. Closing first makes
        // GLib fire the name-lost callback with a NULL connection, which
        // panics in gio's closure trampoline — a non-unwinding abort.
        if let Some(owner_id) = self.owner_id.take() {
            gio::bus_unown_name(owner_id);
        }
        let _ = self.connection.close_sync(gio::Cancellable::NONE);
    }
}

#[cfg(test)]
mod tests {
    use gtk::gio;
    use gtk::glib::VariantDict;
    use gtk::glib::variant::FromVariant;

    use super::{
        LauncherEntryUpdate, desktop_application_uri, first_dbus_interface,
        hidden_launcher_entry_update, metadata_changed_properties,
        playback_status_changed_properties, private_session_connection_flags, root_method_command,
        taskbar_progress_launcher_entry_update,
    };

    #[test]
    fn root_method_command_dispatches_window_commands() {
        assert_eq!(root_method_command("Raise"), Some("raise"));
        assert_eq!(root_method_command("Quit"), Some("quit"));
        assert_eq!(root_method_command("Play"), None);
    }

    #[test]
    fn private_session_connection_flags_enable_client_message_bus_connection() {
        assert_eq!(
            private_session_connection_flags(),
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                .union(gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION)
        );
    }

    #[test]
    fn first_dbus_interface_rejects_nodes_without_interfaces() {
        let node = gio::DBusNodeInfo::for_xml("<node></node>").unwrap();
        assert!(first_dbus_interface(&node, "empty").is_none());
    }

    #[test]
    fn desktop_application_uri_uses_application_scheme_and_desktop_suffix() {
        assert_eq!(
            desktop_application_uri("br.com.biglinux.VideoPlayer"),
            "application://br.com.biglinux.VideoPlayer.desktop"
        );
    }

    #[test]
    fn hidden_launcher_entry_update_clears_progress() {
        assert_eq!(
            hidden_launcher_entry_update("br.com.biglinux.AudioPlayer"),
            LauncherEntryUpdate {
                desktop_uri: "application://br.com.biglinux.AudioPlayer.desktop".into(),
                is_visible: false,
                progress: 0.0,
            }
        );
    }

    #[test]
    fn taskbar_progress_launcher_entry_update_respects_enabled_flag() {
        assert_eq!(
            taskbar_progress_launcher_entry_update(
                "br.com.biglinux.VideoPlayer",
                false,
                50.0,
                100.0,
            ),
            None
        );
    }

    #[test]
    fn taskbar_progress_launcher_entry_update_scales_and_clamps_progress() {
        assert_eq!(
            taskbar_progress_launcher_entry_update(
                "br.com.biglinux.VideoPlayer",
                true,
                25.0,
                100.0,
            ),
            Some(LauncherEntryUpdate {
                desktop_uri: "application://br.com.biglinux.VideoPlayer.desktop".into(),
                is_visible: true,
                progress: 0.25,
            })
        );
        assert_eq!(
            taskbar_progress_launcher_entry_update(
                "br.com.biglinux.VideoPlayer",
                true,
                -25.0,
                100.0,
            )
            .map(|update| update.progress),
            Some(0.0)
        );
        assert_eq!(
            taskbar_progress_launcher_entry_update(
                "br.com.biglinux.VideoPlayer",
                true,
                125.0,
                100.0,
            )
            .map(|update| update.progress),
            Some(1.0)
        );
    }

    #[test]
    fn taskbar_progress_launcher_entry_update_hides_without_duration() {
        assert_eq!(
            taskbar_progress_launcher_entry_update("br.com.biglinux.VideoPlayer", true, 25.0, 0.0,),
            Some(LauncherEntryUpdate {
                desktop_uri: "application://br.com.biglinux.VideoPlayer.desktop".into(),
                is_visible: false,
                progress: 0.0,
            })
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn playback_status_changed_properties_wraps_status() {
        let paused_properties = playback_status_changed_properties(true);
        let paused_dict = VariantDict::from_variant(&paused_properties).expect("paused dict");
        assert_eq!(
            paused_dict
                .lookup::<String>("PlaybackStatus")
                .expect("paused status")
                .as_deref(),
            Some("Paused")
        );

        let playing_properties = playback_status_changed_properties(false);
        let playing_dict = VariantDict::from_variant(&playing_properties).expect("playing dict");
        assert_eq!(
            playing_dict
                .lookup::<String>("PlaybackStatus")
                .expect("playing status")
                .as_deref(),
            Some("Playing")
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn metadata_changed_properties_wraps_metadata_payload() {
        let properties = metadata_changed_properties(
            "Song",
            "Artist",
            "Album",
            12.5,
            Some("file:///tmp/art.png"),
        );
        let properties_dict = VariantDict::from_variant(&properties).expect("properties dict");
        let metadata = properties_dict
            .lookup_value("Metadata", None)
            .expect("metadata payload");
        let metadata_dict = VariantDict::from_variant(&metadata).expect("metadata dict");

        assert_eq!(
            metadata_dict
                .lookup::<String>("xesam:title")
                .expect("title")
                .as_deref(),
            Some("Song")
        );
        assert_eq!(
            metadata_dict
                .lookup::<String>("mpris:artUrl")
                .expect("art")
                .as_deref(),
            Some("file:///tmp/art.png")
        );
    }
}
