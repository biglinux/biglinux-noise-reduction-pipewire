//! Advanced-mode "Tuning" page.
//!
//! Lets the user override the BigLinux distro defaults for PipeWire and
//! WirePlumber on a per-control basis without leaving the GUI. Every
//! card hosts a single dropdown whose first entry is the sentinel
//! `Distribution default` — that selection wipes the corresponding
//! line from the user drop-in so the layered defaults shipped by
//! `pipewire-biglinux-config` (and the package's own
//! `61-biglinux-alsa-headroom.conf`) take over again.
//!
//! ## Apply pipeline
//!
//! 1. Capture the current dropdown selections into a [`UserTweaks`]
//!    snapshot.
//! 2. Persist via [`UserTweaks::apply`] (atomic rename per drop-in).
//! 3. Bounce the user-scoped audio stack via
//!    [`crate::services::system_audio::restart_pipewire_user_stack`]
//!    on a worker thread so the UI thread stays responsive.
//! 4. Surface success / failure through an [`adw::Banner`] above the
//!    cards.
//!
//! ## Plain-language UI
//!
//! Card titles and bodies stay free of jargon (no "frames", "ms",
//! "kHz"). Dropdown labels keep a short technical hint in parentheses
//! for users who already know what the knob does, but the description
//! always answers "what changes for me?" first.
//!
//! ## Reset
//!
//! The "Restore defaults" entry opens an [`adw::AlertDialog`] before
//! deleting both drop-ins. We never auto-clear on app shutdown — the
//! user file is meant to outlive the GUI session.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::dialogs;
use gtk::{gio, glib, Box as GtkBox, Orientation, ScrolledWindow};

use crate::services::pipewire::user_tweaks::{SampleRates, UserTweaks};
use crate::services::system_audio::restart_pipewire_user_stack;

use super::super::i18n::i18n;
use super::super::widgets::didactic::{labelled_row, section_header, DidacticCard};

/// Per-control selection, decoupled from the on-disk shape so the UI
/// can offer a richer set of curated defaults than what raw integers
/// allow. The sentinel `None` always maps to "Distribution default".
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Selection {
    quantum: Option<u32>,
    headroom_usb: Option<u32>,
    headroom_pci: Option<u32>,
    bt_latency: Option<u32>,
    sample_rates: Option<SampleRates>,
    bt_sbc_xq: Option<bool>,
    bt_call_autoswitch: Option<bool>,
    alsa_no_suspend: Option<bool>,
}

impl Selection {
    fn from_disk(t: &UserTweaks) -> Self {
        Self {
            quantum: t.quantum,
            headroom_usb: t.headroom_usb,
            headroom_pci: t.headroom_pci,
            bt_latency: t.bt_latency,
            sample_rates: t.sample_rates,
            bt_sbc_xq: t.bt_sbc_xq,
            bt_call_autoswitch: t.bt_call_autoswitch,
            alsa_no_suspend: t.alsa_no_suspend,
        }
    }

    fn into_tweaks(self) -> UserTweaks {
        UserTweaks {
            quantum: self.quantum,
            headroom_usb: self.headroom_usb,
            headroom_pci: self.headroom_pci,
            bt_latency: self.bt_latency,
            sample_rates: self.sample_rates,
            bt_sbc_xq: self.bt_sbc_xq,
            bt_call_autoswitch: self.bt_call_autoswitch,
            alsa_no_suspend: self.alsa_no_suspend,
        }
    }

    fn is_modified(self) -> bool {
        self.quantum.is_some()
            || self.headroom_usb.is_some()
            || self.headroom_pci.is_some()
            || self.bt_latency.is_some()
            || self.sample_rates.is_some()
            || self.bt_sbc_xq.is_some()
            || self.bt_call_autoswitch.is_some()
            || self.alsa_no_suspend.is_some()
    }
}

