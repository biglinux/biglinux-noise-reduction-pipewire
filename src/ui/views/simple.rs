//! Combined Simple-mode page.
//!
//! One scrollable area with two cards (microphone, system sound). No
//! tab switcher — beginners see every control in one place. The mic
//! card splits internally into "Input device" (always visible) and
//! "Noise filter" (own switch + intensity), so picking a mic stays
//! independent from turning the filter on.
//!
//! Controls emit typed [`MicInput`] messages to the [`MicShell`] component
//! (`super::super::mic_shell`); the component's `update` owns every `AppState`
//! transition. (The device picker keeps its own wiring — converted in a later
//! stage.)

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, Label, Orientation, ScrolledWindow};

use crate::services::pipewire::source_volume;

use big_relm4_components::feedback::tooltip;

use super::super::i18n::i18n;
use super::super::mic_shell::MicInput;
use super::super::state::AppState;
use super::super::widgets::didactic::{
    group_separator, illustration, percent_slider_on_change, switch_row, DidacticCard,
};
use super::super::widgets::source_picker;

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

    content.append(&mic_card(state, input));
    content.append(output_card(state, input).widget());

    scroll.set_child(Some(&content));
    scroll.upcast()
}

/// Microphone card — header-less. Top half is the hardware picker
/// (dropdown + volume); bottom half mirrors a [`DidacticCard`] header
/// inline (`[SVG | title+desc | switch]`) followed by the intensity
/// slider and the "Hear my voice" toggle.
fn mic_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> GtkBox {
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

    let nr_strength = state.settings().noise_reduction.strength;
    let intensity = percent_slider_on_change(&i18n("Intensity"), nr_strength, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::MicIntensityChanged(v));
        }
    });
    card.append(&intensity);
    card.append(&self_listen_row(state, input));
    card
}

/// `[SVG | title+desc | switch]` row that mirrors `DidacticCard::new`
/// so the Noise filter sub-section visually matches the System sound
/// card below.
fn noise_filter_header(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> GtkBox {
    let switch = gtk::Switch::builder()
        .valign(Align::Center)
        .active(state.settings().noise_reduction.enabled)
        .build();
    switch.update_property(&[gtk::accessible::Property::Label(&i18n("Noise filter"))]);
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::NoiseFilterToggled(sw.is_active()));
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
            "Removes background noise from your voice with the GTCRN \
             neural network. Higher intensity = stronger cleanup.",
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

/// "Hear my voice" toggle row — feeds the mic into the default sink so
/// the user can calibrate the filter intensity. Recommended only with
/// headphones (loopback to speakers can create acoustic feedback).
fn self_listen_row(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> GtkBox {
    let switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(state.settings().monitor.enabled)
        .build();
    tooltip::set(&switch, i18n("Headphones only — speakers cause feedback."));
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::SelfListenToggled(sw.is_active()));
        });
    }
    switch_row(&i18n("Hear my voice"), &switch)
}

fn output_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().output_filter.enabled)
        .build();
    {
        let input = input.clone();
        switch.connect_active_notify(move |sw| {
            let _ = input.send(MicInput::OutputFilterToggled(sw.is_active()));
        });
    }
    let card = DidacticCard::new(
        "output_filter.svg",
        &i18n("System sound"),
        &i18n("Cleans background noise from everything you hear before it reaches your speakers."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    );

    let strength = state.settings().output_filter.noise_reduction.strength;
    let row = percent_slider_on_change(&i18n("Intensity"), strength, {
        let input = input.clone();
        move |v| {
            let _ = input.send(MicInput::OutputIntensityChanged(v));
        }
    });
    card.add_row(&row);
    card
}
