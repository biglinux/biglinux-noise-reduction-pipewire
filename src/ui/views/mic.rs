//! Microphone Advanced-mode page.
//!
//! Every individual control sits in its own `DidacticCard`. Beginners get a
//! simpler combined view from [`super::simple::build`]; this page shows only
//! when the header `Advanced` switch is on. Controls emit typed [`MicInput`]
//! messages; `MicShell::update` owns the `AppState` transitions.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, Label, Orientation, ScrolledWindow};

use crate::config::{GATE_INTENSITY_MAX, StereoMode};
use crate::services::pipewire::source_volume;

use super::super::i18n::i18n;
use super::super::mic_shell::MicInput;
use super::super::state::AppState;
use super::super::widgets::didactic::{
    DidacticCard, group_separator, illustration, labelled_row, percent_slider_on_change,
    section_header, slider_row, u8_slider_on_change, u32_slider_on_change,
};
use super::super::widgets::eq_card::build_eq_card;
use super::super::widgets::{model_picker, source_picker};

pub fn build(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> gtk::Widget {
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

    content.append(&mic_combo_card(state, input));
    content.append(self_listen_delay_card(state, input).widget());
    content.append(echo_cancel_card(state, input).widget());

    content.append(&section_header(&i18n("Noise filter — fine-tune"), 16));
    content.append(quality_card(state, input).widget());
    content.append(&resource_options(state, input));
    content.append(model_card(state, input).widget());
    content.append(voice_recovery_card(state, input).widget());

    content.append(&section_header(&i18n("Voice enhancements"), 16));
    content.append(hpf_card(state, input).widget());
    content.append(gate_card(state, input).widget());
    content.append(compressor_card(state, input).widget());
    content.append(eq_card(state, input).widget());

    content.append(&section_header(&i18n("Voice changer"), 16));
    content.append(voice_changer_card(state, input).widget());

    scroll.set_child(Some(&content));
    scroll.upcast()
}

// ── Advanced-mode cards ────────────────────────────────────────────

/// Combined microphone card — same layout as the microphone card built by
/// [`super::simple::build`]
/// (hardware picker → separator → noise-filter master → intensity →
/// self-listen toggle).
fn mic_combo_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .css_classes(vec!["card"])
        .margin_top(8)
        .margin_bottom(12)
        .build();

    let rows = source_picker::build_rows(source_volume);
    rows.dropdown_row.set_margin_top(12);
    card.append(&rows.dropdown_row);
    card.append(&rows.volume_row);

    card.append(&group_separator());
    card.append(&noise_filter_header(state, input));

    let strength = state.settings().noise_reduction.strength;
    let intensity = percent_slider_on_change(&i18n("Intensity"), strength, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::MicIntensityChanged(v));
        }
    });
    card.append(&intensity);
    card.append(&super::simple::self_listen_row(state, input));
    card
}

/// Standalone Echo-cancellation card.
pub(super) fn echo_cancel_card(
    state: &Rc<AppState>,
    input: &relm4::Sender<MicInput>,
) -> DidacticCard {
    use crate::config::EchoMode;
    let modes = [EchoMode::Automatic, EchoMode::Always, EchoMode::Never];
    let labels = [i18n("Automatic"), i18n("Always"), i18n("Never")];
    let dropdown =
        gtk::DropDown::from_strings(&labels.iter().map(String::as_str).collect::<Vec<_>>());
    dropdown.set_selected(
        modes
            .iter()
            .position(|mode| *mode == state.settings().echo_cancel.mode)
            .unwrap_or(0) as u32,
    );
    dropdown.update_property(&[gtk::accessible::Property::Label(&i18n("Echo cancellation"))]);
    let input = input.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        if let Some(mode) = modes.get(dropdown.selected() as usize) {
            let _ = input.send(MicInput::MicEchoModeChanged(*mode));
        }
    });
    DidacticCard::new(
        "master_mic.svg",
        &i18n("Echo cancellation"),
        &i18n(
            "Automatic cancels speaker echo and turns off cancellation when headphones are in use.",
        ),
        Some(dropdown.upcast_ref()),
    )
}