/// Build the Tuning page. Returns a scrollable container ready to be
/// added to the [`adw::ViewStack`] in `views::window::populate_body`.
pub fn build() -> gtk::Widget {
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .build();

    let content = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    scroll.set_child(Some(&content));

    // Read the on-disk pipewire/wireplumber tweaks off the main loop so a hung
    // or slow $HOME can't block the Tuning tab from opening; the cards fill in
    // once the read lands (empty until then).
    let content_weak = content.downgrade();
    glib::spawn_future_local(async move {
        let tweaks = gio::spawn_blocking(UserTweaks::load_from_disk)
            .await
            .unwrap_or_default();
        if let Some(content) = content_weak.upgrade() {
            populate_tuning_page(&content, Selection::from_disk(&tweaks));
        }
    });
    scroll.upcast()
}

/// Build the Tuning page cards from the already-loaded on-disk selection. Split
/// from [`build`] so the disk read can run off the main loop first.
fn populate_tuning_page(content: &GtkBox, initial: Selection) {
    let selection = Rc::new(RefCell::new(initial));

    let banner = adw::Banner::builder()
        .title(i18n("The standard audio settings are in use."))
        .revealed(false)
        .build();
    content.append(&banner);

    let intro = gtk::Label::builder()
        .label(i18n(
            "Adjust how the system drives your audio hardware. Each \
             card starts on the default we tested across a wide range \
             of machines — change a value only if you have a specific \
             reason to.",
        ))
        .wrap(true)
        .xalign(0.0)
        .margin_top(8)
        .margin_bottom(8)
        .css_classes(vec!["dim-label"])
        .build();
    content.append(&intro);

    let toolbar = action_toolbar();
    content.append(&toolbar.row);

    // ── Bluetooth ────────────────────────────────────────────────────
    content.append(&section_header(&i18n("Bluetooth"), 8));
    content.append(bt_call_card(&selection, &banner).widget());
    content.append(bt_latency_card(&selection, &banner).widget());
    content.append(bt_codec_card(&selection, &banner).widget());

    // ── Estabilidade e desempenho ────────────────────────────────────
    content.append(&section_header(&i18n("Stability and performance"), 16));
    content.append(quantum_card(&selection, &banner).widget());
    content.append(headroom_usb_card(&selection, &banner).widget());
    content.append(headroom_pci_card(&selection, &banner).widget());
    content.append(alsa_suspend_card(&selection, &banner).widget());

    // ── Qualidade de som ─────────────────────────────────────────────
    content.append(&section_header(&i18n("Sound quality"), 16));
    content.append(sample_rates_card(&selection, &banner).widget());

    refresh_banner(&banner, &selection);

    {
        let selection = Rc::clone(&selection);
        let banner = banner.clone();
        toolbar.apply.connect_clicked(move |btn| {
            apply_clicked(btn, &selection, &banner);
        });
    }
    {
        let selection = Rc::clone(&selection);
        let banner = banner.clone();
        toolbar.reset.connect_clicked(move |btn| {
            let dialog = reset_audio_settings_dialog();
            let selection = Rc::clone(&selection);
            let banner = banner.clone();
            let button_weak = btn.downgrade();
            dialog.connect_response(None, move |dlg, response| {
                if is_reset_response(response) {
                    *selection.borrow_mut() = Selection::default();
                    refresh_banner(&banner, &selection);
                    // No separate synchronous `UserTweaks::clear()`: applying the
                    // now-default selection renders empty drop-ins, and
                    // `UserTweaks::apply` deletes them (same two files, same
                    // order) — but on the worker thread, with the restart, so the
                    // reset no longer fsyncs/removes on the main loop.
                    if let Some(button) = button_weak.upgrade() {
                        apply_clicked(&button, &selection, &banner);
                    }
                }
                dlg.close();
            });
            dialog.present(Some(btn));
        });
    }
}

fn is_reset_response(response: &str) -> bool {
    response == "reset"
}

struct ActionToolbar {
    row: GtkBox,
    apply: gtk::Button,
    reset: gtk::Button,
}

