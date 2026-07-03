//! Neural-model dropdown shared by the mic and output Advanced cards.
//!
//! Three rows, always present, in the same order as
//! [`crate::config::NoiseModel`]:
//!
//! 1. GTCRN – DNS3 (16 kHz, strong)
//! 2. GTCRN – VCTK (16 kHz, gentle)
//! 3. DeepFilterNet3 (48 kHz)
//!
//! When the DFN3 LADSPA plugin isn't installed, row 3 stays in the
//! list with a `not installed` suffix, but is rendered greyed-out and
//! made non-selectable via the row's `selectable=false` /
//! `activatable=false` flags. That keeps the option discoverable
//! (users learn the optdep exists) without letting them pick something
//! that would fail to instantiate at the LADSPA layer.
//!
//! The mic and output views share this helper so the labels stay in
//! lockstep — every label change here lands in both Advanced pages
//! atomically without each view drifting on its own.

use gtk::prelude::*;

use crate::config::{deepfilter_available, NoiseModel};

use super::super::i18n::i18n;

/// Build the model `DropDown` and wire its selection to `on_change`.
///
/// `initial` is the persisted model from `AppSettings`. If the user
/// has DFN3 saved but the plugin is now missing, the caller is
/// expected to have already demoted it via
/// [`crate::config::AppSettings::demote_unavailable_models`]; this
/// builder still treats DFN3 defensively and falls back to row 0
/// when the plugin is unavailable.
pub fn build<F>(initial: NoiseModel, on_change: F) -> gtk::DropDown
where
    F: Fn(NoiseModel) + 'static,
{
    let dfn3_present = deepfilter_available();

    let dfn3_label = if dfn3_present {
        i18n("DeepFilterNet3 (48 kHz)")
    } else {
        i18n("DeepFilterNet3 (48 kHz) — not installed")
    };
    let entries = [
        i18n("GTCRN – DNS3 (16 kHz, strong)"),
        i18n("GTCRN – VCTK (16 kHz, gentle)"),
        dfn3_label,
    ];

    let str_refs: Vec<&str> = entries.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&str_refs);

    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let label = gtk::Label::builder().xalign(0.0).build();
        let Some(setup_list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        setup_list_item.set_child(Some(&label));
    });
    factory.connect_bind(move |_, list_item| {
        let Some(bound_list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(string) = bound_list_item.item().and_downcast::<gtk::StringObject>() else {
            return;
        };
        let Some(label) = bound_list_item.child().and_downcast::<gtk::Label>() else {
            return;
        };
        label.set_label(&string.string());

        // Index 2 is DFN3. When the plugin is missing, render the row
        // greyed and refuse selection so users see the option is real
        // but can't point the chain at a plugin that isn't there.
        let row_interaction = dfn3_row_interaction(bound_list_item.position(), dfn3_present);
        bound_list_item.set_selectable(row_interaction.is_selectable);
        bound_list_item.set_activatable(row_interaction.is_activatable);
        if row_interaction.is_dimmed {
            label.add_css_class("dim-label");
        } else {
            label.remove_css_class("dim-label");
        }
    });

    let dropdown = gtk::DropDown::new(Some(model), gtk::Expression::NONE);
    dropdown.set_factory(Some(&factory));
    dropdown.set_selected(model_to_index(initial, dfn3_present));

    // Snap selection back if a keyboard navigation or accessibility
    // tool ever bypasses the row-level `selectable=false` and lands
    // on the disabled DFN3 row.
    dropdown.connect_selected_notify(move |dd| {
        let idx = dd.selected();
        if selected_index_requires_dfn3_snapback(idx, dfn3_present) {
            dd.set_selected(model_to_index(NoiseModel::GtcrnDns3, dfn3_present));
            return;
        }
        on_change(index_to_model(idx, dfn3_present));
    });

    dropdown
}

/// Description shown above the dropdown. Kept in this module so the
/// "premium" wording can never resurface in only one of the two cards.
#[must_use]
pub fn description() -> String {
    i18n(description_message(deepfilter_available()))
}

fn description_message(dfn3_present: bool) -> &'static str {
    if dfn3_present {
        "DNS3 removes more noise but can smudge consonants. VCTK is \
         gentler and lighter on CPU. DeepFilterNet3 is a full-band \
         48 kHz model with the cleanest output and the highest CPU \
         cost."
    } else {
        "DNS3 removes more noise but can smudge consonants. VCTK is \
         gentler and lighter on CPU — good for podcasts."
    }
}

