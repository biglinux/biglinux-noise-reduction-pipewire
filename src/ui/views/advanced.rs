//! Advanced-mode "Tuning" page.
//!
//! Lets the user override the BigLinux distro defaults for PipeWire and
//! WirePlumber on a per-control basis without leaving the GUI. Every
//! card hosts a single dropdown whose first entry is the sentinel
//! `Distribution default` — that selection wipes the corresponding
//! line from the user drop-in so the layered PipeWire and WirePlumber
//! defaults take over again.
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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Box as GtkBox, Orientation, ScrolledWindow, gio, glib};

use super::super::i18n::i18n;
use crate::services::pipewire::user_tweaks::{SampleRates, TuningRevision, UserTweaks};
use apply::apply_clicked;
pub(in crate::ui) use apply::reset_audio_settings_dialog;
#[cfg(test)]
pub(in crate::ui) use apply::trigger_apply_button_contract;

mod apply;
use super::super::widgets::didactic::{DidacticCard, labelled_row, section_header};

/// Editable values and the last successfully applied snapshot are distinct.
pub(super) struct TuningSelection {
    current: RefCell<UserTweaks>,
    applied: RefCell<UserTweaks>,
    persisted: RefCell<TuningRevision>,
    restart_pending: Cell<bool>,
    busy: Cell<bool>,
    content: glib::WeakRef<GtkBox>,
    apply_button: RefCell<glib::WeakRef<gtk::Button>>,
    reset_button: RefCell<glib::WeakRef<gtk::Button>>,
    dropdowns: RefCell<Vec<glib::WeakRef<gtk::DropDown>>>,
    preview: RefCell<Option<crate::services::preview::QuantumPreview>>,
    preview_busy: Cell<bool>,
}

impl TuningSelection {
    fn new(initial: UserTweaks) -> Self {
        Self {
            persisted: RefCell::new(TuningRevision::from_settings(&initial)),
            restart_pending: Cell::new(false),
            applied: RefCell::new(initial.clone()),
            current: RefCell::new(initial),
            busy: Cell::new(false),
            content: glib::WeakRef::new(),
            apply_button: RefCell::new(glib::WeakRef::new()),
            reset_button: RefCell::new(glib::WeakRef::new()),
            dropdowns: RefCell::new(Vec::new()),
            preview: RefCell::new(None),
            preview_busy: Cell::new(false),
        }
    }
    fn borrow(&self) -> std::cell::Ref<'_, UserTweaks> {
        self.current.borrow()
    }
    fn borrow_mut(&self) -> std::cell::RefMut<'_, UserTweaks> {
        self.current.borrow_mut()
    }
    fn register(&self, dropdown: &gtk::DropDown) {
        self.dropdowns.borrow_mut().push(dropdown.downgrade());
    }
    fn reset_controls(&self) {
        for dropdown in self
            .dropdowns
            .borrow()
            .iter()
            .filter_map(glib::WeakRef::upgrade)
        {
            dropdown.set_selected(0);
        }
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
        let result = gio::spawn_blocking(UserTweaks::load_snapshot).await;
        if let Some(content) = content_weak.upgrade() {
            match result {
                Ok(Ok((tweaks, revision))) => {
                    populate_tuning_page(&content, tweaks, Some(revision));
                }
                other => {
                    log::warn!("tuning configuration could not be read: {other:?}");
                    let label = gtk::Label::new(Some(&i18n(
                        "Audio settings could not be read. Check file permissions and reopen this window.",
                    )));
                    label.set_wrap(true);
                    content.append(&label);
                }
            }
        }
    });
    scroll.upcast()
}

/// Build the Tuning page cards from the already-loaded on-disk selection. Split
/// from [`build`] so the disk read can run off the main loop first.
fn populate_tuning_page(content: &GtkBox, initial: UserTweaks, revision: Option<TuningRevision>) {
    let selection = Rc::new(TuningSelection::new(initial));
    if let Some(revision) = revision {
        *selection.persisted.borrow_mut() = revision;
    }

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

    selection.content.set(Some(content));
    let toolbar = action_toolbar();
    *selection.apply_button.borrow_mut() = toolbar.apply.downgrade();
    *selection.reset_button.borrow_mut() = toolbar.reset.downgrade();
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
                    *selection.borrow_mut() = UserTweaks::default();
                    selection.reset_controls();
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

fn refresh_banner(banner: &adw::Banner, selection: &Rc<TuningSelection>) {
    let dirty =
        *selection.borrow() != *selection.applied.borrow() || selection.restart_pending.get();
    let modified = selection.borrow().is_modified();
    let busy = selection.busy.get();
    let title = if busy {
        i18n("Applying audio settings…")
    } else if selection.restart_pending.get() {
        i18n("Settings were saved, but audio still needs to restart. Apply to try again.")
    } else if dirty {
        i18n("Changes are not applied yet. Apply them when you are ready.")
    } else if modified {
        i18n("Your audio settings are active.")
    } else {
        i18n("The standard audio settings are in use.")
    };
    banner.set_title(&title);
    banner.set_revealed(dirty || modified || busy);
    if let Some(button) = selection.apply_button.borrow().upgrade() {
        button.set_sensitive(dirty && !busy);
    }
    if let Some(button) = selection.reset_button.borrow().upgrade() {
        button.set_sensitive((modified || selection.applied.borrow().is_modified()) && !busy);
    }
}

#[cfg(test)]
fn banner_title_for_modified_state(is_modified: bool) -> String {
    i18n(banner_title_message_for_modified_state(is_modified))
}

#[cfg(test)]
fn banner_title_message_for_modified_state(is_modified: bool) -> &'static str {
    if is_modified {
        "Custom settings active. Click Apply to enable them."
    } else {
        "The standard audio settings are in use."
    }
}

