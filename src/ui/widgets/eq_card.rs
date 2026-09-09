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

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, DropDown, Label, Orientation, Scale};

use crate::config::{
    EQ_BAND_COUNT, EQ_BAND_MAX, EQ_BAND_MIN, EQ_BANDS_HZ, EqualizerConfig, eq_preset_bands,
    eq_preset_ids,
};

use super::super::i18n::i18n;
use super::didactic::DidacticCard;

/// Pieces of [`EqualizerConfig`] the widget needs to mutate. Emitted as a
/// typed message; [`apply_eq_mutation`] applies it into the settings tree.
#[derive(Debug, Clone, Copy)]
pub enum EqMutation {
    Enabled(bool),
    Preset(&'static str),
    Band { index: usize, gain_db: f32 },
}

/// Build an Equalizer card from an `initial` snapshot. Each control change is
/// reported via `apply` as a single [`EqMutation`] (the Relm4 message path —
/// the component's `update` routes it to state through [`apply_eq_mutation`]).
pub fn build_eq_card(
    initial: EqualizerConfig,
    title: String,
    description: String,
    apply: impl Fn(EqMutation) + 'static,
) -> DidacticCard {
    let apply = Rc::new(apply);

    let switch = gtk::Switch::builder().active(initial.enabled).build();
    switch.update_property(&[gtk::accessible::Property::Label(&i18n("Equalizer enabled"))]);
    {
        let apply = Rc::clone(&apply);
        switch.connect_active_notify(move |sw| apply(EqMutation::Enabled(sw.is_active())));
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

    for (scale, gain) in band_scales.iter().zip(initial.bands.iter()) {
        scale.adjustment().set_value(f64::from(*gain));
    }

    // RefCell guards "we are programmatically setting band values
    // because the user picked a preset" — without it the per-band
    // change handlers below would loop back into `Preset(custom)`.
    let suppress = Rc::new(Cell::new(false));
    {
        let apply = Rc::clone(&apply);
        let band_scales = Rc::clone(&band_scales);
        let suppress = Rc::clone(&suppress);
        let ids = eq_preset_ids();
        preset_dropdown.connect_selected_notify(move |dd| {
            if suppress.get() {
                return;
            }
            let Some(id) = ids.get(dd.selected() as usize).copied() else {
                return;
            };
            let Some(bands) = eq_preset_bands(id) else {
                return;
            };
            suppress.set(true);
            for (scale, gain) in band_scales.iter().zip(bands.iter()) {
                scale.adjustment().set_value(f64::from(*gain));
            }
            suppress.set(false);
            apply(EqMutation::Preset(id));
        });
    }

    for (idx, scale) in band_scales.iter().enumerate() {
        let apply = Rc::clone(&apply);
        let suppress = Rc::clone(&suppress);
        // Weak: the dropdown already owns the scales through its callback.
        let dropdown = preset_dropdown.downgrade();
        let custom = eq_preset_ids().iter().position(|id| *id == "custom");
        scale.set_widget_name(&format!("eq-band-{idx}"));
        scale.adjustment().connect_value_changed(move |a| {
            if suppress.get() {
                return;
            }
            let gain_db = (a.value() as f32).clamp(EQ_BAND_MIN, EQ_BAND_MAX);
            apply(EqMutation::Band {
                index: idx,
                gain_db,
            });
            if let (Some(dropdown), Some(index)) = (dropdown.upgrade(), custom) {
                suppress.set(true);
                dropdown.set_selected(index as u32);
                suppress.set(false);
            }
        });
    }

    card.add_row(&preset_row(&preset_dropdown));
    card.add_row(&bands_row(&band_scales));
    card
}

/// Apply one [`EqMutation`] into an [`EqualizerConfig`] — the reducer shared by
/// the mic and output EQ message handlers in `MicShell::update`.
pub fn apply_eq_mutation(eq: &mut EqualizerConfig, mutation: EqMutation) {
    match mutation {
        EqMutation::Enabled(on) => eq.enabled = on,
        EqMutation::Preset(id) => {
            id.clone_into(&mut eq.preset);
            if let Some(bands) = eq_preset_bands(id) {
                eq.bands = bands.to_vec();
            }
        }
        EqMutation::Band { index, gain_db } => {
            if let Some(slot) = eq.bands.get_mut(index) {
                *slot = gain_db.clamp(EQ_BAND_MIN, EQ_BAND_MAX);
                "custom".clone_into(&mut eq.preset);
            }
        }
    }
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
    dd.set_widget_name("eq-preset");
    dd.update_property(&[gtk::accessible::Property::Label(&i18n("Preset"))]);
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

fn preset_row(dropdown: &DropDown) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(12)
        .build();
    let label = Label::builder().label(i18n("Preset")).xalign(0.0).build();
    row.append(&label);
    row.append(dropdown);
    row
}

fn bands_row(scales: &[Scale]) -> gtk::FlowBox {
    let row = gtk::FlowBox::builder()
        .orientation(Orientation::Horizontal)
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(2)
        .max_children_per_line(EQ_BAND_COUNT as u32)
        .column_spacing(8)
        .row_spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(16)
        .halign(Align::Center)
        .build();

    for (scale, freq) in scales.iter().zip(EQ_BANDS_HZ.iter()) {
        // The visible caption below the slider is not programmatically tied to
        // it; stamp an explicit accessible name so a screen reader announces
        // which frequency band each slider adjusts.
        scale.update_property(&[gtk::accessible::Property::Label(&format!(
            "{} {freq} Hz",
            i18n("Equalizer band")
        ))]);
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

#[cfg(test)]
pub(in crate::ui) fn assert_interaction_contract() {
    fn find(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if widget.widget_name() == name {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find(&current, name) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
    let initial = EqualizerConfig::default();
    let preset_id = initial.preset.clone();
    let expected = initial.bands.clone();
    let state = Rc::new(std::cell::RefCell::new(initial.clone()));
    let changed = Rc::clone(&state);
    let card = build_eq_card(initial, "Equalizer".into(), String::new(), move |event| {
        apply_eq_mutation(&mut changed.borrow_mut(), event);
    });
    let root: &gtk::Widget = card.widget().upcast_ref();
    let dropdown = find(root, "eq-preset")
        .unwrap()
        .downcast::<DropDown>()
        .unwrap();
    let scale = find(root, "eq-band-0")
        .unwrap()
        .downcast::<Scale>()
        .unwrap();
    scale.set_value(7.0);
    let ids = eq_preset_ids();
    assert_eq!(ids[dropdown.selected() as usize], "custom");
    assert_eq!(state.borrow().preset, "custom");
    assert_eq!(state.borrow().bands[0], 7.0);
    dropdown.set_selected(ids.iter().position(|id| *id == preset_id).unwrap() as u32);
    assert_eq!(state.borrow().bands, expected);
    // Minimum requested width must not contain ten side-by-side columns.
    let minimum = root.measure(Orientation::Horizontal, -1).0;
    assert!(minimum < 400, "equalizer minimum width: {minimum}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_mutation_updates_id_and_bands_together() {
        let mut equalizer = EqualizerConfig::default();

        apply_eq_mutation(&mut equalizer, EqMutation::Preset("voice_boost"));

        assert_eq!(equalizer.preset, "voice_boost");
        assert_eq!(equalizer.bands, eq_preset_bands("voice_boost").unwrap());
    }
}
