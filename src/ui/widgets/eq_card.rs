//! 10-band parametric equalizer card.
//!
//! Shared between the Advanced mic view (`s.equalizer`) and the
//! Advanced output view (`s.output_filter.equalizer`) — the caller
//! provides a reader closure that hands back a snapshot of the relevant
//! [`EqualizerConfig`] and a writer that mutates it back into the
//! settings tree. This indirection keeps the widget agnostic about
//! which sub-tree it operates on.
//!
//! Layout:
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │ [SVG] Equalizer                                  [Switch]│
//! │       10-band parametric EQ. Pick a preset or             │
//! │       fine-tune each band manually.                       │
//! │                                                           │
//! │       Preset: [▼ Voice boost ]                            │
//! │       31  63  125 250 500 1k  2k  4k  8k  16k             │
//! │       │   │   │   │   │   │   │   │   │   │              │
//! │       (vertical sliders, ±40 dB)                          │
//! └──────────────────────────────────────────────────────────┘
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, DropDown, Label, Orientation, Scale};

use crate::config::{
    eq_preset_bands, eq_preset_ids, AppSettings, EqualizerConfig, EQ_BANDS_HZ, EQ_BAND_COUNT,
    EQ_BAND_MAX, EQ_BAND_MIN,
};

use super::super::i18n::i18n;
use super::super::state::AppState;
use super::didactic::DidacticCard;

/// Pieces of [`EqualizerConfig`] the widget needs to mutate. The view
/// closure decides where in the settings tree these land.
pub enum EqMutation {
    Enabled(bool),
    Preset(&'static str),
    Band { index: usize, gain_db: f32 },
}

/// Build an Equalizer card. `read` returns a snapshot of the current
/// config; `write` applies a single field mutation back into the
/// settings tree.
pub fn build_eq_card(
    state: &Rc<AppState>,
    title: String,
    description: String,
    read: impl Fn(&AppSettings) -> EqualizerConfig + 'static,
    write: impl Fn(&mut AppSettings, EqMutation) + 'static,
) -> DidacticCard {
    let initial = read(&state.settings());
    let write = Rc::new(write);
    let _ = read; // accessor was only needed for the initial snapshot

    let switch = gtk::Switch::builder().active(initial.enabled).build();
    {
        let state = Rc::clone(state);
        let write = Rc::clone(&write);
        switch.connect_active_notify(move |sw| {
            let on = sw.is_active();
            state.mutate(|s| write(s, EqMutation::Enabled(on)));
        });
    }

    let card = DidacticCard::new(
        "equalizer.svg",
        &title,
        &description,
        Some(switch.upcast_ref::<gtk::Widget>()),
    );

    let preset_dropdown = preset_dropdown(&initial.preset);
    let band_scales: Vec<Scale> = (0..EQ_BAND_COUNT).map(|_| build_band_scale()).collect();
    let band_scales = Rc::new(band_scales);

    apply_initial_band_values(&band_scales, &initial.bands);

    // RefCell guards "we are programmatically setting band values
    // because the user picked a preset" — without it the per-band
    // change handlers below would loop back into `Preset(custom)`.
    let suppress: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    {
        let state = Rc::clone(state);
        let write = Rc::clone(&write);
        let band_scales = Rc::clone(&band_scales);
        let suppress = Rc::clone(&suppress);
        let ids = eq_preset_ids();
        preset_dropdown.connect_selected_notify(move |dd| {
            let Some(id) = ids.get(dd.selected() as usize).copied() else {
                return;
            };
            let Some(bands) = eq_preset_bands(id) else {
                return;
            };
            *suppress.borrow_mut() = true;
            for (scale, gain) in band_scales.iter().zip(bands.iter()) {
                scale.adjustment().set_value(f64::from(*gain));
            }
            *suppress.borrow_mut() = false;
            state.mutate(|s| {
                write(s, EqMutation::Preset(id));
                for (i, gain) in bands.iter().enumerate() {
                    write(
                        s,
                        EqMutation::Band {
                            index: i,
                            gain_db: *gain,
                        },
                    );
                }
            });
        });
    }

    for (idx, scale) in band_scales.iter().enumerate() {
        let state = Rc::clone(state);
        let write = Rc::clone(&write);
        let suppress = Rc::clone(&suppress);
        scale.adjustment().connect_value_changed(move |a| {
            if *suppress.borrow() {
                return;
            }
            let gain_db = (a.value() as f32).clamp(EQ_BAND_MIN, EQ_BAND_MAX);
            state.mutate(|s| {
                write(
                    s,
                    EqMutation::Band {
                        index: idx,
                        gain_db,
                    },
                );
                write(s, EqMutation::Preset("custom"));
            });
        });
    }

    card.add_row(&preset_row(&preset_dropdown));
    card.add_row(&bands_row(&band_scales));
    card
}

fn preset_dropdown(initial_id: &str) -> DropDown {
    let labels: Vec<String> = eq_preset_ids().into_iter().map(preset_label).collect();
    let labels_ref: Vec<&str> = labels.iter().map(String::as_str).collect();
    let dd = DropDown::from_strings(&labels_ref);
    let ids = eq_preset_ids();
    if let Some(idx) = ids.iter().position(|id| *id == initial_id) {
        dd.set_selected(idx as u32);
    }
    dd.set_hexpand(true);
    dd
}

/// Translatable display name for a preset id.
fn preset_label(id: &str) -> String {
    match id {
        "default_voice" => i18n("Default voice"),
        "flat" => i18n("Flat"),
        "voice_boost" => i18n("Voice boost"),
        "podcast" => i18n("Podcast"),
        "warm" => i18n("Warm"),
        "bright" => i18n("Bright"),
        "de_esser" => i18n("De-esser"),
        "bass_cut" => i18n("Bass cut"),
        "presence" => i18n("Presence"),
        "custom" => i18n("Custom"),
        other => other.to_owned(),
    }
}

fn build_band_scale() -> Scale {
    let adj = gtk::Adjustment::new(
        0.0,
        f64::from(EQ_BAND_MIN),
        f64::from(EQ_BAND_MAX),
        1.0,
        5.0,
        0.0,
    );
    let scale = Scale::new(Orientation::Vertical, Some(&adj));
    scale.set_inverted(true);
    scale.set_draw_value(true);
    scale.set_value_pos(gtk::PositionType::Bottom);
    scale.set_digits(0);
    scale.set_height_request(140);
    scale.set_width_request(48);
    scale.set_valign(Align::Fill);
    scale.add_mark(0.0, gtk::PositionType::Right, None);
    scale
}

fn apply_initial_band_values(scales: &[Scale], bands: &[f32]) {
    for (scale, gain) in scales.iter().zip(bands.iter()) {
        scale.adjustment().set_value(f64::from(*gain));
    }
}

fn preset_row(dropdown: &DropDown) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(12)
        .build();
    let label = Label::builder()
        .label(i18n("Preset"))
        .xalign(0.0)
        .width_request(140)
        .build();
    row.append(&label);
    row.append(dropdown);
    row
}

fn bands_row(scales: &[Scale]) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(16)
        .halign(Align::Center)
        .build();

    for (scale, freq) in scales.iter().zip(EQ_BANDS_HZ.iter()) {
        let column = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .build();
        column.append(scale);
        let freq_label = Label::builder()
            .label(format_freq(*freq))
            .css_classes(vec!["caption", "dim-label"])
            .build();
        column.append(&freq_label);
        row.append(&column);
    }
    row
}

fn format_freq(hz: u32) -> String {
    if hz >= 1000 {
        format!("{}k", hz / 1000)
    } else {
        format!("{hz}")
    }
}
