//! Advanced-mode "Tuning" page.
//!
//! Lets the user override the BigLinux distro defaults for PipeWire and
//! WirePlumber on a per-control basis without leaving the GUI. Every
//! card hosts a single dropdown whose first entry is the sentinel
//! `Padrão da distribuição` — that selection wipes the corresponding
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
use gtk::{gio, glib, Box as GtkBox, Orientation, ScrolledWindow};

use crate::services::pipewire::user_tweaks::{SampleRates, UserTweaks};
use crate::services::system_audio::restart_pipewire_user_stack;

use super::super::i18n::i18n;
use super::super::widgets::didactic::{labelled_row, section_header, DidacticCard};

/// Per-control selection, decoupled from the on-disk shape so the UI
/// can offer a richer set of curated defaults than what raw integers
/// allow. The sentinel `None` always maps to "Padrão da distribuição".
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
    let initial = Selection::from_disk(&UserTweaks::load_from_disk());
    let selection = Rc::new(RefCell::new(initial));

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
            reset_clicked(btn, &selection, &banner);
        });
    }

    scroll.set_child(Some(&content));
    scroll.upcast()
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
    banner.set_title(&if modified {
        i18n("Custom settings active. Click Apply to enable them.")
    } else {
        i18n("The standard audio settings are in use.")
    });
    banner.set_revealed(modified);
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
/// "Padrão da distribuição".
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

// ── Apply / Reset ────────────────────────────────────────────────────

fn apply_clicked(button: &gtk::Button, selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) {
    let snapshot = *selection.borrow();
    let tweaks = snapshot.into_tweaks();
    if let Err(e) = tweaks.apply() {
        present_error(
            button,
            &i18n("Failed to write configuration"),
            &e.to_string(),
        );
        return;
    }

    button.set_sensitive(false);
    button.set_label(&i18n("Restarting audio…"));

    let button_weak = button.downgrade();
    let banner_weak = banner.downgrade();
    let selection = Rc::clone(selection);
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(restart_pipewire_user_stack)
            .await
            .unwrap_or_else(|_| Err(std::io::Error::other("worker thread panicked")));

        let Some(button) = button_weak.upgrade() else {
            return;
        };
        button.set_sensitive(true);
        button.set_label(&i18n("Apply and restart audio"));

        if let Some(banner) = banner_weak.upgrade() {
            refresh_banner(&banner, &selection);
        }

        if let Err(e) = result {
            present_error(
                &button,
                &i18n("Audio service restart failed"),
                &e.to_string(),
            );
        }
    });
}

fn reset_clicked(button: &gtk::Button, selection: &Rc<RefCell<Selection>>, banner: &adw::Banner) {
    let dialog = adw::AlertDialog::new(
        Some(&i18n("Restore default audio settings?")),
        Some(&i18n(
            "All your custom audio settings will be removed and the \
             audio service will restart with the defaults.",
        )),
    );
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response("reset", &i18n("Restore"));
    dialog.set_response_appearance("reset", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    let selection = Rc::clone(selection);
    let banner = banner.clone();
    let button_weak = button.downgrade();
    dialog.connect_response(None, move |dlg, response| {
        if response == "reset" {
            *selection.borrow_mut() = Selection::default();
            if let Err(e) = UserTweaks::clear() {
                if let Some(b) = button_weak.upgrade() {
                    present_error(&b, &i18n("Failed to remove configuration"), &e.to_string());
                }
                dlg.close();
                return;
            }
            refresh_banner(&banner, &selection);
            if let Some(b) = button_weak.upgrade() {
                apply_clicked(&b, &selection, &banner);
            }
        }
        dlg.close();
    });

    dialog.present(Some(button));
}

fn present_error(parent: &gtk::Button, title: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(title), Some(body));
    dialog.add_response("ok", &i18n("OK"));
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    dialog.present(Some(parent));
}
