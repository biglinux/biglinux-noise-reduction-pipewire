// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Headless runtime probe for shared media immersion chrome.
//!
//! This example mounts `BigMediaChrome`, `BigMediaOverlayStack`, and
//! `BigMediaImmersionController` in a real GTK/libadwaita window, then checks
//! auto-hide, hide guards, inhibit/uninhibit, explicit show/hide, and teardown
//! finalization. Run it only through the headless KWin/AT-SPI harness.

use std::cell::Cell;
use std::env;
use std::fs;
use std::path::Path;
use std::process;
use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::input::button_content::{BigButtonContentSpec, build_button};
use big_relm4_components::layout::hamburger_menu::BigMenuActionItem;
use big_relm4_components::layout::media_window::{
    BigMediaChrome, BigMediaChromePlacement, BigMediaImmersionController, BigMediaMenuSpec,
    BigMediaOverlayStack,
};
use relm4::gtk;
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent};

const IDLE_TIMEOUT: Duration = Duration::from_millis(1);
const BEFORE_MIN_TIMEOUT_SETTLE: Duration = Duration::from_millis(120);
const AFTER_MIN_TIMEOUT_SETTLE: Duration = Duration::from_millis(260);
const TIMER_SETTLE: Duration = Duration::from_millis(360);
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

fn main() {
    let app = RelmApp::new("br.com.biglinux.big_relm4_components.media_immersion_probe");
    app.run::<MediaImmersionProbe>(());
    process::exit(EXIT_CODE.load(Ordering::SeqCst));
}

struct MediaImmersionProbe;

impl SimpleComponent for MediaImmersionProbe {
    type Input = ();
    type Output = ();
    type Init = ();
    type Root = adw::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .default_width(1280)
            .default_height(720)
            .title("Media immersion probe")
            .build()
    }

    fn init(
        (): Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        install_probe_placeholder(&root);
        root.present();
        gtk::glib::timeout_add_local_once(Duration::from_millis(500), move || {
            println!("BIG_MEDIA_IMMERSION_PROBE_READY");
            if let Err(error) = run_probe(&root) {
                eprintln!("BIG_MEDIA_IMMERSION_PROBE_FAIL: {error}");
                write_probe_result(&format!("status=fail\nerror={error}\n"));
                EXIT_CODE.store(3, Ordering::SeqCst);
            } else {
                println!("BIG_MEDIA_IMMERSION_PROBE_OK");
                write_probe_result(
                    "status=ok\nmin_timeout_clamp=ok\nauto_hide=ok\nhide_guard=ok\nfocus_hide_guard=ok\ninhibit=ok\nexplicit_show_hide=ok\nteardown_finalization=ok\n",
                );
            }
            if let Some(app) = root.application() {
                gtk::glib::timeout_add_local_once(Duration::from_millis(5_000), move || {
                    app.quit();
                });
            }
        });

        ComponentParts {
            model: Self,
            widgets: (),
        }
    }
}

fn install_probe_placeholder(root: &adw::ApplicationWindow) {
    let placeholder = gtk::Label::builder()
        .label("Media immersion probe")
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    placeholder.update_property(&[gtk::accessible::Property::Label("Media immersion probe")]);
    root.set_content(Some(&placeholder));
}

fn run_probe(root: &adw::ApplicationWindow) -> Result<(), String> {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }

    let (header_weak, bottom_weak, status_weak, overlay_weak) = {
        let background = probe_background();
        let media_surface = probe_media_surface();
        let status_overlay = probe_status_overlay();
        let chrome = probe_chrome();
        let (bottom_controls, play_button) = probe_bottom_controls();
        chrome.set_bottom_child(&bottom_controls);

        let media_stack = BigMediaOverlayStack::new(&background);
        media_stack.add_primary_media_surface(&media_surface);
        media_stack.add_status_overlay(&status_overlay);
        media_stack.install_chrome(&chrome, BigMediaChromePlacement::HeaderThenBottom);

        let controller = BigMediaImmersionController::new(media_stack.overlay());
        controller.set_idle_timeout(IDLE_TIMEOUT);
        controller.register_revealer(chrome.header_revealer());
        controller.register_revealer(chrome.bottom_revealer());
        controller.register_opacity_target(&status_overlay);

        root.set_content(Some(media_stack.overlay()));
        root.present();
        pump();
        assert_media_chrome_state(&chrome, &status_overlay, true, "initial reveal")?;

        controller.show_and_schedule_hide();
        wait_for(BEFORE_MIN_TIMEOUT_SETTLE);
        assert_media_chrome_state(&chrome, &status_overlay, true, "minimum timeout clamp")?;

        wait_for(AFTER_MIN_TIMEOUT_SETTLE);
        assert_media_chrome_state(&chrome, &status_overlay, false, "idle auto-hide")?;

        let guard_active = Rc::new(Cell::new(true));
        controller.register_hide_guard({
            let guard_active = Rc::clone(&guard_active);
            move || guard_active.get()
        });
        controller.show_and_schedule_hide();
        wait_for(TIMER_SETTLE);
        assert_media_chrome_state(&chrome, &status_overlay, true, "hide guard")?;

        guard_active.set(false);
        controller.show_and_schedule_hide();
        wait_for(TIMER_SETTLE);
        assert_media_chrome_state(&chrome, &status_overlay, false, "released hide guard")?;

        controller.show_and_schedule_hide();
        play_button.grab_focus();
        pump();
        wait_for(TIMER_SETTLE);
        assert_media_chrome_state(&chrome, &status_overlay, true, "focused chrome hide guard")?;

        gtk::prelude::GtkWindowExt::set_focus(root, None::<&gtk::Widget>);
        pump();
        controller.show_and_schedule_hide();
        wait_for(TIMER_SETTLE);
        assert_media_chrome_state(
            &chrome,
            &status_overlay,
            false,
            "released focused chrome hide guard",
        )?;

        controller.inhibit();
        controller.show_and_schedule_hide();
        wait_for(TIMER_SETTLE);
        assert_media_chrome_state(&chrome, &status_overlay, true, "inhibit")?;

        controller.uninhibit();
        wait_for(TIMER_SETTLE);
        assert_media_chrome_state(&chrome, &status_overlay, false, "uninhibit")?;

        controller.show_now();
        pump();
        assert_media_chrome_state(&chrome, &status_overlay, true, "explicit show")?;

        controller.hide_now();
        pump();
        assert_media_chrome_state(&chrome, &status_overlay, false, "explicit hide")?;

        let header_weak = chrome.header_revealer().downgrade();
        let bottom_weak = chrome.bottom_revealer().downgrade();
        let status_weak = status_overlay.downgrade();
        let overlay_weak = media_stack.overlay().downgrade();
        root.set_content(Option::<&gtk::Widget>::None);
        (header_weak, bottom_weak, status_weak, overlay_weak)
    };

    pump();
    pump();
    assert_finalized(header_weak.upgrade().is_none(), "header revealer")?;
    assert_finalized(bottom_weak.upgrade().is_none(), "bottom revealer")?;
    assert_finalized(status_weak.upgrade().is_none(), "status overlay")?;
    assert_finalized(overlay_weak.upgrade().is_none(), "media overlay")?;
    Ok(())
}

