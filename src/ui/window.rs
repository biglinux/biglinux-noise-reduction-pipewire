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
use big_app_kit::{desktop, dialogs};
use big_relm4_components::feedback::tooltip;
use big_relm4_components::layout::hamburger_menu::{BigHamburgerMenuSpec, BigMenuActionItem};
use glib::MainContext;
use gtk::{glib, Orientation};
use relm4::{Component, ComponentController};

use crate::config::{app_id, app_version, AppSettings};
use crate::services::audio_monitor::{AudioMonitor, Event as MonitorEvent};

use super::i18n::i18n;
use super::mic_shell::{MicInput, MicShell, MicShellInit};
use super::state::AppState;
use super::views::{advanced, mic, output, simple, Mode};
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
    // Opt in to optional Big Gnome Center background styling.
    window.add_css_class("biglinux-microphone");

    // The window content is a mountable Relm4 component (ADR-D14): it owns the
    // header, spectrum strip, body, the Advanced toggle, and the external
    // `settings.json` watch. The window-scoped concerns stay here because they
    // need the `adw::ApplicationWindow` itself — the menu GActions (their
    // dialogs target the window) and the close handler (geometry persistence +
    // flush).
    let controller = MicShell::builder()
        .launch(MicShellInit {
            state: Rc::clone(&state),
            monitor,
        })
        .detach();
    window.set_content(Some(controller.widget()));

    install_window_actions(&window, Rc::clone(&state), controller.sender().clone());

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

    // Park the controller for the window's lifetime: the component — and with
    // it the widget tree, spectrum, monitor binding, and settings watch — drops
    // when the window finalizes on close.
    // SAFETY: unique key, only written here, never read back; keeps the
    // controller alive exactly as long as the window.
    unsafe {
        window.set_data("biglinux-mic-shell", controller);
    }

    window
}

