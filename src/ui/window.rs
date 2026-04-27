//! Main application window.
//!
//! Layout (top to bottom):
//!
//! ```text
//! ┌─ HeaderBar [Advanced ⏻]   (Mic | Output) ─ when Advanced ──────┐
//! ├──────────────────────────────────────────────────────────────────┤
//! │  Spectrum widget (peak meter + bars) — hidden on Output tab      │
//! ├──────────────────────────────────────────────────────────────────┤
//! │  Active view                                                     │
//! │   • Simple   → single page with both mic + system-sound cards    │
//! │   • Advanced → ViewStack with Microphone / Output filter tabs    │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! The spectrum reflects the microphone capture stream, so it stays
//! visible in Simple mode (which centres on the mic) and on the
//! Microphone tab in Advanced mode, but hides on the Output filter tab
//! where it would mislead users into reading speaker output levels.
//!
//! The "Advanced" header switch persists into
//! `AppSettings::ui::show_advanced` and rebuilds both the body and the
//! header title in place — no flicker, no window recreate.

use std::rc::Rc;

use adw::prelude::*;
use glib::MainContext;
use gtk::{gio, glib, Orientation};

use crate::config::{app_id, app_version, settings_file, AppSettings};
use crate::services::audio_monitor::{AudioMonitor, Event as MonitorEvent};

use super::i18n::i18n;
use super::state::AppState;
use super::views::{mic, output, simple, Mode};
use super::widgets::spectrum::Spectrum;

pub fn build(
    app: &adw::Application,
    state: Rc<AppState>,
    monitor: Rc<AudioMonitor>,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(i18n("Filter noise"))
        .default_width(state.settings().window.width.try_into().unwrap_or(720))
        .default_height(state.settings().window.height.try_into().unwrap_or(700))
        .build();

    let initial_mode = current_mode(&state);

    // ── Header ────────────────────────────────────────────────────────
    let header = adw::HeaderBar::new();
    header.set_decoration_layout(Some(":minimize,maximize,close"));

    let mode_picker = build_mode_picker(initial_mode);
    header.pack_start(&mode_picker.container);

    header.pack_end(&build_primary_menu_button());

    // The body container hosts whichever layout the current mode picks.
    let body = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .vexpand(true)
        .hexpand(true)
        .build();

    // ── Spectrum strip ────────────────────────────────────────────────
    let spectrum = Spectrum::new();
    bind_spectrum_to_monitor(&spectrum, &monitor);
    let spectrum_container = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .margin_top(12)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();
    spectrum_container.append(spectrum.widget());

    populate_body(&state, &header, &body, &spectrum_container, initial_mode);

    let root = gtk::Box::new(Orientation::Vertical, 0);
    root.append(&header);
    root.append(&spectrum_container);
    root.append(&body);

    window.set_content(Some(&root));

    // SAFETY: `set_data` keeps the value alive for the window's
    // lifetime under a unique key. The key `"biglinux-spectrum"` is
    // only read from this module, so there is no aliasing or type
    // mismatch risk.
    unsafe {
        window.set_data("biglinux-spectrum", spectrum);
    }

    install_window_actions(
        &window,
        Rc::clone(&state),
        header.clone(),
        body.clone(),
        spectrum_container.clone(),
    );

    // Rebuild body + header title whenever the mode flips.
    {
        let state = Rc::clone(&state);
        let header = header.clone();
        let body = body.clone();
        let spectrum_container = spectrum_container.clone();
        mode_picker.switch.connect_active_notify(move |sw| {
            let advanced = sw.is_active();
            state.mutate(|s| s.ui.show_advanced = advanced);
            populate_body(
                &state,
                &header,
                &body,
                &spectrum_container,
                Mode::from_advanced_flag(advanced),
            );
        });
    }

    install_external_settings_watch(
        &window,
        Rc::clone(&state),
        header.clone(),
        body.clone(),
        spectrum_container.clone(),
        mode_picker.switch.clone(),
    );

    {
        let state = Rc::clone(&state);
        let window_weak = window.downgrade();
        window.connect_close_request(move |_| {
            // Self-listen is a calibration aid only — the loopback must
            // not survive the configuration window. The mutation below
            // also persists `monitor.enabled = false` to settings, so
            // the next launch does not auto-open the loopback either.
            state.mutate(|s| {
                s.monitor.enabled = false;
                if let Some(w) = window_weak.upgrade() {
                    s.window.width = u32::try_from(w.default_width()).unwrap_or(720);
                    s.window.height = u32::try_from(w.default_height()).unwrap_or(700);
                }
            });
            state.flush();
            glib::Propagation::Proceed
        });
    }

    window
}

