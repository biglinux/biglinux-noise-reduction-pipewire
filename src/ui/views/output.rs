//! Output filter Advanced-mode page.
//!
//! Mirrors `views::mic`: every control as its own `DidacticCard`. The
//! Simple-mode beginner layout lives in [`super::simple::build`] and
//! does not route through this module. Controls emit typed [`MicInput`]
//! messages; `MicShell::update` owns the `AppState` transitions.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Box as GtkBox, Orientation, ScrolledWindow};

use crate::config::GATE_INTENSITY_MAX;

use super::super::i18n::i18n;
use super::super::mic_shell::MicInput;
use super::super::state::AppState;
use super::super::widgets::didactic::{
    DidacticCard, labelled_row, percent_slider_on_change, section_header, u8_slider_on_change,
};
use super::super::widgets::eq_card::build_eq_card;
use super::super::widgets::model_picker;

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

    content.append(super::simple::output_card(state, input).widget());

    content.append(&section_header(&i18n("AI noise reduction"), 16));
    content.append(model_card(state, input).widget());
    content.append(voice_recovery_card(state, input).widget());

    content.append(&section_header(&i18n("Audio enhancements"), 16));
    content.append(hpf_card(state, input).widget());
    content.append(gate_card(state, input).widget());
    content.append(compressor_card(state, input).widget());
    content.append(eq_card(state, input).widget());

    scroll.set_child(Some(&content));
    scroll.upcast()
}

fn eq_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let initial = state.settings().output_filter.equalizer.clone();
    let input = input.clone();
    build_eq_card(
        initial,
        i18n("Equalizer"),
        i18n(
            "10-band parametric EQ on what you hear. Pick a preset or \
             drag each band between -40 dB and +40 dB.",
        ),
        move |mutation| {
            let _ = input.send(MicInput::OutputEq(mutation));
        },
    )
}

// ── Advanced-mode cards ────────────────────────────────────────────

fn model_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let card = DidacticCard::new(
        "model.svg",
        &i18n("Neural model"),
        &model_picker::description(),
        None,
    );
    let initial = state.settings().output_filter.noise_reduction.model;
    let dropdown = model_picker::build(initial, {
        let input = input.clone();
        move |pick| {
            let _ = input.send(MicInput::OutputModelChanged(pick));
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
            "Restores high-frequency clarity after suppression. Lower \
             only if the source already sounds harsh.",
        ),
        None,
    );
    let initial = state
        .settings()
        .output_filter
        .noise_reduction
        .voice_recovery;
    let row = percent_slider_on_change(&i18n("Recovery"), initial, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::OutputVoiceRecoveryChanged(v));
        }
    });
    card.add_row(&row);
    card
}

fn hpf_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().output_filter.hpf.enabled)
        .build();
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::OutputHpfToggled(sw.is_active()));
        });
    }
    DidacticCard::new(
        "hpf.svg",
        &i18n("High-pass filter"),
        &i18n("Cuts low-frequency rumble from playback — wind, AC noise."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    )
}

fn gate_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().output_filter.gate.enabled)
        .build();
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::OutputGateToggled(sw.is_active()));
        });
    }
    let card = DidacticCard::new(
        "gate.svg",
        &i18n("Silence gate"),
        &i18n("Mutes playback during silent moments so you don't hear hiss between words."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    );
    let initial = state.settings().output_filter.gate.intensity;
    let row = u8_slider_on_change(&i18n("Intensity"), initial, GATE_INTENSITY_MAX, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::OutputGateIntensityChanged(v));
        }
    });
    card.add_row(&row);
    card
}

fn compressor_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().output_filter.compressor.enabled)
        .build();
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::OutputCompressorToggled(sw.is_active()));
        });
    }
    let card = DidacticCard::new(
        "compressor.svg",
        &i18n("Compressor"),
        &i18n(
            "Evens out volume across loud and quiet speakers — useful \
             for meetings with mixed mic levels.",
        ),
        Some(switch.upcast_ref::<gtk::Widget>()),
    );
    let initial = state.settings().output_filter.compressor.intensity;
    let row = percent_slider_on_change(&i18n("Intensity"), initial, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::OutputCompressorIntensityChanged(v));
        }
    });
    card.add_row(&row);
    card
}
