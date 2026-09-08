//! Neural-model dropdown shared by the mic and output Advanced cards.
//!
//! The available rows come from `big-audio-effects`, which owns the
//! serialized model identifiers and LADSPA plugin/label contract. Missing
//! optional plugins stay visible with a `not installed` suffix, but are
//! rendered dimmed and made non-selectable via the row's
//! `selectable=false` / `activatable=false` flags. That keeps the option
//! discoverable without letting the user point the realtime chain at a
//! plugin that is not installed.

use crate::config::noise_model::{NoiseModel, REALTIME_LAVFI_MODELS};
use gtk::prelude::*;

use super::super::i18n::i18n;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelChoice {
    model: NoiseModel,
    label: String,
    is_available: bool,
}

/// Build the model `DropDown` and wire its selection to `on_change`.
///
/// The initial selection is a preference. Unavailable models are explained and
/// cannot be selected as a new choice; native probing happens on workers.
pub fn build<F>(initial: NoiseModel, on_change: F) -> gtk::DropDown
where
    F: Fn(NoiseModel) + 'static,
{
    // Loadability, not bare file presence: a present-but-unloadable
    // plugin (broken native dependency) must not be selectable — picking
    // it would crash-loop the mic unit.
    build_with_availability(initial, on_change, NoiseModel::plugin_loadable_snapshot)
}

/// Capability input is injectable so display tests do not depend on LADSPA
/// packages installed on the runner.
pub(crate) fn build_with_availability<F>(
    initial: NoiseModel,
    on_change: F,
    available: impl Fn(NoiseModel) -> bool,
) -> gtk::DropDown
where
    F: Fn(NoiseModel) + 'static,
{
    let choices = model_choices(available);
    let labels: Vec<&str> = choices.iter().map(|choice| choice.label.as_str()).collect();
    let string_model = gtk::StringList::new(&labels);

    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let label = gtk::Label::builder().xalign(0.0).build();
        let Some(setup_list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        setup_list_item.set_child(Some(&label));
    });
    {
        let choices = choices.clone();
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

            // One question — is this row's plugin missing — drove three
            // fields of a struct whose only reader was right here.
            let is_disabled = selected_index_unavailable(bound_list_item.position(), &choices);
            bound_list_item.set_selectable(!is_disabled);
            bound_list_item.set_activatable(!is_disabled);
            if is_disabled {
                label.add_css_class("dimmed");
            } else {
                label.remove_css_class("dimmed");
            }
        });
    }

    let dropdown = gtk::DropDown::new(Some(string_model), gtk::Expression::NONE);
    dropdown.set_factory(Some(&factory));
    let initial_index = choices
        .iter()
        .position(|choice| choice.model == initial)
        .map_or_else(|| default_index(&choices), |index| index as u32);
    dropdown.set_selected(initial_index);
    dropdown.set_sensitive(choices.iter().any(|choice| choice.is_available));
    let previous = std::cell::Cell::new(initial_index);
    dropdown.connect_selected_notify(move |dropdown| {
        let selected = dropdown.selected();
        if selected_index_unavailable(selected, &choices) {
            dropdown.set_selected(previous.get());
            return;
        }
        previous.set(selected);
        on_change(index_to_model(selected, &choices));
    });

    dropdown
}

/// Description shown above the dropdown. Kept in this module so the model
/// guidance cannot drift between Microphone and Output cards.
#[must_use]
pub fn description() -> String {
    i18n(
        "DNS3 removes more noise but can smudge consonants. VCTK is gentler \
         and lighter on CPU. Full-band models can sound cleaner when their \
         LADSPA packages are installed, with higher CPU cost.",
    )
}

fn model_choices(is_available: impl Fn(NoiseModel) -> bool) -> Vec<ModelChoice> {
    REALTIME_LAVFI_MODELS
        .iter()
        .copied()
        .map(|model| {
            let is_available = is_available(model);
            let label = if is_available {
                i18n(model_label(model))
            } else {
                i18n("{model} - not installed").replace("{model}", &i18n(model_label(model)))
            };
            ModelChoice {
                model,
                label,
                is_available,
            }
        })
        .collect()
}