fn action_toolbar() -> ActionToolbar {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(12)
        .halign(gtk::Align::End)
        .build();

    let reset = gtk::Button::builder()
        .label(i18n("Restore defaults"))
        .css_classes(vec!["destructive-action"])
        .build();

    let apply = gtk::Button::builder()
        .label(i18n("Apply and restart audio"))
        .css_classes(vec!["suggested-action"])
        .build();

    row.append(&reset);
    row.append(&apply);

    ActionToolbar { row, apply, reset }
}

fn refresh_banner(banner: &adw::Banner, selection: &Rc<RefCell<Selection>>) {
    let modified = selection.borrow().is_modified();
    banner.set_title(&banner_title_for_modified_state(modified));
    banner.set_revealed(modified);
}

fn banner_title_for_modified_state(is_modified: bool) -> String {
    i18n(banner_title_message_for_modified_state(is_modified))
}

fn banner_title_message_for_modified_state(is_modified: bool) -> &'static str {
    if is_modified {
        "Custom settings active. Click Apply to enable them."
    } else {
        "The standard audio settings are in use."
    }
}

// ── Bluetooth cards ──────────────────────────────────────────────────

fn bt_call_card(selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) -> DidacticCard {
    let card = DidacticCard::new(
        "bluetooth_call.svg",
        &i18n("Bluetooth call mode"),
        &i18n(
            "When you start a video or voice call, your Bluetooth \
             headset can switch to a special call mode that turns on \
             the microphone — but the sound becomes mono and lower \
             quality, like an old phone. This is off by default so \
             music and movies stay in full quality even when an app \
             asks for the microphone. Turn it on if you rely on the \
             headset's microphone for calls.",
        ),
        None,
    );
    let options = vec![
        TweakOption::distro(),
        TweakOption::value(i18n("On — switch to call mode automatically"), true),
        TweakOption::value(i18n("Off — always keep stereo sound (default)"), false),
    ];
    let dropdown = build_dropdown(options, selection.borrow().bt_call_autoswitch, {
        let selection = Rc::clone(selection);
        let banner = banner.clone();
        move |picked: Option<bool>| {
            selection.borrow_mut().bt_call_autoswitch = picked;
            refresh_banner(&banner, &selection);
        }
    });
    card.add_row(&labelled_row(&i18n("Behaviour"), &dropdown));
    card
}

fn bt_latency_card(selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) -> DidacticCard {
    let card = DidacticCard::new(
        "bluetooth_latency.svg",
        &i18n("Bluetooth audio buffer"),
        &i18n(
            "Bluetooth needs a small audio reserve so the sound plays \
             smoothly even when the wireless connection has tiny \
             stutters. A bigger reserve hides hiccups but adds delay \
             between picture and sound — keep the default unless your \
             headset stutters or you need quicker audio.",
        ),
        None,
    );
    let options = vec![
        TweakOption::distro(),
        TweakOption::value(
            i18n("Small (low delay — needs a stable headset, ~11 ms)"),
            512_u32,
        ),
        TweakOption::value(i18n("Medium (~21 ms)"), 1024_u32),
        TweakOption::value(i18n("Standard (default, ~42 ms)"), 2048_u32),
        TweakOption::value(i18n("Large (steadiest, ~85 ms)"), 4096_u32),
    ];
    let dropdown = build_dropdown(options, selection.borrow().bt_latency, {
        let selection = Rc::clone(selection);
        let banner = banner.clone();
        move |picked: Option<u32>| {
            selection.borrow_mut().bt_latency = picked;
            refresh_banner(&banner, &selection);
        }
    });
    card.add_row(&labelled_row(&i18n("Buffer size"), &dropdown));
    card
}