pub(super) fn quality_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    use crate::config::Quality;
    let modes = [
        Quality::Automatic,
        Quality::Best,
        Quality::Cheapest,
        Quality::Manual,
    ];
    let labels = [
        i18n("Automatic"),
        i18n("Best quality"),
        i18n("Lower CPU use"),
        i18n("Manual model"),
    ];
    let dropdown =
        gtk::DropDown::from_strings(&labels.iter().map(String::as_str).collect::<Vec<_>>());
    dropdown.set_selected(
        modes
            .iter()
            .position(|mode| *mode == state.settings().quality)
            .unwrap_or(0) as u32,
    );
    dropdown.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Noise reduction quality",
    ))]);
    let input = input.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        if let Some(mode) = modes.get(dropdown.selected() as usize) {
            let _ = input.send(MicInput::QualityChanged(*mode));
        }
    });
    DidacticCard::new(
        "master_mic.svg",
        &i18n("Noise reduction quality"),
        &i18n(
            "Automatic chooses a model for this computer. Choose lower CPU use if sound stutters, or select a model in Advanced.",
        ),
        Some(dropdown.upcast_ref()),
    )
}

/// `[SVG | title+desc | switch]` row matching `DidacticCard::new` so the
/// AI noise-filter master sits visually identical to the System sound card.
fn noise_filter_header(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> GtkBox {
    let switch = gtk::Switch::builder()
        .valign(Align::Center)
        .active(state.settings().noise_reduction.enabled)
        .build();
    switch.update_property(&[gtk::accessible::Property::Label(&i18n("Noise filter"))]);
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::MicNrEnabled(sw.is_active()));
        });
    }

    let header = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(16)
        .margin_top(16)
        .margin_bottom(12)
        .margin_start(16)
        .margin_end(16)
        .build();

    let svg = illustration("master_mic.svg");
    svg.set_halign(Align::Start);
    header.append(&svg);

    let text = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .valign(Align::Center)
        .build();

    let title = Label::builder()
        .label(i18n("Noise filter"))
        .halign(Align::Start)
        .wrap(true)
        .css_classes(vec!["heading"])
        .build();
    text.append(&title);

    let desc = Label::builder()
        .label(i18n(
            "Removes background noise from your voice using AI. Higher \
             intensity = stronger cleanup.",
        ))
        .wrap(true)
        .xalign(0.0)
        .css_classes(vec!["dim-label"])
        .build();
    text.append(&desc);

    header.append(&text);
    header.append(&switch);
    header
}

fn model_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let card = DidacticCard::new(
        "model.svg",
        &i18n("Neural model"),
        &model_picker::description(),
        None,
    );
    let initial = state.settings().noise_reduction.model;
    let dropdown = model_picker::build(initial, {
        let input = input.clone();
        move |pick| {
            let _ = input.send(MicInput::MicModelChanged(pick));
        }
    });
    card.add_row(&labelled_row(&i18n("Model"), &dropdown));
    card
}

fn voice_recovery_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let card = DidacticCard::new(
        "voice_recovery.svg",
        &i18n("Voice presence"),
        &i18n(
            "Restores high-frequency clarity (sibilants like \"s\", \"sh\") \
             after suppression. Lower only if your voice already sounds harsh.",
        ),
        None,
    );
    let initial = state.settings().noise_reduction.voice_recovery;
    let row = percent_slider_on_change(&i18n("Recovery"), initial, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::MicVoiceRecoveryChanged(v));
        }
    });
    card.add_row(&row);
    card
}

fn hpf_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().hpf.enabled)
        .build();
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::MicHpfToggled(sw.is_active()));
        });
    }
    DidacticCard::new(
        "hpf.svg",
        &i18n("High-pass filter"),
        &i18n("Cuts low-frequency rumble — AC, wind, mic-stand bumps."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    )
}

fn gate_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().gate.enabled)
        .build();
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::MicGateToggled(sw.is_active()));
        });
    }
    let card = DidacticCard::new(
        "gate.svg",
        &i18n("Silence gate"),
        &i18n(
            "Mutes the mic during pauses so listeners hear silence \
             instead of hiss. Higher = more aggressive.",
        ),
        Some(switch.upcast_ref::<gtk::Widget>()),
    );
    let initial = state.settings().gate.intensity;
    let row = u8_slider_on_change(&i18n("Intensity"), initial, GATE_INTENSITY_MAX, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::MicGateIntensityChanged(v));
        }
    });
    card.add_row(&row);
    card
}

fn eq_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let initial = state.settings().equalizer.clone();
    let input = input.clone();
    build_eq_card(
        initial,
        i18n("Equalizer"),
        i18n(
            "Fine-tune your voice tone across 10 frequency bands. Pick a \
             preset or drag each band between -40 dB and +40 dB.",
        ),
        move |mutation| {
            let _ = input.send(MicInput::MicEq(mutation));
        },
    )
}