/// Replace the body contents and the header title widget so they match
/// the requested mode. Simple → no tab switcher, single combined page.
/// Advanced → tab switcher in the header, `ViewStack` in the body. The
/// `spectrum_container` is hidden whenever the active view does not
/// represent the microphone capture stream (currently: the Output
/// filter tab in Advanced mode).
pub(super) fn populate_body(
    state: &Rc<AppState>,
    input: &relm4::Sender<MicInput>,
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
            body.append(&simple::build(state, input));
        }
        Mode::Advanced => {
            let stack = adw::ViewStack::new();
            stack.add_titled_with_icon(
                &mic::build(state, input),
                Some("mic"),
                &i18n("Microphone"),
                "audio-input-microphone-symbolic",
            );
            stack.add_titled_with_icon(
                &output::build(state, input),
                Some("output"),
                &i18n("Output filter"),
                "audio-headphones-symbolic",
            );
            stack.add_titled_with_icon(
                &advanced::build(),
                Some("tuning"),
                &i18n("Tuning"),
                "applications-system-symbolic",
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
pub(in crate::ui) fn sync_spectrum_visibility(
    spectrum_container: &gtk::Box,
    stack: &adw::ViewStack,
) {
    let visible = stack.visible_child_name().is_none_or(|name| {
        let n = name.as_str();
        n != "output" && n != "tuning"
    });
    spectrum_container.set_visible(visible);
}

/// `[label | switch]` packed at the start of the header bar. Off =
/// simplified single-page combined layout, on = full per-control layout
/// with tabs.
pub(super) struct ModePicker {
    pub(super) container: gtk::Box,
    pub(super) switch: gtk::Switch,
}

pub(super) fn build_mode_picker(initial: Mode) -> ModePicker {
    let container = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();

    let label = gtk::Label::builder().label(i18n("Advanced")).build();
    let switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(matches!(initial, Mode::Advanced))
        .build();
    tooltip::set(&switch, &i18n("Show every control individually"));
    switch.update_property(&[gtk::accessible::Property::Label(&i18n("Advanced"))]);

    container.append(&label);
    container.append(&switch);
    ModePicker { container, switch }
}

pub(super) fn current_mode(state: &Rc<AppState>) -> Mode {
    Mode::from_advanced_flag(state.settings().ui.show_advanced)
}

/// Hamburger menu spec in the header. Holds the "Restore default settings"
/// and "About" entries — both routed through window-scoped GAction
/// instances installed by [`install_window_actions`].
pub(super) fn primary_menu_spec() -> BigHamburgerMenuSpec {
    BigHamburgerMenuSpec::new(i18n(PRIMARY_MENU_LABEL)).items(
        primary_menu_action_specs()
            .map(|(label, action)| BigMenuActionItem::new(i18n(label), action)),
    )
}

const PRIMARY_MENU_LABEL: &str = "Main menu";

fn primary_menu_action_specs() -> [(&'static str, &'static str); 2] {
    [
        ("Restore default settings", "win.reset-defaults"),
        ("About Filter noise", "win.about"),
    ]
}

/// Wire the hamburger menu's GActions to the window. Window-scoped (not
/// app-scoped) so the dialogs can target the active window directly and
/// shut down with it.
fn install_window_actions(
    window: &adw::ApplicationWindow,
    state: Rc<AppState>,
    shell: relm4::Sender<MicInput>,
) {
    // Cataloged shared action installer (`big_app_kit::desktop`) instead of raw
    // `gio::SimpleAction`. Weak window captures: the exported actions outlive
    // widget teardown, so strong refs would leak the window after close. The
    // reset body-rebuild goes through the shell sender (the component owns the
    // widgets), so no header/body/spectrum captures are needed.
    {
        let state = Rc::clone(&state);
        let window_weak = window.downgrade();
        desktop::install_action(window, "reset-defaults", move || {
            if let Some(window) = window_weak.upgrade() {
                let dialog = reset_confirmation_dialog();
                let state = Rc::clone(&state);
                let shell = shell.clone();
                dialog.connect_response(None, move |dialog, response| {
                    if is_reset_response(response) {
                        apply_factory_defaults(&state);
                        // The component owns the body widgets; ask it to rebuild for the
                        // (geometry/UI-preserving) factory snapshot just written to state.
                        let _ = shell.send(MicInput::RebuildBody);
                    }
                    dialog.close();
                });
                dialog.present(Some(&window));
            }
        });
    }
    {
        let window_weak = window.downgrade();
        desktop::install_action(window, "about", move || {
            if let Some(window) = window_weak.upgrade() {
                present_about_dialog(&window);
            }
        });
    }
}

#[cfg(test)]
pub(in crate::ui) fn install_window_actions_contract(
    window: &adw::ApplicationWindow,
    state: Rc<AppState>,
) -> relm4::Receiver<MicInput> {
    let (sender, receiver) = relm4::channel();
    install_window_actions(window, state, sender);
    receiver
}

fn is_reset_response(response: &str) -> bool {
    response == "reset"
}

/// Confirm before overwriting the user's audio configuration.
pub(in crate::ui) fn reset_confirmation_dialog() -> adw::AlertDialog {
    dialogs::confirm_dialog(
        &i18n("Restore default settings?"),
        &i18n(
            "All audio processing settings will return to their factory values. \
             Your window size and view preferences are kept.",
        ),
        &i18n("Cancel"),
        "reset",
        &i18n("Restore defaults"),
        true,
    )
}

/// Replace the current settings with [`AppSettings::default`] while
/// keeping window geometry and UI preferences intact.
pub(in crate::ui) fn apply_factory_defaults(state: &Rc<AppState>) {
    state.mutate(|s| {
        let preserved_window = s.window.clone();
        let preserved_ui = s.ui.clone();
        *s = AppSettings::default();
        s.window = preserved_window;
        s.ui = preserved_ui;
    });
}

fn present_about_dialog(parent: &adw::ApplicationWindow) -> adw::AboutDialog {
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
    about
}

#[cfg(test)]
pub(in crate::ui) fn present_about_dialog_contract(
    parent: &adw::ApplicationWindow,
) -> adw::AboutDialog {
    present_about_dialog(parent)
}

/// Wire the audio-monitor event stream into the spectrum widget. Runs on
/// the GTK main context so every update stays on the UI thread.
pub(super) fn bind_spectrum_to_monitor(spectrum: &Rc<Spectrum>, monitor: &Rc<AudioMonitor>) {
    let events = monitor.events();
    // Weak: a strong capture here is a teardown deadlock — the loop keeps
    // the spectrum alive until the channel closes, the channel only closes
    // when the monitor drops, and the monitor is held by closures on the
    // spectrum's own widget. Embedded (window-close) teardown never breaks
    // that cycle; standalone never noticed because it exits the process.
    let spectrum = Rc::downgrade(spectrum);
    MainContext::default().spawn_local(async move {
        while let Ok(evt) = events.recv().await {
            let Some(spectrum) = spectrum.upgrade() else {
                break;
            };
            match evt {
                MonitorEvent::Frame(frame) => spectrum.push_frame(&frame),
                MonitorEvent::Fatal(_) => break,
            }
        }
    });
}

/// Pause the audio-monitor worker (and back-pressure pw-cat into idle)
/// whenever the spectrum widget is not visible on screen — different
/// stack page in Advanced mode, window minimised, parent window closed.
/// `map`/`unmap` cover both cases without us having to listen on the
/// `GtkWindow` itself, since GTK4 unmaps every descendant of an
/// invisible toplevel.
pub(super) fn bind_monitor_to_spectrum_visibility(
    spectrum: &Rc<Spectrum>,
    monitor: &Rc<AudioMonitor>,
) {
    let widget = spectrum.widget().clone();
    // Weak: these closures live on the spectrum's widget — strong monitor
    // refs here prevent the embed guard's `Rc::try_unwrap` teardown.
    let monitor_for_map = Rc::downgrade(monitor);
    widget.connect_map(move |_| {
        if let Some(monitor) = monitor_for_map.upgrade() {
            monitor.set_active(true);
        }
    });
    let monitor_for_unmap = Rc::downgrade(monitor);
    widget.connect_unmap(move |_| {
        if let Some(monitor) = monitor_for_unmap.upgrade() {
            monitor.set_active(false);
        }
    });
    // Initial state: a freshly built widget is not yet mapped, so the
    // worker stays active until GTK realises the strip — at which point
    // the `map` signal fires. Pause now so the brief startup window
    // does not pump the FFT for a hidden widget on Advanced/Output mode.
    if should_pause_monitor_for_initial_mapping(widget.is_mapped()) {
        monitor.set_active(false);
    }
}

fn should_pause_monitor_for_initial_mapping(is_mapped: bool) -> bool {
    !is_mapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_menu_actions_use_accessible_contract() {
        assert_eq!(PRIMARY_MENU_LABEL, "Main menu");
        assert_eq!(
            primary_menu_action_specs(),
            [
                ("Restore default settings", "win.reset-defaults"),
                ("About Filter noise", "win.about"),
            ]
        );
    }

    #[test]
    #[cfg(not(miri))]
    fn localized_primary_menu_spec_uses_accessible_main_menu_contract() {
        let spec = primary_menu_spec();

        assert_eq!(spec.icon_name, "open-menu-symbolic");
        assert_eq!(spec.label, "Main menu");
        assert_eq!(spec.items.len(), 2);
        assert_eq!(spec.items[0].label, "Restore default settings");
        assert_eq!(spec.items[0].action, "win.reset-defaults");
        assert_eq!(spec.items[1].label, "About Filter noise");
        assert_eq!(spec.items[1].action, "win.about");
    }

    #[test]
    fn reset_response_only_accepts_reset_id() {
        assert!(is_reset_response("reset"));
        assert!(!is_reset_response("cancel"));
        assert!(!is_reset_response(""));
    }

    #[test]
    fn initial_monitor_pause_tracks_mapping_state() {
        assert!(should_pause_monitor_for_initial_mapping(false));
        assert!(!should_pause_monitor_for_initial_mapping(true));
    }
}