/// Replace the body contents and the header title widget so they match
/// the requested mode. Simple → no tab switcher, single combined page.
/// Advanced → tab switcher in the header, `ViewStack` in the body. The
/// `spectrum_container` is hidden whenever the active view does not
/// represent the microphone capture stream (currently: the Output
/// filter tab in Advanced mode).
fn populate_body(
    state: &Rc<AppState>,
    header: &adw::HeaderBar,
    body: &gtk::Box,
    spectrum_container: &gtk::Box,
    mode: Mode,
) {
    while let Some(child) = body.first_child() {
        body.remove(&child);
    }

    match mode {
        Mode::Simple => {
            header.set_title_widget(None::<&gtk::Widget>);
            spectrum_container.set_visible(true);
            body.append(&simple::build(state));
        }
        Mode::Advanced => {
            let stack = adw::ViewStack::new();
            stack.add_titled_with_icon(
                &mic::build(state),
                Some("mic"),
                &i18n("Microphone"),
                "audio-input-microphone-symbolic",
            );
            stack.add_titled_with_icon(
                &output::build(state),
                Some("output"),
                &i18n("Output filter"),
                "audio-headphones-symbolic",
            );
            stack.set_vexpand(true);

            let switcher = adw::ViewSwitcher::builder()
                .stack(&stack)
                .policy(adw::ViewSwitcherPolicy::Wide)
                .build();
            header.set_title_widget(Some(&switcher));

            sync_spectrum_visibility(spectrum_container, &stack);
            {
                let spectrum_container = spectrum_container.clone();
                stack.connect_visible_child_name_notify(move |stack| {
                    sync_spectrum_visibility(&spectrum_container, stack);
                });
            }

            body.append(&stack);
        }
    }
}

/// The spectrum reflects the mic capture path, so we hide it on any
/// stack page that isn't about the microphone (today: the `output`
/// page). Centralised here so both the initial render and the
/// `notify::visible-child-name` handler agree on the rule.
fn sync_spectrum_visibility(spectrum_container: &gtk::Box, stack: &adw::ViewStack) {
    let visible = stack
        .visible_child_name()
        .is_none_or(|name| name.as_str() != "output");
    spectrum_container.set_visible(visible);
}

/// `[label | switch]` packed at the start of the header bar. Off =
/// simplified single-page combined layout, on = full per-control layout
/// with tabs.
struct ModePicker {
    container: gtk::Box,
    switch: gtk::Switch,
}

fn build_mode_picker(initial: Mode) -> ModePicker {
    let container = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();

    let label = gtk::Label::builder().label(i18n("Advanced")).build();
    let switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .tooltip_text(i18n("Show every control individually"))
        .active(matches!(initial, Mode::Advanced))
        .build();

    container.append(&label);
    container.append(&switch);
    ModePicker { container, switch }
}

fn current_mode(state: &Rc<AppState>) -> Mode {
    Mode::from_advanced_flag(state.settings().ui.show_advanced)
}

/// Hamburger menu in the header. Holds the "Restore default settings"
/// and "About" entries — both routed through window-scoped GAction
/// instances installed by [`install_window_actions`].
fn build_primary_menu_button() -> gtk::MenuButton {
    let menu = gio::Menu::new();
    menu.append(
        Some(&i18n("Restore default settings")),
        Some("win.reset-defaults"),
    );
    menu.append(Some(&i18n("About Filter noise")), Some("win.about"));

    let button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text(i18n("Main menu"))
        .primary(true)
        .build();
    // Icon-only control needs an explicit accessible label; tooltips
    // alone are not surfaced as accessible names by every reader.
    button.update_property(&[gtk::accessible::Property::Label(&i18n("Main menu"))]);
    button
}

/// Wire the hamburger menu's GActions to the window. Window-scoped (not
/// app-scoped) so the dialogs can target the active window directly and
/// shut down with it.
fn install_window_actions(
    window: &adw::ApplicationWindow,
    state: Rc<AppState>,
    header: adw::HeaderBar,
    body: gtk::Box,
    spectrum_container: gtk::Box,
) {
    let reset_action = gio::SimpleAction::new("reset-defaults", None);
    {
        let state = Rc::clone(&state);
        let window_weak = window.downgrade();
        reset_action.connect_activate(move |_, _| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            present_reset_confirmation(&window, &state, &header, &body, &spectrum_container);
        });
    }
    window.add_action(&reset_action);

    let about_action = gio::SimpleAction::new("about", None);
    {
        let window_weak = window.downgrade();
        about_action.connect_activate(move |_, _| {
            if let Some(window) = window_weak.upgrade() {
                present_about_dialog(&window);
            }
        });
    }
    window.add_action(&about_action);
}

