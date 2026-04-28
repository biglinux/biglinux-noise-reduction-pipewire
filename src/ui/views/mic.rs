//! Microphone Advanced-mode page.
//!
//! Every individual control (model picker, noise vs. voice strength,
//! voice presence, HPF, gate, compressor) sits in its own
//! `DidacticCard`. Beginners get a much simpler combined view from
//! [`super::simple::build`] — this page is shown only when the
//! `Advanced` switch in the header is on.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, Label, Orientation, ScrolledWindow};

use crate::config::{StereoMode, GATE_INTENSITY_MAX};
use crate::services::pipewire::source_volume;

use super::super::i18n::i18n;
use super::super::state::AppState;
use super::super::widgets::didactic::{
    group_separator, illustration, labelled_row, percent_slider, section_header, slider_row,
    switch_row, u32_slider, u8_slider, DidacticCard,
};
use super::super::widgets::eq_card::{build_eq_card, EqMutation};
use super::super::widgets::{model_picker, source_picker};

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

    content.append(&mic_combo_card(state));
    content.append(self_listen_delay_card(state).widget());
    content.append(echo_cancel_card(state).widget());

    content.append(&section_header(&i18n("Noise filter — fine-tune"), 16));
    content.append(model_card(state).widget());
    content.append(voice_recovery_card(state).widget());

    content.append(&section_header(&i18n("Voice enhancements"), 16));
    content.append(hpf_card(state).widget());
    content.append(gate_card(state).widget());
    content.append(compressor_card(state).widget());
    content.append(eq_card(state).widget());

    content.append(&section_header(&i18n("Voice changer"), 16));
    content.append(voice_changer_card(state).widget());

    scroll.set_child(Some(&content));
    scroll.upcast()
}

// ── Advanced-mode cards ────────────────────────────────────────────

/// Combined microphone card. The initial portion is byte-for-byte the
/// same layout as [`super::simple::mic_card`] (hardware picker →
/// separator → noise-filter master row → intensity slider →
/// self-listen toggle) so users switching between Simple and Advanced
/// don't lose orientation. Advanced complements it with extra cards
/// below — self-listen delay, echo cancel, fine-tune controls.
fn mic_combo_card(state: &Rc<AppState>) -> GtkBox {
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

/// Standalone Echo-cancellation card. Pulled out of the combo card so
/// power users can flip AEC on/off without scrolling and so the toggle
/// is obvious when diagnosing routing problems (a misbehaving AEC
/// virtual node will silence every app pulling from `mic-biglinux`).
fn echo_cancel_card(state: &Rc<AppState>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .valign(Align::Center)
        .active(state.settings().echo_cancel.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.echo_cancel.enabled = on);
        });
    }
    DidacticCard::new(
        "master_mic.svg",
        &i18n("Echo cancellation"),
        &i18n(
            "Removes speaker bleed from your microphone using the WebRTC \
             AEC. Turn off when wearing headphones or when troubleshooting \
             microphone routing.",
        ),
        Some(switch.upcast_ref()),
    )
}

/// `[SVG | title+desc | switch]` row matching `DidacticCard::new` so the
/// AI noise-filter master sits visually identical to the System sound
/// card and to the Simple-mode mic card.
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

fn model_card(state: &Rc<AppState>) -> DidacticCard {
    let card = DidacticCard::new(
        "model.svg",
        &i18n("Neural model"),
        &model_picker::description(),
        None,
    );
    let initial = state.settings().noise_reduction.model;
    let dropdown = model_picker::build(initial, {
        let state = Rc::clone(state);
        move |pick| state.mutate(|s| s.noise_reduction.model = pick)
    });
    card.add_row(&labelled_row(&i18n("Model"), &dropdown));
    card
}

fn voice_recovery_card(state: &Rc<AppState>) -> DidacticCard {
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
    let row = percent_slider(state, &i18n("Recovery"), initial, |s, v| {
        s.noise_reduction.voice_recovery = v;
    });
    card.add_row(&row);
    card
}

fn hpf_card(state: &Rc<AppState>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().hpf.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.hpf.enabled = on);
        });
    }
    DidacticCard::new(
        "hpf.svg",
        &i18n("High-pass filter"),
        &i18n("Cuts low-frequency rumble — AC, wind, mic-stand bumps."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    )
}