/// Pitch-shift card (LADSPA `pitch_scale_1193`). Width 0..1 maps exponentially
/// to coefficient 0.5..2.0 — see [`crate::pipeline::build_mic_conf_for`].
fn voice_changer_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(
            state.settings().stereo.enabled
                && state.settings().stereo.mode == StereoMode::VoiceChanger,
        )
        .build();
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::MicVoiceChangerToggled(sw.is_active()));
        });
    }
    let card = DidacticCard::new(
        "voice_changer.svg",
        &i18n("Voice changer"),
        &i18n(
            "Shifts the pitch of your voice without retiming. Left = \
             deeper, right = higher. Center keeps your natural voice.",
        ),
        Some(switch.upcast_ref::<gtk::Widget>()),
    );

    let initial = state.settings().stereo.width;
    let row = pitch_slider(initial, input);
    card.add_row(&row);
    card
}

/// Pitch slider with labelled marks at the five voice-changer presets.
/// Maps `stereo.width` (0.0..=1.0) to a 0..100 percent scale.
fn pitch_slider(initial: f32, input: &relm4::Sender<MicInput>) -> GtkBox {
    let percent = (f64::from(initial) * 100.0).clamp(0.0, 100.0);
    let adj = gtk::Adjustment::new(percent, 0.0, 100.0, 1.0, 25.0, 0.0);
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adj));
    scale.add_mark(0.0, gtk::PositionType::Bottom, Some(&i18n("Deep")));
    scale.add_mark(25.0, gtk::PositionType::Bottom, Some(&i18n("Lower")));
    scale.add_mark(50.0, gtk::PositionType::Bottom, Some(&i18n("Natural")));
    scale.add_mark(75.0, gtk::PositionType::Bottom, Some(&i18n("Higher")));
    scale.add_mark(100.0, gtk::PositionType::Bottom, Some(&i18n("Chipmunk")));
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, 0);

    {
        let input = input.clone();
        adj.connect_value_changed(move |a| {
            let v = (a.value() / 100.0).clamp(0.0, 1.0) as f32;
            let _ = input.send(MicInput::MicPitchChanged(v));
        });
    }

    slider_row(&i18n("Pitch"), &scale, &spin)
}

/// Delay slider for the self-listen monitor (own card, hidden in Simple).
fn self_listen_delay_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let card = DidacticCard::new(
        "master_mic.svg",
        &i18n("Self-listen delay"),
        &i18n(
            "Round-trip latency for the \"Hear my voice\" monitor. Raise \
             only if the loopback sounds rushed or chops on your hardware.",
        ),
        None,
    );
    let initial = state.settings().monitor.delay_ms;
    let row = u32_slider_on_change(&i18n("Delay (ms)"), initial, 2000, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::MicSelfListenDelayChanged(v));
        }
    });
    card.add_row(&row);
    card
}

fn compressor_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().compressor.enabled)
        .build();
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::MicCompressorToggled(sw.is_active()));
        });
    }
    let card = DidacticCard::new(
        "compressor.svg",
        &i18n("Compressor"),
        &i18n("Evens out loud peaks and quiet parts so your voice stays at a steady volume."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    );
    let initial = state.settings().compressor.intensity;
    let row = percent_slider_on_change(&i18n("Intensity"), initial, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::MicCompressorIntensityChanged(v));
        }
    });
    card.add_row(&row);
    card
}

fn resource_options(
    state: &Rc<AppState>,
    input: &relm4::Sender<MicInput>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let details = adw::ExpanderRow::builder()
        .title(i18n("Performance options"))
        .subtitle(i18n("Leave these off unless audio still stutters. Changing them briefly restarts the filters."))
        .build();
    let fast = adw::SwitchRow::builder()
        .title(i18n("Prefer faster processor cores"))
        .subtitle(i18n("May help on some computers, but can use more power. The automatic system choice is usually best."))
        .active(state.settings().runtime.prefer_fast_cpus).build();
    let sender = input.clone();
    fast.connect_active_notify(move |row| {
        let _ = sender.send(MicInput::PreferFastCpusChanged(row.is_active()));
    });
    let memory = adw::SwitchRow::builder()
        .title(i18n("Keep loaded audio data in memory"))
        .subtitle(i18n("May reduce pauses under memory pressure, but leaves less RAM for other applications. This is optional and limited by the system."))
        .active(state.settings().runtime.reserve_memory).build();
    let sender = input.clone();
    memory.connect_active_notify(move |row| {
        let _ = sender.send(MicInput::ReserveMemoryChanged(row.is_active()));
    });
    details.add_row(&fast);
    details.add_row(&memory);
    group.add(&details);
    group
}