/// Confirm before overwriting the user's audio configuration. Window
/// geometry (`window`) and UI preferences (`ui`) are preserved so the
/// reset doesn't snap the window back to default size or flip the
/// Simple/Advanced toggle the user already chose.
fn present_reset_confirmation(
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    header: &adw::HeaderBar,
    body: &gtk::Box,
    spectrum_container: &gtk::Box,
) {
    let dialog = adw::AlertDialog::new(
        Some(&i18n("Restore default settings?")),
        Some(&i18n(
            "All audio processing settings will return to their factory values. \
             Your window size and view preferences are kept.",
        )),
    );
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response("reset", &i18n("Restore defaults"));
    dialog.set_response_appearance("reset", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    let state = Rc::clone(state);
    let header = header.clone();
    let body = body.clone();
    let spectrum_container = spectrum_container.clone();
    dialog.connect_response(None, move |dialog, response| {
        if response == "reset" {
            apply_factory_defaults(&state);
            populate_body(
                &state,
                &header,
                &body,
                &spectrum_container,
                current_mode(&state),
            );
        }
        dialog.close();
    });

    dialog.present(Some(window));
}

/// Replace the current settings with [`AppSettings::default`] while
/// keeping window geometry and UI preferences intact.
fn apply_factory_defaults(state: &Rc<AppState>) {
    state.mutate(|s| {
        let preserved_window = s.window.clone();
        let preserved_ui = s.ui.clone();
        *s = AppSettings::default();
        s.window = preserved_window;
        s.ui = preserved_ui;
    });
}

fn present_about_dialog(parent: &adw::ApplicationWindow) {
    let about = adw::AboutDialog::builder()
        .application_name(i18n("Filter noise"))
        .application_icon(app_id())
        .version(app_version())
        .developer_name("BigLinux Team")
        .website("https://github.com/biglinux/biglinux-noise-reduction-pipewire")
        .issue_url("https://github.com/biglinux/biglinux-noise-reduction-pipewire/issues")
        .license_type(gtk::License::Gpl30)
        .copyright("© 2026 BigLinux Team")
        .comments(i18n(
            "Real-time noise filter for microphone capture and system audio playback. \
             Powered by the GTCRN neural network running as a LADSPA plugin on top of \
             PipeWire — every filter is driven by a WirePlumber smart-filter so the \
             default source and sink stay clean even after device changes.",
        ))
        .build();
    about.add_credit_section(
        Some(&i18n("Built on")),
        &[
            "PipeWire https://pipewire.org",
            "WirePlumber https://pipewire.pages.freedesktop.org/wireplumber/",
            "GTCRN (neural denoiser) https://github.com/Xiaobin-Rong/gtcrn",
            "GTK4 / libadwaita https://gitlab.gnome.org/GNOME/libadwaita",
        ],
    );
    about.present(Some(parent));
}

/// Watch `settings.json` and reflect changes coming from outside this
/// process (CLI, plasmoid, manual edit). Cheap by design: a single
/// `gio::FileMonitor` re-reads the file on each event and only rebuilds
/// the body when the loaded snapshot differs from the in-memory one —
/// so our own atomic-rename writes round-trip into a no-op compare. The
/// monitor stashes itself on the window via `set_data` so it lives as
/// long as the window does.
fn install_external_settings_watch(
    window: &adw::ApplicationWindow,
    state: Rc<AppState>,
    header: adw::HeaderBar,
    body: gtk::Box,
    spectrum_container: gtk::Box,
    mode_switch: gtk::Switch,
) {
    let path = settings_file();
    let file = gio::File::for_path(&path);
    let monitor =
        match file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("settings file monitor unavailable: {e}");
                return;
            }
        };

    monitor.connect_changed(move |_, _, _, event| {
        if !matches!(
            event,
            gio::FileMonitorEvent::Changed
                | gio::FileMonitorEvent::ChangesDoneHint
                | gio::FileMonitorEvent::Created
                | gio::FileMonitorEvent::Renamed
                | gio::FileMonitorEvent::MovedIn
        ) {
            return;
        }
        let new = AppSettings::load();
        if !state.external_replace(new) {
            return;
        }
        let new_advanced = state.settings().ui.show_advanced;
        if mode_switch.is_active() == new_advanced {
            populate_body(
                &state,
                &header,
                &body,
                &spectrum_container,
                Mode::from_advanced_flag(new_advanced),
            );
        } else {
            // Handler on the switch will run populate_body for us.
            mode_switch.set_active(new_advanced);
        }
    });

    // SAFETY: unique key, only read here. Keeps the FileMonitor alive
    // for the window's lifetime — dropping it would silence the watch.
    unsafe {
        window.set_data("biglinux-settings-monitor", monitor);
    }
}

/// Wire the audio-monitor event stream into the spectrum widget. Runs on
/// the GTK main context so every update stays on the UI thread.
fn bind_spectrum_to_monitor(spectrum: &Rc<Spectrum>, monitor: &Rc<AudioMonitor>) {
    let events = monitor.events();
    let spectrum = Rc::clone(spectrum);
    MainContext::default().spawn_local(async move {
        while let Ok(evt) = events.recv().await {
            match evt {
                MonitorEvent::Frame(frame) => spectrum.push_frame(&frame),
                MonitorEvent::Fatal(_) => break,
            }
        }
    });
}
