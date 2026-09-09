//! Combined Simple-mode page.
//!
//! One scrollable area with two cards (microphone, system sound). No
//! tab switcher — beginners see every control in one place. The mic
//! card splits internally into "Input device" (always visible) and
//! "Noise filter" (own switch + intensity), so picking a mic stays
//! independent from turning the filter on.
//!
//! Controls emit typed [`MicInput`] messages to the
//! [`MicShell`](crate::ui::mic_shell::MicShell) component; its `update` owns every `AppState`
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
    DidacticCard, group_separator, illustration, percent_slider_on_change, switch_row,
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
    content.append(super::mic::quality_card(state, input).widget());
    content.append(super::mic::echo_cancel_card(state, input).widget());

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
    if let Some(note) = advanced_effects_note(state) {
        card.append(&note);
    }
    card.append(&self_listen_row(state, input));
    card
}

/// Honesty note for the Simple view: the "Noise filter" switch mirrors
/// `noise_reduction.enabled`, but Advanced can leave other mic effects
/// (echo cancellation, EQ, gate, …) running with that switch off. When
/// that happens, say so instead of letting the off switch imply the
/// microphone is untouched. Simple-view toggling never hits this: the
/// master off cascades every flag, and the body repopulates whenever
/// the mode changes or settings arrive externally.
fn advanced_effects_note(state: &Rc<AppState>) -> Option<Label> {
    let settings = state.settings();
    if settings.noise_reduction.enabled || !crate::pipeline::mic_chain_wanted(&settings) {
        return None;
    }
    Some(
        Label::builder()
            .label(i18n(
                "Other microphone effects are still active — see the Advanced view.",
            ))
            .wrap(true)
            .xalign(0.0)
            .margin_start(16)
            .margin_end(16)
            .margin_bottom(8)
            .css_classes(vec!["dim-label", "caption"])
            .build(),
    )
}

/// `[SVG | title+desc | switch]` row that mirrors `DidacticCard::new`
/// so the Noise filter sub-section visually matches the System sound
/// card below.
fn noise_filter_header(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> GtkBox {
    let switch = gtk::Switch::builder()
        .valign(Align::Center)
        .active(crate::pipeline::mic_chain_wanted(&state.settings()))
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
            "Reduces background noise in your voice. Turning this off pauses microphone effects without forgetting your choices.",
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
pub(super) fn self_listen_row(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> GtkBox {
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

pub(super) fn output_card(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> DidacticCard {
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