fn bt_codec_card(selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) -> DidacticCard {
    let card = DidacticCard::new(
        "bluetooth_codec.svg",
        &i18n("High-quality Bluetooth audio (SBC-XQ)"),
        &i18n(
            "Your Bluetooth devices already use the best sound mode \
             they support. Force this on if a headset stubbornly \
             chooses lower quality, or off if you have a very old \
             device that breaks with the high-quality mode.",
        ),
        None,
    );
    let options = vec![
        TweakOption::distro(),
        TweakOption::value(i18n("Force on (high quality)"), true),
        TweakOption::value(i18n("Force off (maximum compatibility)"), false),
    ];
    let dropdown = build_dropdown(options, selection.borrow().bt_sbc_xq, {
        let selection = Rc::clone(selection);
        let banner = banner.clone();
        move |picked: Option<bool>| {
            selection.borrow_mut().bt_sbc_xq = picked;
            refresh_banner(&banner, &selection);
        }
    });
    card.add_row(&labelled_row(&i18n("SBC-XQ"), &dropdown));
    card
}

// ── Stability / performance cards ────────────────────────────────────

fn quantum_card(selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) -> DidacticCard {
    let card = DidacticCard::new(
        "quantum.svg",
        &i18n("Audio responsiveness"),
        &i18n(
            "How fast the system reacts to sound changes. Faster \
             reaction is great for music production but uses more CPU; \
             slower reaction makes playback steadier on weaker \
             machines. The default works well for video calls, music \
             and games on most computers.",
        ),
        None,
    );
    let options = vec![
        TweakOption::distro(),
        TweakOption::value(i18n("Very fast (music production, ~5 ms)"), 256_u32),
        TweakOption::value(i18n("Fast (low delay, ~11 ms)"), 512_u32),
        TweakOption::value(
            i18n("Balanced (upstream PipeWire default, ~21 ms)"),
            1024_u32,
        ),
        TweakOption::value(i18n("Steady (default, ~42 ms)"), 2048_u32),
        TweakOption::value(
            i18n("Most stable (older or busy machines, ~85 ms)"),
            4096_u32,
        ),
    ];
    let dropdown = build_dropdown(options, selection.borrow().quantum, {
        let selection = Rc::clone(selection);
        let banner = banner.clone();
        move |picked: Option<u32>| {
            selection.borrow_mut().quantum = picked;
            refresh_banner(&banner, &selection);
        }
    });
    card.add_row(&labelled_row(&i18n("Reaction speed"), &dropdown));
    card
}

fn headroom_usb_card(selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) -> DidacticCard {
    let card = DidacticCard::new(
        "headroom_usb.svg",
        &i18n("USB audio safety margin"),
        &i18n(
            "A small extra reserve for USB headsets, microphones and \
             external audio interfaces. Increase only if you hear \
             tiny cuts or pops on a USB device when the computer is \
             busy.",
        ),
        None,
    );
    let dropdown = build_dropdown(headroom_options(), selection.borrow().headroom_usb, {
        let selection = Rc::clone(selection);
        let banner = banner.clone();
        move |picked: Option<u32>| {
            selection.borrow_mut().headroom_usb = picked;
            refresh_banner(&banner, &selection);
        }
    });
    card.add_row(&labelled_row(&i18n("Margin"), &dropdown));
    card
}

fn headroom_pci_card(selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) -> DidacticCard {
    let card = DidacticCard::new(
        "headroom_pci.svg",
        &i18n("Built-in audio safety margin"),
        &i18n(
            "Same idea as the USB margin, but for the speakers and \
             microphone built into your laptop or motherboard. Most \
             people never need to change this — try it only if the \
             internal speaker glitches when the computer is under \
             load.",
        ),
        None,
    );
    let dropdown = build_dropdown(headroom_options(), selection.borrow().headroom_pci, {
        let selection = Rc::clone(selection);
        let banner = banner.clone();
        move |picked: Option<u32>| {
            selection.borrow_mut().headroom_pci = picked;
            refresh_banner(&banner, &selection);
        }
    });
    card.add_row(&labelled_row(&i18n("Margin"), &dropdown));
    card
}

