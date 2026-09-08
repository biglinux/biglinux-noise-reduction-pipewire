// SPDX-License-Identifier: MIT

//! Headless render smoke for [`BigZoneLayoutEditor`].
//!
//! Builds a media-player-style two-bar layout (top: pin/title/menu, bottom:
//! transport + a seek slider) and shows the editor in a window so the miniature,
//! its drop zones, and the draggable control buttons can be screenshot-verified.
//! Drag/drop itself needs real pointer input (not driveable headless) — this
//! proves the widget RENDERS; interaction is covered on the VM.
//!
//! Run: `cargo run -p big-relm4-components --example zone_layout_editor_demo`.

use adw::prelude::*;
use big_relm4_components::input::zone_layout::{
    BarDecoration, BarSpec, ZoneControl, ZoneLayout, ZoneLayoutSpec, ZonePlacement,
};
use big_relm4_components::input::zone_layout_editor::BigZoneLayoutEditor;
use relm4::gtk;
use relm4::gtk::glib;
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent};

fn demo_spec() -> ZoneLayoutSpec {
    ZoneLayoutSpec::new(
        vec![
            ZoneControl::new("pin", "Keep Above", "view-pin-symbolic", "top:start"),
            ZoneControl::new("menu", "Menu", "open-menu-symbolic", "top:end"),
            ZoneControl::new(
                "volume",
                "Volume",
                "audio-volume-high-symbolic",
                "bottom:start",
            ),
            ZoneControl::new(
                "prev",
                "Previous",
                "media-skip-backward-symbolic",
                "bottom:center",
            ),
            ZoneControl::new(
                "play",
                "Play",
                "media-playback-start-symbolic",
                "bottom:center",
            ),
            ZoneControl::new(
                "next",
                "Next",
                "media-skip-forward-symbolic",
                "bottom:center",
            ),
            ZoneControl::new(
                "subtitle",
                "Subtitles",
                "media-view-subtitles-symbolic",
                "bottom:end",
            ),
            ZoneControl::new(
                "fullscreen",
                "Fullscreen",
                "view-fullscreen-symbolic",
                "bottom:end",
            ),
        ],
        vec![
            BarSpec::new("top", vec![ZonePlacement::Start, ZonePlacement::End]).with_decoration(
                BarDecoration {
                    title: Some("Video Title".into()),
                    seek_slider: false,
                },
            ),
            BarSpec::new("bottom", ZonePlacement::ALL.to_vec()).with_decoration(BarDecoration {
                title: None,
                seek_slider: true,
            }),
        ],
    )
}

fn main() {
    let app = RelmApp::new("com.biglinux.ZoneLayoutEditorDemo");
    app.run::<ZoneLayoutEditorDemo>(());
}

struct ZoneLayoutEditorDemo {
    _editor: BigZoneLayoutEditor,
}

impl SimpleComponent for ZoneLayoutEditorDemo {
    type Input = ();
    type Output = ();
    type Init = ();
    type Root = adw::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .default_width(720)
            .default_height(420)
            .title("BigZoneLayoutEditor demo")
            .build()
    }

    fn init(
        (): Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let editor = build_editor();
        root.set_content(Some(&build_page(&editor)));

        glib::timeout_add_seconds_local_once(3, || {
            println!("zone_layout_editor_demo: ok (editor rendered, defaults placed)");
            if let Some(app) = gtk::gio::Application::default() {
                app.quit();
            }
        });

        ComponentParts {
            model: Self { _editor: editor },
            widgets: (),
        }
    }
}

fn build_editor() -> BigZoneLayoutEditor {
    let spec = demo_spec();
    // Start from the per-control defaults (an empty layout normalizes to them).
    let editor = BigZoneLayoutEditor::new(&spec, &ZoneLayout::default());
    editor.connect_changed(|layout| {
        println!(
            "zone_layout_editor_demo: changed, bottom:center={:?}",
            layout.zone("bottom", ZonePlacement::Center)
        );
    });

    // Sanity: every control landed in its default slot.
    let layout = editor.layout();
    assert_eq!(layout.zone("top", ZonePlacement::Start), ["pin"]);
    assert_eq!(
        layout.zone("bottom", ZonePlacement::Center),
        ["prev", "play", "next"]
    );
    assert_eq!(
        layout.zone("bottom", ZonePlacement::End),
        ["subtitle", "fullscreen"]
    );

    editor
}

fn build_page(editor: &BigZoneLayoutEditor) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_margin_top(24);
    page.set_margin_bottom(24);
    page.set_margin_start(24);
    page.set_margin_end(24);
    let heading = gtk::Label::builder()
        .label("Button Layout — drag between zones · click to toggle")
        .css_classes(["title-3"])
        .build();
    page.append(&heading);
    page.append(editor.root());
    page
}
