//! Output filter Advanced-mode page.
//!
//! Mirrors `views::mic`: every control as its own `DidacticCard`. The
//! Simple-mode beginner layout lives in [`super::simple::build`] and
//! does not route through this module.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Box as GtkBox, Orientation, ScrolledWindow};

use crate::config::GATE_INTENSITY_MAX;

use super::super::i18n::i18n;
use super::super::state::AppState;
use super::super::widgets::didactic::{
    labelled_row, percent_slider, section_header, u8_slider, DidacticCard,
};
use super::super::widgets::eq_card::{build_eq_card, EqMutation};
use super::super::widgets::model_picker;

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

    content.append(output_combo_card(state).widget());

    content.append(&section_header(&i18n("AI noise reduction"), 16));
    content.append(model_card(state).widget());
    content.append(voice_recovery_card(state).widget());

    content.append(&section_header(&i18n("Audio enhancements"), 16));
    content.append(hpf_card(state).widget());
    content.append(gate_card(state).widget());
    content.append(compressor_card(state).widget());
    content.append(eq_card(state).widget());

    scroll.set_child(Some(&content));
    scroll.upcast()
}

fn eq_card(state: &Rc<AppState>) -> DidacticCard {
    build_eq_card(
        state,
        i18n("Equalizer"),
        i18n(
            "10-band parametric EQ on what you hear. Pick a preset or \
             drag each band between -40 dB and +40 dB.",
        ),
        |s| s.output_filter.equalizer.clone(),
        |s, m| match m {
            EqMutation::Enabled(on) => s.output_filter.equalizer.enabled = on,
            EqMutation::Preset(id) => id.clone_into(&mut s.output_filter.equalizer.preset),
            EqMutation::Band { index, gain_db } => {
                if let Some(slot) = s.output_filter.equalizer.bands.get_mut(index) {
                    *slot = gain_db;
                }
            }
        },
    )
}

// ── Advanced-mode cards ────────────────────────────────────────────

/// Master card for the output chain. Mirrors
/// [`super::simple::output_card`] one-for-one (System sound title, the
/// same description, master switch in the header, intensity slider as
/// the first row) so the Simple → Advanced jump only adds depth — it
/// never reshuffles the controls users already learned.
fn output_combo_card(state: &Rc<AppState>) -> DidacticCard {
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

fn model_card(state: &Rc<AppState>) -> DidacticCard {
    let card = DidacticCard::new(
        "model.svg",
        &i18n("Neural model"),
        &model_picker::description(),
        None,
    );
    let initial = state.settings().output_filter.noise_reduction.model;
    let dropdown = model_picker::build(initial, {
        let state = Rc::clone(state);
        move |pick| {
            state.mutate(|s| s.output_filter.noise_reduction.model = pick);
        }
    });
    card.add_row(&labelled_row(&i18n("Model"), &dropdown));
    card
}

fn voice_recovery_card(state: &Rc<AppState>) -> DidacticCard {
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
    let row = percent_slider(state, &i18n("Recovery"), initial, |s, v| {
        s.output_filter.noise_reduction.voice_recovery = v;
    });
    card.add_row(&row);
    card
}

fn hpf_card(state: &Rc<AppState>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().output_filter.hpf.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.output_filter.hpf.enabled = on);
        });
    }
    DidacticCard::new(
        "hpf.svg",
        &i18n("High-pass filter"),
        &i18n("Cuts low-frequency rumble from playback — wind, AC noise."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    )
}

fn gate_card(state: &Rc<AppState>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().output_filter.gate.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.output_filter.gate.enabled = on);
        });
    }
    let card = DidacticCard::new(
        "gate.svg",
        &i18n("Silence gate"),
        &i18n("Mutes playback during silent moments so you don't hear hiss between words."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    );
    let initial = state.settings().output_filter.gate.intensity;
    let row = u8_slider(
        state,
        &i18n("Intensity"),
        initial,
        GATE_INTENSITY_MAX,
        |s, v| s.output_filter.gate.intensity = v,
    );
    card.add_row(&row);
    card
}

fn compressor_card(state: &Rc<AppState>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().output_filter.compressor.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.output_filter.compressor.enabled = on);
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
    let row = percent_slider(state, &i18n("Intensity"), initial, |s, v| {
        s.output_filter.compressor.intensity = v;
    });
    card.add_row(&row);
    card
}