fn alsa_suspend_card(selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) -> DidacticCard {
    let card = DidacticCard::new(
        "alsa_suspend.svg",
        &i18n("Avoid the click at the start of sounds"),
        &i18n(
            "Some laptops and motherboards put the speakers to sleep \
             after a few seconds of silence to save power. When the \
             next sound arrives — a notification, a video starting — \
             you may hear a small click or miss the very first part of \
             the audio. Turn this on to keep the speakers always \
             ready. The extra battery use is tiny.",
        ),
        None,
    );
    let options = vec![
        TweakOption::distro(),
        TweakOption::value(
            i18n("Always keep the speakers ready (recommended on desktops)"),
            true,
        ),
        TweakOption::value(i18n("Let the speakers sleep when idle (default)"), false),
    ];
    let dropdown = build_dropdown(options, selection.borrow().alsa_no_suspend, {
        let selection = Rc::clone(selection);
        let banner = banner.clone();
        move |picked: Option<bool>| {
            selection.borrow_mut().alsa_no_suspend = picked;
            refresh_banner(&banner, &selection);
        }
    });
    card.add_row(&labelled_row(&i18n("Speaker sleep"), &dropdown));
    card
}

// ── Sound-quality cards ──────────────────────────────────────────────

fn sample_rates_card(selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) -> DidacticCard {
    let card = DidacticCard::new(
        "sample_rates.svg",
        &i18n("Sound quality range"),
        &i18n(
            "All audio runs at studio quality (48 kHz) by default. \
             Hi-res music players and high-end USB DACs can play \
             higher quality if you allow it here, but every change \
             between qualities causes a brief audio reload. Keep the \
             default unless you actually own hi-res equipment.",
        ),
        None,
    );
    let options = vec![
        TweakOption::distro(),
        TweakOption::value(
            i18n("Studio quality only (default, 48 kHz)"),
            SampleRates::Standard,
        ),
        TweakOption::value(i18n("CD + studio quality (44.1 + 48 kHz)"), SampleRates::Cd),
        TweakOption::value(
            i18n("Allow hi-res audio (44.1 / 48 / 88.2 / 96 / 176.4 / 192 kHz)"),
            SampleRates::HiRes,
        ),
    ];
    let dropdown = build_dropdown(options, selection.borrow().sample_rates, {
        let selection = Rc::clone(selection);
        let banner = banner.clone();
        move |picked: Option<SampleRates>| {
            selection.borrow_mut().sample_rates = picked;
            refresh_banner(&banner, &selection);
        }
    });
    card.add_row(&labelled_row(&i18n("Allowed quality"), &dropdown));
    card
}

// ── Dropdown helper ──────────────────────────────────────────────────

/// One row in the dropdown. `value = None` is the sentinel meaning
/// "Distribution default".
struct TweakOption<T: Copy + 'static> {
    label: String,
    value: Option<T>,
}

impl<T: Copy + 'static> TweakOption<T> {
    fn distro() -> Self {
        Self {
            label: i18n("Default"),
            value: None,
        }
    }

    fn value(label: String, value: T) -> Self {
        Self {
            label,
            value: Some(value),
        }
    }
}

fn headroom_options() -> Vec<TweakOption<u32>> {
    vec![
        TweakOption::distro(),
        TweakOption::value(i18n("None"), 0_u32),
        TweakOption::value(i18n("Small (~5 ms)"), 256_u32),
        TweakOption::value(i18n("Medium (~11 ms)"), 512_u32),
        TweakOption::value(i18n("Standard (default, ~21 ms)"), 1024_u32),
        TweakOption::value(i18n("Large (virtual machines, ~42 ms)"), 2048_u32),
        TweakOption::value(i18n("Maximum (last resort, ~85 ms)"), 4096_u32),
    ]
}