fn probe_background() -> gtk::Box {
    let background = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    background.update_property(&[gtk::accessible::Property::Label(
        "Media immersion background",
    )]);
    background.append(
        &gtk::Label::builder()
            .label("Media immersion probe")
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .vexpand(true)
            .build(),
    );
    background
}

fn probe_media_surface() -> gtk::Box {
    let media_surface = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .width_request(560)
        .height_request(300)
        .build();
    media_surface.update_property(&[gtk::accessible::Property::Label("Probe media surface")]);
    media_surface.append(&gtk::Label::new(Some("Probe media surface")));
    media_surface
}

fn probe_status_overlay() -> gtk::Box {
    let status_overlay = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(8)
        .build();
    status_overlay.update_property(&[gtk::accessible::Property::Label("Probe status overlay")]);
    status_overlay.append(&gtk::Label::new(Some("Probe status overlay")));
    status_overlay
}

fn probe_chrome() -> BigMediaChrome {
    BigMediaChrome::new(
        BigMediaMenuSpec {
            icon_name: "open-menu-symbolic".to_owned(),
            label: "Main Menu".to_owned(),
            items: vec![BigMenuActionItem::new("Open Media", "win.open-media")],
        },
        true,
    )
}

fn probe_bottom_controls() -> (gtk::Box, gtk::Button) {
    let controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::End)
        .spacing(8)
        .build();
    controls.update_property(&[gtk::accessible::Property::Label("Probe media controls")]);
    let play_button = build_button(
        &BigButtonContentSpec::new("media-playback-start-symbolic", "Play media")
            .accessible_label("Play media")
            .tooltip("Play media"),
    );
    controls.append(&play_button);
    (controls, play_button)
}

fn assert_media_chrome_state(
    chrome: &BigMediaChrome,
    status_overlay: &impl IsA<gtk::Widget>,
    expected_visible: bool,
    step: &str,
) -> Result<(), String> {
    let header_visible = chrome.header_revealer().reveals_child();
    let bottom_visible = chrome.bottom_revealer().reveals_child();
    if header_visible != expected_visible {
        return Err(format!(
            "{step}: expected header visible={expected_visible}, got {header_visible}"
        ));
    }
    if bottom_visible != expected_visible {
        return Err(format!(
            "{step}: expected bottom visible={expected_visible}, got {bottom_visible}"
        ));
    }

    let expected_opacity = if expected_visible { 1.0 } else { 0.0 };
    let actual_opacity = status_overlay.upcast_ref::<gtk::Widget>().opacity();
    if (actual_opacity - expected_opacity).abs() > f64::EPSILON {
        return Err(format!(
            "{step}: expected status opacity={expected_opacity}, got {actual_opacity}"
        ));
    }
    Ok(())
}

fn assert_finalized(is_finalized: bool, widget_name: &str) -> Result<(), String> {
    if is_finalized {
        Ok(())
    } else {
        Err(format!("{widget_name} survived teardown"))
    }
}

fn write_probe_result(contents: &str) {
    let Some(output_dir) =
        env::var_os("OUT_DIR").or_else(|| env::var_os("BIGLINUX_UI_EVIDENCE_DIR"))
    else {
        return;
    };
    let path = Path::new(&output_dir).join("probe-result.txt");
    let _ = fs::write(path, contents);
}

fn wait_for(duration: Duration) {
    let main_loop = gtk::glib::MainLoop::new(None, false);
    gtk::glib::timeout_add_local_once(duration, {
        let main_loop = main_loop.clone();
        move || main_loop.quit()
    });
    main_loop.run();
    pump();
}

fn pump() {
    let main_context = gtk::glib::MainContext::default();
    let mut guard = 0;
    while main_context.pending() && guard < 10_000 {
        main_context.iteration(false);
        guard += 1;
    }
}
