use super::*;
use crate::actions::{BigActionScope, BigActionSpec};
use gtk::glib::value::ToValue;

#[test]
fn uri_list_parser_ignores_comments_and_limits() {
    assert_eq!(
        parse_uri_list("# comment\nfile:///a\n\nfile:///b\nfile:///c", Some(2)),
        vec!["file:///a".to_string(), "file:///b".to_string()]
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn drop_paths_from_text_accepts_uris_and_paths() {
    assert_eq!(
        drop_paths_from_text("file:///tmp/a.mp3\n/home/me/b.mp3", None),
        vec![
            std::path::PathBuf::from("/tmp/a.mp3"),
            std::path::PathBuf::from("/home/me/b.mp3")
        ]
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn drop_paths_from_value_accepts_string_payload() {
    let dropped_value = "/tmp/a.mp3\n/tmp/b.mp3".to_value();

    assert_eq!(
        drop_paths_from_value(&dropped_value),
        vec![
            std::path::PathBuf::from("/tmp/a.mp3"),
            std::path::PathBuf::from("/tmp/b.mp3")
        ]
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn drop_paths_from_value_accepts_gio_file_payload() {
    let file = gio::File::for_path("/tmp/a.mp3");
    let dropped_value = file.to_value();

    assert_eq!(
        drop_paths_from_value(&dropped_value),
        vec![std::path::PathBuf::from("/tmp/a.mp3")]
    );
}

#[test]
fn picker_labels_match_mode() {
    assert_eq!(
        picker_icon_and_label(&BigFilePickerSpec::open_file_button("Open file")),
        ("document-open-symbolic", "Select file")
    );
    assert_eq!(
        picker_icon_and_label(&BigFilePickerSpec::open_folder_button("Open folder")),
        ("folder-open-symbolic", "Select folder")
    );
    assert_eq!(
        picker_icon_and_label(&BigFilePickerSpec::save_file_button("Save file")),
        ("document-save-symbolic", "Save file")
    );
    assert_eq!(
        picker_icon_and_label(&BigFilePickerSpec::destination_folder_row(
            "Destination folder"
        )),
        ("folder-symbolic", "Select destination")
    );
}

#[test]
fn dialog_icons_follow_severity() {
    assert_eq!(dialog_icon_name("error"), "dialog-error-symbolic");
    assert_eq!(dialog_icon_name("warning"), "dialog-warning-symbolic");
    assert_eq!(dialog_icon_name("confirm"), "dialog-question-symbolic");
    assert_eq!(dialog_icon_name("info"), "dialog-information-symbolic");
}

#[test]
fn closable_settings_window_spec_names_close_action() {
    let spec = BigClosableSettingsWindowSpec::new("AI Assistant").size(640, 480);

    assert_eq!(spec.title, "AI Assistant");
    assert_eq!(spec.close_label, "Close AI Assistant");
    assert_eq!(spec.default_width, 640);
    assert_eq!(spec.default_height, 480);
}

#[test]
fn preview_labels_cover_media_and_editor_modes() {
    assert_eq!(
        preview_icon_name(BigPreviewKind::Image),
        "image-x-generic-symbolic"
    );
    assert_eq!(
        preview_icon_name(BigPreviewKind::MiniImageEditor),
        "image-x-generic-symbolic"
    );
    assert_eq!(
        preview_icon_name(BigPreviewKind::CropResizeRotate),
        "image-x-generic-symbolic"
    );
    assert_eq!(
        preview_icon_name(BigPreviewKind::Video),
        "video-x-generic-symbolic"
    );
    assert_eq!(
        preview_icon_name(BigPreviewKind::Audio),
        "audio-x-generic-symbolic"
    );
    assert_eq!(
        preview_icon_name(BigPreviewKind::Metadata),
        "dialog-information-symbolic"
    );
    assert_eq!(
        preview_icon_name(BigPreviewKind::MiniTextEditor),
        "text-x-generic-symbolic"
    );
    assert_eq!(preview_description(&BigPreviewSpec::image()), "Preview");
    assert_eq!(
        preview_description(&BigPreviewSpec::mini_text_editor()),
        "Editor"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn gio_menu_from_actions_preserves_action_count() {
    // gio::Menu construction crosses GLib FFI, which Miri cannot execute. Normal
    // tests still cover the menu item projection exported by this helper.
    let actions = [
        BigActionSpec::new("open", "Open", BigActionScope::App),
        BigActionSpec::new("close", "Close", BigActionScope::Window),
    ];
    let menu = gio_menu_from_actions(&actions);

    assert_eq!(menu.n_items(), 2);
}

#[test]
fn expand_drop_paths_keeps_files_and_expands_dirs() {
    let paths = vec![
        std::path::PathBuf::from("/tmp/file.mp3"),
        std::path::PathBuf::from("/tmp"),
    ];
    let expanded = expand_drop_paths(paths, |_| vec![std::path::PathBuf::from("/tmp/inside.mp3")]);
    assert!(expanded.contains(&std::path::PathBuf::from("/tmp/file.mp3")));
    assert!(expanded.contains(&std::path::PathBuf::from("/tmp/inside.mp3")));
}