fn gate_card(state: &Rc<AppState>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().gate.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.gate.enabled = on);
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
    let row = u8_slider(
        state,
        &i18n("Intensity"),
        initial,
        GATE_INTENSITY_MAX,
        |s, v| s.gate.intensity = v,
    );
    card.add_row(&row);
    card
}

fn eq_card(state: &Rc<AppState>) -> DidacticCard {
    build_eq_card(
        state,
        i18n("Equalizer"),
        i18n(
            "10-band parametric EQ. Pick a preset or drag each band \
             between -40 dB and +40 dB.",
        ),
        |s| s.equalizer.clone(),
        |s, m| match m {
            EqMutation::Enabled(on) => s.equalizer.enabled = on,
            EqMutation::Preset(id) => id.clone_into(&mut s.equalizer.preset),
            EqMutation::Band { index, gain_db } => {
                if let Some(slot) = s.equalizer.bands.get_mut(index) {
                    *slot = gain_db;
                }
            }
        },
    )
}

/// Pitch-shift card (LADSPA `pitch_scale_1193` + amp gain compensation).
/// Width 0..1 maps exponentially to coefficient 0.5..2.0 — see
/// [`crate::pipeline::mic`] for the math.
fn voice_changer_card(state: &Rc<AppState>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(
            state.settings().stereo.enabled
                && state.settings().stereo.mode == StereoMode::VoiceChanger,
        )
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| {
                s.stereo.enabled = on;
                s.stereo.mode = if on {
                    StereoMode::VoiceChanger
                } else {
                    StereoMode::Mono
                };
            });
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
    let row = pitch_slider(state, initial);
    card.add_row(&row);
    card
}

/// Pitch slider with labelled marks at the five voice-changer presets.
/// Maps the underlying `stereo.width` (0.0..=1.0) to a 0..100 percent
/// scale the user sees. The marks make it easy to land on "Natural"
/// (50%) or one of the four flavored offsets without dragging by feel.
fn pitch_slider(state: &Rc<AppState>, initial: f32) -> GtkBox {
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
        let state = Rc::clone(state);
        adj.connect_value_changed(move |a| {
            let v = (a.value() / 100.0).clamp(0.0, 1.0) as f32;
            state.mutate(|cfg| cfg.stereo.width = v);
        });
    }

    slider_row(&i18n("Pitch"), &scale, &spin)
}

/// Calibration toggle that mirrors `super::simple::self_listen_row`:
/// a single `[label | switch]` row that loops the processed mic into
/// the default sink. Recommended for headphones only — loopback to
/// speakers creates acoustic feedback. The delay control lives in
/// the row below ([`self_listen_delay_row`]) so the toggle stays
/// uncluttered.
fn self_listen_row(state: &Rc<AppState>) -> GtkBox {
    let switch = gtk::Switch::builder()
        .valign(Align::Center)
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

/// Delay slider for the self-listen monitor. Sits in its own
/// `DidacticCard` directly under the mic combo card — Simple mode
/// hides the delay knob altogether, so Advanced surfaces it as a
/// separate refinement instead of bloating the mic card.
fn self_listen_delay_card(state: &Rc<AppState>) -> DidacticCard {
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
    let row = u32_slider(state, &i18n("Delay (ms)"), initial, 2000, |s, v| {
        s.monitor.delay_ms = v;
    });
    card.add_row(&row);
    card
}

fn compressor_card(state: &Rc<AppState>) -> DidacticCard {
    let switch = gtk::Switch::builder()
        .active(state.settings().compressor.enabled)
        .build();
    {
        let state = Rc::clone(state);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| s.compressor.enabled = on);
        });
    }
    let card = DidacticCard::new(
        "compressor.svg",
        &i18n("Compressor"),
        &i18n("Evens out loud peaks and quiet parts so your voice stays at a steady volume."),
        Some(switch.upcast_ref::<gtk::Widget>()),
    );
    let initial = state.settings().compressor.intensity;
    let row = percent_slider(state, &i18n("Intensity"), initial, |s, v| {
        s.compressor.intensity = v;
    });
    card.add_row(&row);
    card
}