fn build_dropdown<T, F>(
    options: Vec<TweakOption<T>>,
    initial: Option<T>,
    on_pick: F,
) -> gtk::DropDown
where
    T: Copy + PartialEq + 'static,
    F: Fn(Option<T>) + 'static,
{
    let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
    let model = gtk::StringList::new(&labels);
    let dropdown = gtk::DropDown::builder().model(&model).build();

    let initial_idx = options.iter().position(|o| o.value == initial).unwrap_or(0);
    dropdown.set_selected(initial_idx as u32);

    let values: Vec<Option<T>> = options.into_iter().map(|o| o.value).collect();
    dropdown.connect_selected_notify(move |dd| {
        let idx = dd.selected() as usize;
        if let Some(v) = values.get(idx).copied() {
            on_pick(v);
        }
    });

    dropdown
}

#[cfg(test)]
pub(in crate::ui) fn build_headroom_contract_dropdown<F>(
    initial: Option<u32>,
    on_pick: F,
) -> gtk::DropDown
where
    F: Fn(Option<u32>) + 'static,
{
    build_dropdown(headroom_options(), initial, on_pick)
}

#[cfg(test)]
pub(in crate::ui) fn refresh_banner_contract(banner: &adw::Banner, tweaks: UserTweaks) {
    let selection = Rc::new(RefCell::new(Selection::from_disk(&tweaks)));
    refresh_banner(banner, &selection);
}

#[cfg(test)]
pub(in crate::ui) fn build_tuning_page_contract(tweaks: UserTweaks) -> GtkBox {
    let content = GtkBox::builder().orientation(Orientation::Vertical).build();
    populate_tuning_page(&content, Selection::from_disk(&tweaks));
    content
}

// ── Apply / Reset ────────────────────────────────────────────────────

/// Outcome of the apply worker, distinguishing a config-write failure
/// from a service-restart failure so the UI can show the right message.
enum ApplyOutcome {
    Ok,
    WriteFailed(String),
    RestartFailed(String),
}

fn apply_clicked(button: &gtk::Button, selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) {
    let snapshot = *selection.borrow();
    let tweaks = snapshot.into_tweaks();

    button.set_sensitive(false);
    button.set_label(&i18n("Restarting audio…"));

    let button_weak = button.downgrade();
    let banner_weak = banner.downgrade();
    let selection = Rc::clone(selection);
    glib::spawn_future_local(async move {
        // Write the drop-ins and restart the stack on the same worker so
        // the load-bearing order (fsync the config, *then* bounce the
        // daemons that re-read it) is preserved off the main loop —
        // offloading the write separately could race it past the restart.
        let outcome = gio::spawn_blocking(move || {
            if let Err(e) = tweaks.apply() {
                return ApplyOutcome::WriteFailed(e.to_string());
            }
            match restart_pipewire_user_stack() {
                Ok(()) => ApplyOutcome::Ok,
                Err(e) => ApplyOutcome::RestartFailed(e.to_string()),
            }
        })
        .await
        .unwrap_or_else(|_| ApplyOutcome::RestartFailed("worker thread panicked".to_owned()));

        let Some(button) = button_weak.upgrade() else {
            return;
        };
        button.set_sensitive(true);
        button.set_label(&i18n("Apply and restart audio"));

        if let Some(banner) = banner_weak.upgrade() {
            refresh_banner(&banner, &selection);
        }

        match outcome {
            ApplyOutcome::Ok => {}
            ApplyOutcome::WriteFailed(message) => {
                dialogs::error_dialog(
                    &i18n("Failed to write configuration"),
                    &message,
                    &i18n("OK"),
                )
                .present(Some(&button));
            }
            ApplyOutcome::RestartFailed(message) => {
                dialogs::error_dialog(&i18n("Audio service restart failed"), &message, &i18n("OK"))
                    .present(Some(&button));
            }
        }
    });
}

#[cfg(test)]
pub(in crate::ui) fn trigger_apply_button_contract(button: &gtk::Button, banner: &adw::Banner) {
    let selection = Rc::new(RefCell::new(Selection::default()));
    apply_clicked(button, &selection, banner);
}