// ── Bluetooth cards ──────────────────────────────────────────────────

fn bt_call_card(selection: &Rc<TuningSelection>, banner: &adw::Banner) -> DidacticCard {
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
    selection.register(&dropdown);
    card.add_row(&labelled_row(&i18n("Behaviour"), &dropdown));
    card
}

fn bt_latency_card(selection: &Rc<TuningSelection>, banner: &adw::Banner) -> DidacticCard {
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
            i18n("Lower delay (needs a stable headset, ~11 ms)"),
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
    selection.register(&dropdown);
    card.add_row(&labelled_row(&i18n("Buffer size"), &dropdown));
    card
}

fn bt_codec_card(selection: &Rc<TuningSelection>, banner: &adw::Banner) -> DidacticCard {
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
    selection.register(&dropdown);
    card.add_row(&labelled_row(
        &i18n("Higher Bluetooth audio quality (SBC-XQ)"),
        &dropdown,
    ));
    card
}

// ── Stability / performance cards ────────────────────────────────────

fn quantum_card(selection: &Rc<TuningSelection>, banner: &adw::Banner) -> DidacticCard {
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
        TweakOption::value(i18n("Low delay (~11 ms)"), 512_u32),
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
    selection.register(&dropdown);
    card.add_row(&labelled_row(&i18n("Reaction speed"), &dropdown));
    let listen = gtk::Button::builder()
        .label(i18n("Try for 15 seconds"))
        .halign(gtk::Align::Start)
        .build();
    listen.set_tooltip_text(Some(&i18n("Temporarily changes the audio buffer for all applications. Stop the preview to return to the previous value.")));
    {
        let selection = Rc::clone(selection);
        listen.connect_clicked(move |button| {
            if selection.preview_busy.replace(true) { return; }
            let previous = selection.preview.borrow_mut().take();
            let frames = selection.borrow().quantum.unwrap_or(0);
            button.set_sensitive(false);
            let weak_button = button.downgrade();
            let selection = Rc::clone(&selection);
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    if let Some(mut preview) = previous {
                        preview.stop().map(|()| None)
                    } else {
                        crate::services::preview::QuantumPreview::start(frames).map(Some)
                    }
                }).await.unwrap_or_else(|_| Err(std::io::Error::other("preview worker failed")));
                selection.preview_busy.set(false);
                let Some(button) = weak_button.upgrade() else { return; };
                button.set_sensitive(true);
                match result {
                    Ok(preview) => {
                        button.set_label(&if preview.is_some() { i18n("Stop preview") } else { i18n("Try for 15 seconds") });
                        *selection.preview.borrow_mut() = preview;
                    }
                    Err(error) => {
                        log::warn!("buffer preview: {error}");
                        button.set_label(&i18n("Try preview again"));
                        button.set_tooltip_text(Some(&i18n("The audio preview could not be started or restored. Check the audio connection and try again.")));
                    }
                }
            });
        });
    }
    let weak_selection = Rc::downgrade(selection);
    let weak_button = listen.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        let (Some(selection), Some(button)) = (weak_selection.upgrade(), weak_button.upgrade())
        else {
            return glib::ControlFlow::Break;
        };
        let expired = selection
            .preview
            .borrow_mut()
            .as_mut()
            .is_some_and(|preview| !preview.is_alive());
        if expired {
            selection.preview.borrow_mut().take();
            button.set_label(&i18n("Try for 15 seconds"));
        }
        glib::ControlFlow::Continue
    });
    card.add_row(&listen);
    card
}

fn headroom_usb_card(selection: &Rc<TuningSelection>, banner: &adw::Banner) -> DidacticCard {
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
    selection.register(&dropdown);
    card.add_row(&labelled_row(&i18n("Margin"), &dropdown));
    card
}

fn headroom_pci_card(selection: &Rc<TuningSelection>, banner: &adw::Banner) -> DidacticCard {
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
    selection.register(&dropdown);
    card.add_row(&labelled_row(&i18n("Margin"), &dropdown));
    card
}

fn alsa_suspend_card(selection: &Rc<TuningSelection>, banner: &adw::Banner) -> DidacticCard {
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
    selection.register(&dropdown);
    card.add_row(&labelled_row(&i18n("Speaker sleep"), &dropdown));
    card
}

// ── Sound-quality cards ──────────────────────────────────────────────

fn sample_rates_card(selection: &Rc<TuningSelection>, banner: &adw::Banner) -> DidacticCard {
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
            i18n("Standard quality only (default, 48 kHz)"),
            SampleRates::Standard,
        ),
        TweakOption::value(
            i18n("CD and standard quality (44.1 + 48 kHz)"),
            SampleRates::Cd,
        ),
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
    selection.register(&dropdown);
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
        TweakOption::value(i18n("Standard (~21 ms)"), 1024_u32),
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
    let selection = Rc::new(TuningSelection::new(tweaks));
    refresh_banner(banner, &selection);
}

#[cfg(test)]
pub(in crate::ui) fn build_tuning_page_contract(tweaks: UserTweaks) -> GtkBox {
    let content = GtkBox::builder().orientation(Orientation::Vertical).build();
    populate_tuning_page(&content, tweaks, None);
    content
}

#[cfg(test)]
#[path = "advanced_tests.rs"]
mod tests;
