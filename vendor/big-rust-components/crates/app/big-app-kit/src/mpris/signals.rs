use glib::variant::ToVariant;
use gtk::{gio, glib};

use super::{LAUNCHER_ENTRY_IFACE, MPRIS_PATH, MPRIS_PLAYER_IFACE};

fn properties_changed_signal_body(changed_props: glib::Variant) -> glib::Variant {
    let empty_invalidated: Vec<String> = Vec::new();
    glib::Variant::tuple_from_iter([
        MPRIS_PLAYER_IFACE.to_variant(),
        changed_props,
        empty_invalidated.to_variant(),
    ])
}

fn launcher_entry_signal_body(desktop_uri: &str, visible: bool, progress: f64) -> glib::Variant {
    let dict = glib::VariantDict::new(None);
    dict.insert("progress-visible", visible);
    dict.insert("progress", progress);
    glib::Variant::tuple_from_iter([desktop_uri.to_variant(), dict.end()])
}

pub(super) fn emit_properties_changed(
    connection: &gio::DBusConnection,
    changed_props: glib::Variant,
) {
    let body = properties_changed_signal_body(changed_props);
    let _ = connection.emit_signal(
        None::<&str>,
        MPRIS_PATH,
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
        Some(&body),
    );
}

pub(super) fn emit_launcher_entry(
    connection: &gio::DBusConnection,
    desktop_uri: &str,
    visible: bool,
    progress: f64,
) {
    let body = launcher_entry_signal_body(desktop_uri, visible, progress);
    let _ = connection.emit_signal(
        None::<&str>,
        "/com/canonical/unity/launcherentry/1",
        LAUNCHER_ENTRY_IFACE,
        "Update",
        Some(&body),
    );
}

#[cfg(test)]
mod tests {
    use gtk::glib::VariantDict;
    use gtk::glib::variant::FromVariant;

    use super::{launcher_entry_signal_body, properties_changed_signal_body};
    use crate::mpris::MPRIS_PLAYER_IFACE;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn properties_changed_signal_body_wraps_changed_properties() {
        let changed_properties = {
            let dict = gtk::glib::VariantDict::new(None);
            dict.insert("PlaybackStatus", "Playing");
            dict.end()
        };
        let body = properties_changed_signal_body(changed_properties);

        assert_eq!(
            body.child_value(0).get::<String>().as_deref(),
            Some(MPRIS_PLAYER_IFACE)
        );
        let wrapped_properties = body.child_value(1);
        let wrapped_dict = VariantDict::from_variant(&wrapped_properties).expect("changed props");
        assert_eq!(
            wrapped_dict
                .lookup::<String>("PlaybackStatus")
                .expect("playback status")
                .as_deref(),
            Some("Playing")
        );
        assert_eq!(
            body.child_value(2).get::<Vec<String>>(),
            Some(Vec::<String>::new())
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn launcher_entry_signal_body_wraps_desktop_uri_and_progress() {
        let body = launcher_entry_signal_body(
            "application://br.com.biglinux.VideoPlayer.desktop",
            true,
            0.75,
        );

        assert_eq!(
            body.child_value(0).get::<String>().as_deref(),
            Some("application://br.com.biglinux.VideoPlayer.desktop")
        );
        let update_properties = body.child_value(1);
        let update_dict = VariantDict::from_variant(&update_properties).expect("update props");
        assert_eq!(
            update_dict
                .lookup::<bool>("progress-visible")
                .expect("progress visible"),
            Some(true)
        );
        assert_eq!(
            update_dict.lookup::<f64>("progress").expect("progress"),
            Some(0.75)
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn launcher_entry_signal_body_preserves_hidden_zero_progress() {
        let body = launcher_entry_signal_body(
            "application://br.com.biglinux.AudioPlayer.desktop",
            false,
            0.0,
        );
        let update_properties = body.child_value(1);
        let update_dict = VariantDict::from_variant(&update_properties).expect("update props");

        assert_eq!(
            update_dict
                .lookup::<bool>("progress-visible")
                .expect("progress visible"),
            Some(false)
        );
        assert_eq!(
            update_dict.lookup::<f64>("progress").expect("progress"),
            Some(0.0)
        );
    }
}