const DFN3_INDEX: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelRowInteraction {
    is_selectable: bool,
    is_activatable: bool,
    is_dimmed: bool,
}

fn dfn3_row_interaction(row_position: u32, dfn3_present: bool) -> ModelRowInteraction {
    let is_disabled = is_dfn3_row_disabled(row_position, dfn3_present);
    ModelRowInteraction {
        is_selectable: !is_disabled,
        is_activatable: !is_disabled,
        is_dimmed: is_disabled,
    }
}

fn is_dfn3_row_disabled(row_position: u32, dfn3_present: bool) -> bool {
    row_position == DFN3_INDEX && !dfn3_present
}

fn selected_index_requires_dfn3_snapback(selected_index: u32, dfn3_present: bool) -> bool {
    selected_index == DFN3_INDEX && !dfn3_present
}

fn model_to_index(model: NoiseModel, dfn3_present: bool) -> u32 {
    match model {
        NoiseModel::GtcrnVctk => 1,
        NoiseModel::DeepFilterNet3 if dfn3_present => DFN3_INDEX,
        // Persisted DFN3 with the package uninstalled falls back to
        // the strong GTCRN preset so the dropdown stays in a valid
        // state — `demote_unavailable_models()` normally rewrites the
        // setting on load, this branch is a defensive net.
        NoiseModel::GtcrnDns3 | NoiseModel::DeepFilterNet3 => 0,
    }
}

fn index_to_model(idx: u32, dfn3_present: bool) -> NoiseModel {
    match idx {
        1 => NoiseModel::GtcrnVctk,
        2 if dfn3_present => NoiseModel::DeepFilterNet3,
        _ => NoiseModel::GtcrnDns3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_indices() {
        for present in [true, false] {
            for model in [
                NoiseModel::GtcrnDns3,
                NoiseModel::GtcrnVctk,
                NoiseModel::DeepFilterNet3,
            ] {
                let idx = model_to_index(model, present);
                let back = index_to_model(idx, present);
                if !present && matches!(model, NoiseModel::DeepFilterNet3) {
                    assert_eq!(back, NoiseModel::GtcrnDns3);
                } else {
                    assert_eq!(back, model);
                }
            }
        }
    }

    #[test]
    fn dfn3_index_is_last() {
        assert_eq!(DFN3_INDEX, 2);
    }

    #[test]
    fn missing_dfn3_is_visible_but_not_selectable() {
        assert!(!is_dfn3_row_disabled(0, false));
        assert!(!is_dfn3_row_disabled(1, false));
        assert!(is_dfn3_row_disabled(DFN3_INDEX, false));
        assert!(!is_dfn3_row_disabled(DFN3_INDEX, true));

        assert!(selected_index_requires_dfn3_snapback(DFN3_INDEX, false));
        assert!(!selected_index_requires_dfn3_snapback(DFN3_INDEX, true));
        assert!(!selected_index_requires_dfn3_snapback(1, false));
    }

    #[test]
    fn dfn3_row_interaction_dims_only_missing_dfn3() {
        assert_eq!(
            dfn3_row_interaction(DFN3_INDEX, false),
            ModelRowInteraction {
                is_selectable: false,
                is_activatable: false,
                is_dimmed: true,
            }
        );
        assert_eq!(
            dfn3_row_interaction(DFN3_INDEX, true),
            ModelRowInteraction {
                is_selectable: true,
                is_activatable: true,
                is_dimmed: false,
            }
        );
        assert_eq!(
            dfn3_row_interaction(1, false),
            ModelRowInteraction {
                is_selectable: true,
                is_activatable: true,
                is_dimmed: false,
            }
        );
    }

    #[test]
    fn dfn3_index_falls_back_when_plugin_is_missing() {
        assert_eq!(model_to_index(NoiseModel::DeepFilterNet3, false), 0);
        assert_eq!(index_to_model(DFN3_INDEX, false), NoiseModel::GtcrnDns3);
        assert_eq!(model_to_index(NoiseModel::DeepFilterNet3, true), DFN3_INDEX);
        assert_eq!(index_to_model(DFN3_INDEX, true), NoiseModel::DeepFilterNet3);
    }

    #[test]
    fn description_messages_name_available_gtcrn_models() {
        for text in [description_message(false), description_message(true)] {
            assert!(text.contains("DNS3"), "{text}");
            assert!(text.contains("VCTK"), "{text}");
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn localized_description_names_available_gtcrn_models() {
        let text = description();

        assert!(text.contains("DNS3"), "{text}");
        assert!(text.contains("VCTK"), "{text}");
    }
}