pub(in crate::ui) fn reset_audio_settings_dialog() -> adw::AlertDialog {
    dialogs::confirm_dialog(
        &i18n("Restore default audio settings?"),
        &i18n(
            "All your custom audio settings will be removed and the \
             audio service will restart with the defaults.",
        ),
        &i18n("Cancel"),
        "reset",
        &i18n("Restore"),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fully_custom_tweaks() -> UserTweaks {
        UserTweaks {
            quantum: Some(512),
            headroom_usb: Some(256),
            headroom_pci: Some(1024),
            bt_latency: Some(2048),
            sample_rates: Some(SampleRates::HiRes),
            bt_sbc_xq: Some(true),
            bt_call_autoswitch: Some(false),
            alsa_no_suspend: Some(true),
        }
    }

    #[test]
    fn selection_round_trips_every_user_tweak_field() {
        let tweaks = fully_custom_tweaks();
        let selection = Selection::from_disk(&tweaks);

        assert_eq!(selection.quantum, Some(512));
        assert_eq!(selection.headroom_usb, Some(256));
        assert_eq!(selection.headroom_pci, Some(1024));
        assert_eq!(selection.bt_latency, Some(2048));
        assert_eq!(selection.sample_rates, Some(SampleRates::HiRes));
        assert_eq!(selection.bt_sbc_xq, Some(true));
        assert_eq!(selection.bt_call_autoswitch, Some(false));
        assert_eq!(selection.alsa_no_suspend, Some(true));
        assert_eq!(selection.into_tweaks(), tweaks);
    }

    #[test]
    fn selection_modified_state_tracks_each_field_independently() {
        assert!(!Selection::default().is_modified());

        for selection in [
            Selection {
                quantum: Some(512),
                ..Selection::default()
            },
            Selection {
                headroom_usb: Some(256),
                ..Selection::default()
            },
            Selection {
                headroom_pci: Some(1024),
                ..Selection::default()
            },
            Selection {
                bt_latency: Some(2048),
                ..Selection::default()
            },
            Selection {
                sample_rates: Some(SampleRates::Cd),
                ..Selection::default()
            },
            Selection {
                bt_sbc_xq: Some(false),
                ..Selection::default()
            },
            Selection {
                bt_call_autoswitch: Some(true),
                ..Selection::default()
            },
            Selection {
                alsa_no_suspend: Some(true),
                ..Selection::default()
            },
        ] {
            assert!(selection.is_modified(), "{selection:?}");
        }
    }

    #[test]
    fn banner_title_message_matches_modified_state() {
        assert_eq!(
            banner_title_message_for_modified_state(false),
            "The standard audio settings are in use."
        );
        assert_eq!(
            banner_title_message_for_modified_state(true),
            "Custom settings active. Click Apply to enable them."
        );
    }

    #[test]
    #[cfg(not(miri))]
    fn localized_banner_title_matches_modified_state() {
        assert_eq!(
            banner_title_for_modified_state(false),
            "The standard audio settings are in use."
        );
        assert_eq!(
            banner_title_for_modified_state(true),
            "Custom settings active. Click Apply to enable them."
        );
    }

    #[test]
    fn reset_response_only_accepts_reset_id() {
        assert!(is_reset_response("reset"));
        assert!(!is_reset_response("cancel"));
        assert!(!is_reset_response(""));
    }

    #[test]
    #[cfg(not(miri))]
    fn headroom_options_keep_default_and_curated_values() {
        let options = headroom_options();
        let labels = options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>();
        let values = options
            .iter()
            .map(|option| option.value)
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            [
                "Default",
                "None",
                "Small (~5 ms)",
                "Medium (~11 ms)",
                "Standard (default, ~21 ms)",
                "Large (virtual machines, ~42 ms)",
                "Maximum (last resort, ~85 ms)",
            ]
        );
        assert_eq!(
            values,
            [
                None,
                Some(0),
                Some(256),
                Some(512),
                Some(1024),
                Some(2048),
                Some(4096),
            ]
        );
    }
}
