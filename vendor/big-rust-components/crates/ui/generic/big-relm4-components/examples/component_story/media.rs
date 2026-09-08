// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Media-window story for the shared component runner.

use adw::prelude::*;
use big_relm4_components::input::button_content::{BigButtonContentSpec, build_button};
use big_relm4_components::layout::hamburger_menu::BigMenuActionItem;
use big_relm4_components::layout::media_window::{
    BigMediaChrome, BigMediaChromePlacement, BigMediaMenuSpec, BigMediaOverlayStack,
    BigMediaSidebarShell, BigMediaSidebarSpec,
};
use relm4::gtk;

use super::{StorySurface, install_window_action_stubs};

pub(super) fn build_media_window_story(root: &adw::ApplicationWindow) -> StorySurface {
    install_media_window_story_css();

    let menu = media_window_story_menu_spec();
    install_window_action_stubs(root, &menu.items);
    let chrome = BigMediaChrome::new(menu, true);
    let pin_button = media_window_story_pin_button();
    chrome.add_header_start(&pin_button);

    let media_stack = media_window_story_stack();
    let buffering_status = media_window_story_buffering_status();
    media_stack.add_status_overlay(&buffering_status);

    let bottom_controls = media_window_story_playback_controls();
    chrome.set_bottom_child(&bottom_controls);
    let sidebar = media_window_story_sidebar();
    media_stack.install_chrome(&chrome, BigMediaChromePlacement::HeaderThenBottom);

    let sidebar_shell = BigMediaSidebarShell::new(
        &sidebar,
        media_stack.overlay(),
        BigMediaSidebarSpec::new()
            .visible_initially(true)
            .width_range(280.0, 360.0)
            .resize_limit(520.0),
    );
    root.set_content(Some(sidebar_shell.split_view()));

    StorySurface {
        ready_widget: chrome.menu_button().clone().upcast(),
        transient_menu_button: None,
        _keepalive_objects: vec![
            Box::new(chrome) as Box<dyn std::any::Any>,
            Box::new(media_stack) as Box<dyn std::any::Any>,
            Box::new(sidebar_shell) as Box<dyn std::any::Any>,
        ],
    }
}

fn media_window_story_menu_spec() -> BigMediaMenuSpec {
    BigMediaMenuSpec {
        icon_name: "open-menu-symbolic".to_owned(),
        label: "Main Menu".to_owned(),
        items: vec![
            BigMenuActionItem::new("Open Video", "win.open-video"),
            BigMenuActionItem::new("Playlist", "win.playlist"),
            BigMenuActionItem::new("Preferences", "win.preferences"),
            BigMenuActionItem::new("Keyboard Shortcuts", "win.shortcuts"),
        ],
    }
}

fn media_window_story_pin_button() -> gtk::Button {
    build_button(
        &BigButtonContentSpec::new("view-pin-symbolic", "Pin media controls")
            .accessible_label("Pin media controls")
            .tooltip("Keep media controls visible")
            .css_classes(["osd-btn"]),
    )
}

fn media_window_story_stack() -> BigMediaOverlayStack {
    let scene_background = media_window_story_background();
    let media_surface = media_window_story_surface();
    let media_stack = BigMediaOverlayStack::new(&scene_background);
    media_stack.set_overflow(gtk::Overflow::Hidden);
    media_stack.add_primary_media_surface(&media_surface);
    media_stack
}

fn media_window_story_background() -> gtk::Box {
    let scene_background = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["story-media-background"])
        .build();
    scene_background.update_property(&[gtk::accessible::Property::Label("Media scene background")]);
    scene_background.append(
        &gtk::Label::builder()
            .label("Media window story")
            .css_classes(["title-1"])
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .vexpand(true)
            .build(),
    );
    scene_background
}

fn media_window_story_surface() -> gtk::Box {
    let media_surface = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .width_request(560)
        .height_request(300)
        .css_classes(["story-media-surface"])
        .build();
    media_surface.update_property(&[gtk::accessible::Property::Label("App-owned media surface")]);
    media_surface.append(
        &gtk::Label::builder()
            .label("App-owned media surface")
            .css_classes(["heading"])
            .build(),
    );
    media_surface
}

fn media_window_story_buffering_status() -> gtk::Box {
    let buffering_status = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(8)
        .css_classes(["osd", "card"])
        .build();
    buffering_status.update_property(&[gtk::accessible::Property::Label("Buffering status")]);
    buffering_status.append(&adw::Spinner::new());
    buffering_status.append(&gtk::Label::new(Some("Buffering status")));
    buffering_status
}

fn media_window_story_playback_controls() -> gtk::Box {
    let bottom_controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::End)
        .spacing(8)
        .margin_bottom(12)
        .css_classes(["story-media-controls", "osd"])
        .build();
    bottom_controls.update_property(&[gtk::accessible::Property::Label("Playback controls")]);
    bottom_controls.append(&media_window_story_control_button(
        "media-skip-backward-symbolic",
        "Skip backward",
        "Skip backward",
    ));
    bottom_controls.append(&media_window_story_control_button(
        "media-playback-start-symbolic",
        "Play media",
        "Play media",
    ));
    bottom_controls.append(&media_window_story_control_button(
        "media-skip-forward-symbolic",
        "Skip forward",
        "Skip forward",
    ));
    bottom_controls
}

fn media_window_story_control_button(
    icon_name: &'static str,
    visible_label: &'static str,
    accessible_label: &'static str,
) -> gtk::Button {
    build_button(
        &BigButtonContentSpec::new(icon_name, visible_label)
            .accessible_label(accessible_label)
            .tooltip(accessible_label),
    )
}

fn media_window_story_sidebar() -> gtk::Box {
    let sidebar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .css_classes(["story-media-sidebar"])
        .build();
    sidebar.update_property(&[gtk::accessible::Property::Label("Playlist sidebar")]);
    sidebar.append(
        &gtk::Label::builder()
            .label("Playlist sidebar")
            .css_classes(["heading"])
            .halign(gtk::Align::Start)
            .build(),
    );
    for title in ["First clip", "Second clip", "Third clip"] {
        sidebar.append(
            &gtk::Label::builder()
                .label(title)
                .halign(gtk::Align::Start)
                .build(),
        );
    }
    sidebar
}

fn install_media_window_story_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "
        .story-media-background {
            background: #111318;
            color: white;
        }
        .story-media-surface {
            background: #202633;
            border: 1px solid alpha(white, 0.35);
            border-radius: 8px;
            color: white;
        }
        .story-media-controls {
            padding: 8px;
            border-radius: 8px;
        }
        .story-media-sidebar {
            background: @window_bg_color;
        }
        ",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
