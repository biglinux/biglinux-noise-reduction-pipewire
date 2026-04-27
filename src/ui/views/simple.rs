//! Combined Simple-mode page.
//!
//! One scrollable area with two cards (microphone, system sound). No
//! tab switcher — beginners see every control in one place. The mic
//! card splits internally into "Input device" (always visible) and
//! "Noise filter" (own switch + intensity), so picking a mic stays
//! independent from turning the filter on.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, Label, Orientation, ScrolledWindow};

use crate::services::pipewire::source_volume;

use super::super::i18n::i18n;
use super::super::state::AppState;
use super::super::widgets::didactic::{
    group_separator, illustration, percent_slider, switch_row, DidacticCard,
};
use super::super::widgets::source_picker;

pub fn build(state: &Rc<AppState>) -> gtk::Widget {
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

    content.append(&mic_card(state));
    content.append(output_card(state).widget());

    scroll.set_child(Some(&content));
    scroll.upcast()
}

/// Microphone card — header-less. Top half is the hardware picker
/// (dropdown + volume); bottom half mirrors a [`DidacticCard`] header
/// inline (`[SVG | title+desc | switch]`) followed by the intensity
/// slider and the "Hear my voice" toggle.
fn mic_card(state: &Rc<AppState>) -> GtkBox {
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
    card.append(&noise_filter_header(state));

    let nr = &state.settings().noise_reduction;
    let intensity = percent_slider(state, &i18n("Intensity"), nr.strength, |s, v| {
        s.noise_reduction.strength = v;
    });
    card.append(&intensity);
    card.append(&self_listen_row(state));
    card
}

/// `[SVG | title+desc | switch]` row that mirrors `DidacticCard::new`
/// so the Noise filter sub-section visually matches the System sound
/// card below.
fn noise_filter_header(state: &Rc<AppState>) -> GtkBox {
    let switch = gtk::Switch::builder()
        .valign(Align::Center)
        .active(state.settings().noise_reduction.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.noise_reduction.enabled = on);
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
fn self_listen_row(state: &Rc<AppState>) -> GtkBox {
    let switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .tooltip_text(i18n("Headphones only — speakers cause feedback."))
        .active(state.settings().monitor.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.monitor.enabled = on);
        });
    }
    switch_row(&i18n("Hear my voice"), &switch)
}

fn output_card(state: &Rc<AppState>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().output_filter.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.output_filter.enabled = on);
        });
    }
    let card = DidacticCard::new(
        "output_filter.svg",
        &i18n("System sound"),
        &i18n("Cleans background noise from everything you hear before it reaches your speakers."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    );

    let nr = &state.settings().output_filter.noise_reduction;
    let row = percent_slider(state, &i18n("Intensity"), nr.strength, |s, v| {
        s.output_filter.noise_reduction.strength = v;
    });
    card.add_row(&row);
    card
}