fn model_label(model: NoiseModel) -> &'static str {
    match model {
        NoiseModel::GtcrnDns3 => "GTCRN - DNS3 (16 kHz, strong)",
        NoiseModel::GtcrnVctk => "GTCRN - VCTK (16 kHz, gentle)",
        NoiseModel::DeepFilterNet3 => "DeepFilterNet3 (48 kHz)",
        NoiseModel::DpdfnetV2Hr => "DPDFNet-2 HR (48 kHz)",
        NoiseModel::DpdfnetV8Hr => "DPDFNet-8 HR (48 kHz)",
        NoiseModel::DpdfnetBaseline
        | NoiseModel::DpdfnetV2
        | NoiseModel::DpdfnetV4
        | NoiseModel::DpdfnetV8 => "Offline-only DPDFNet",
    }
}

fn selected_index_unavailable(selected_index: u32, choices: &[ModelChoice]) -> bool {
    choices
        .get(selected_index as usize)
        .is_none_or(|choice| !choice.is_available)
}

#[cfg(test)]
fn model_to_index(model: NoiseModel, choices: &[ModelChoice]) -> u32 {
    choices
        .iter()
        .position(|choice| choice.model == model && choice.is_available)
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or_else(|| default_index(choices))
}

fn index_to_model(selected_index: u32, choices: &[ModelChoice]) -> NoiseModel {
    choices
        .get(selected_index as usize)
        .filter(|choice| choice.is_available)
        .map(|choice| choice.model)
        .unwrap_or_default()
}

fn default_index(choices: &[ModelChoice]) -> u32 {
    choices
        .iter()
        .position(|choice| choice.model == NoiseModel::default() && choice.is_available)
        .or_else(|| choices.iter().position(|choice| choice.is_available))
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(gtk::INVALID_LIST_POSITION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_realtime_models_drive_choices() {
        let choices = model_choices(|_| true);

        assert_eq!(choices.len(), REALTIME_LAVFI_MODELS.len());
        assert_eq!(choices[0].model, NoiseModel::GtcrnDns3);
        assert!(
            choices
                .iter()
                .any(|choice| choice.model == NoiseModel::DpdfnetV8Hr)
        );
    }

    #[test]
    fn round_trip_available_indices() {
        let choices = model_choices(|_| true);

        for model in REALTIME_LAVFI_MODELS {
            let index = model_to_index(model, &choices);
            assert_eq!(index_to_model(index, &choices), model);
        }
    }

    #[test]
    fn missing_model_is_visible_but_not_selectable() {
        let choices = model_choices(|model| model != NoiseModel::DeepFilterNet3);
        let index = model_to_index(NoiseModel::DeepFilterNet3, &choices);
        let deepfilter_index = REALTIME_LAVFI_MODELS
            .iter()
            .position(|model| *model == NoiseModel::DeepFilterNet3)
            .and_then(|index| u32::try_from(index).ok())
            .expect("DeepFilterNet3 remains in realtime choices");

        assert_eq!(index, 0);
        assert!(
            choices[deepfilter_index as usize]
                .label
                .contains("not installed")
        );
        assert!(selected_index_unavailable(deepfilter_index, &choices));
    }

    #[test]
    fn unavailable_selected_index_falls_back_to_default() {
        let choices = model_choices(|model| model != NoiseModel::DpdfnetV2Hr);
        let unavailable_index = REALTIME_LAVFI_MODELS
            .iter()
            .position(|model| *model == NoiseModel::DpdfnetV2Hr)
            .and_then(|index| u32::try_from(index).ok())
            .expect("DPDFNet-2 HR remains in realtime choices");

        assert!(selected_index_unavailable(unavailable_index, &choices));
        assert_eq!(
            index_to_model(unavailable_index, &choices),
            NoiseModel::default()
        );
    }
}
