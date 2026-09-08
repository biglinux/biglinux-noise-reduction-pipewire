//! Capability checks for the effects actually requested, never a fixed GTCRN gate.
use super::i18n::i18n;
use crate::config::{AppSettings, NoiseModel};
use crate::diagnostics::{command_succeeds, unit_known};
use crate::services::pipewire::{AEC_UNIT, MIC_UNIT, OUTPUT_UNIT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    Ready,
    Unavailable { cause: String, hint: String },
}

/// Blocking native probes run on the health worker. Refreshing can discover a
/// runtime installed since the window opened, including a previously missing one.
pub fn probe(settings: &AppSettings) -> Health {
    crate::config::noise_model::refresh_runtime_availability();
    check(
        settings,
        command_succeeds("/usr/bin/pw-cli", &["info", "0"]),
        NoiseModel::plugin_loadable_cached,
        unit_known,
    )
}

fn check(
    settings: &AppSettings,
    reachable: bool,
    model_available: impl Fn(NoiseModel) -> bool,
    unit_available: impl Fn(&str) -> bool,
) -> Health {
    if !reachable {
        return Health::Unavailable {
            cause: i18n("The audio system (PipeWire) is not responding."),
            hint: i18n("Check the audio connection, then check again. Your preferences are kept."),
        };
    }
    let effective = settings.runtime_settings();
    let mut models = Vec::with_capacity(2);
    if crate::pipeline::ai_node_in_mic_chain(&effective) {
        models.push(effective.noise_reduction.model);
    }
    if crate::pipeline::output_ai_processing(&effective) {
        models.push(effective.output_filter.noise_reduction.model);
    }
    if let Some(model) = models.into_iter().find(|model| !model_available(*model)) {
        return Health::Unavailable {
            cause: i18n("The selected noise model is unavailable: {model}.")
                .replace("{model}", &format!("{model:?}")),
            hint: i18n(
                "Choose another installed model, or install this model and its required runtime, then check again.",
            ),
        };
    }
    let units = [
        (crate::pipeline::mic_chain_wanted(&effective), MIC_UNIT),
        (effective.output_filter.enabled, OUTPUT_UNIT),
        (effective.echo_cancel.enabled, AEC_UNIT),
    ];
    if units
        .into_iter()
        .any(|(wanted, unit)| wanted && !unit_available(unit))
    {
        return Health::Unavailable {
            cause: i18n("The background audio services are not installed."),
            hint: i18n("Reinstall Filter noise, then log out and back in."),
        };
    }
    Health::Ready
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equalizer_only_does_not_require_a_neural_runtime() {
        let mut settings = AppSettings::default();
        crate::pipeline::cascade_mic_off(&mut settings);
        settings.equalizer.enabled = true;
        assert_eq!(
            check(&settings, true, |_| false, |unit| unit == MIC_UNIT),
            Health::Ready
        );
    }

    #[test]
    fn alternative_model_is_not_blocked_by_missing_gtcrn() {
        let mut settings = AppSettings::default();
        settings.noise_reduction.model = NoiseModel::DeepFilterNet3;
        assert_eq!(
            check(
                &settings,
                true,
                |model| model == NoiseModel::DeepFilterNet3,
                |_| true
            ),
            Health::Ready
        );
        assert!(matches!(
            check(&settings, true, |_| false, |_| true),
            Health::Unavailable { .. }
        ));
    }

    #[test]
    fn bypass_does_not_require_disabled_microphone_services() {
        let settings = AppSettings {
            mic_bypass: true,
            ..AppSettings::default()
        };
        assert_eq!(check(&settings, true, |_| false, |_| false), Health::Ready);
    }
}
