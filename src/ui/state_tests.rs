use super::*;

use crate::config::{
    CompressorConfig, EqualizerConfig, GateConfig, HpfConfig, NoiseReductionConfig,
    OutputFilterSettings, StereoConfig,
};

fn all_off() -> AppSettings {
    AppSettings {
        noise_reduction: NoiseReductionConfig {
            enabled: false,
            ..NoiseReductionConfig::default()
        },
        gate: GateConfig {
            enabled: false,
            ..GateConfig::default()
        },
        hpf: HpfConfig {
            enabled: false,
            ..HpfConfig::default()
        },
        stereo: StereoConfig {
            enabled: false,
            ..StereoConfig::default()
        },
        equalizer: EqualizerConfig {
            enabled: false,
            ..EqualizerConfig::default()
        },
        compressor: CompressorConfig {
            enabled: false,
            ..CompressorConfig::default()
        },
        ..AppSettings::default()
    }
}

fn with_nr_enabled(enabled: bool) -> AppSettings {
    AppSettings {
        noise_reduction: NoiseReductionConfig {
            enabled,
            ..NoiseReductionConfig::default()
        },
        ..all_off()
    }
}

#[test]
fn no_reload_when_only_control_values_change() {
    let prev = with_nr_enabled(true);
    let mut next = prev.clone();
    next.noise_reduction.strength = 0.5;
    assert!(!needs_mic_reload(Some(&prev), &next));
}

#[test]
fn reload_when_chain_goes_from_unwanted_to_wanted() {
    let prev = with_nr_enabled(false);
    let next = with_nr_enabled(true);
    assert!(needs_mic_reload(Some(&prev), &next));
}

#[test]
fn reload_when_eq_bands_change() {
    let prev = with_nr_enabled(true);
    let mut next = prev.clone();
    next.equalizer = EqualizerConfig {
        enabled: true,
        bands: vec![3.0; 10],
        ..EqualizerConfig::default()
    };
    assert!(needs_mic_reload(Some(&prev), &next));
}

#[test]
fn reload_on_first_apply_when_chain_wanted() {
    let next = with_nr_enabled(true);
    assert!(needs_mic_reload(None, &next));
}

#[test]
fn output_topology_unchanged_when_only_enable_flag_toggles() {
    let prev = AppSettings {
        output_filter: OutputFilterSettings {
            enabled: true,
            ..OutputFilterSettings::default()
        },
        ..AppSettings::default()
    };
    let mut next = prev.clone();
    next.output_filter.noise_reduction.strength = 0.4;
    assert!(!output_topology_changed(Some(&prev), &next));
}

#[test]
fn reload_when_mic_compressor_node_added_or_removed() {
    // Compressor is now a topology-conditional node — the SC4
    // LADSPA gets dropped from the graph entirely when off, so
    // toggling the flag must reload the chain instead of going
    // through the live-controls path.
    let prev = with_nr_enabled(true);
    let mut next = prev.clone();
    next.compressor.enabled = true;
    assert!(needs_mic_reload(Some(&prev), &next));
}

#[test]
fn reload_when_mic_ai_node_added_or_removed() {
    // NR + gate both off → AI node skipped. Turning the gate on
    // brings the node back into the graph: that's a topology
    // change, the live-controls fast path can't satisfy it.
    let prev = AppSettings {
        gate: GateConfig {
            enabled: false,
            ..GateConfig::default()
        },
        hpf: HpfConfig {
            enabled: true,
            ..HpfConfig::default()
        },
        noise_reduction: NoiseReductionConfig {
            enabled: false,
            ..NoiseReductionConfig::default()
        },
        ..all_off()
    };
    let mut next = prev.clone();
    next.gate.enabled = true;
    assert!(needs_mic_reload(Some(&prev), &next));
}

#[test]
fn reload_when_mic_hpf_toggles() {
    // Enabling HPF inserts a second `bq_highpass` (`hpf_pre`)
    // ahead of `hpf` to form a 4th-order Linkwitz-Riley cascade —
    // can't be done by live-updating control values alone.
    let prev = AppSettings {
        hpf: HpfConfig {
            enabled: false,
            ..HpfConfig::default()
        },
        ..AppSettings::default()
    };
    let mut next = prev.clone();
    next.hpf.enabled = true;
    assert!(needs_mic_reload(Some(&prev), &next));
}

#[test]
fn mono_nr_toggle_rebuilds_the_conditional_neural_stage() {
    // Mono and stereo use the same conditional neural stage.
    let prev = AppSettings {
        output_filter: OutputFilterSettings {
            enabled: true,
            noise_reduction: NoiseReductionConfig {
                enabled: false,
                ..NoiseReductionConfig::default()
            },
            channel_mode: crate::config::OutputChannelMode::Mono,
            ..OutputFilterSettings::default()
        },
        ..AppSettings::default()
    };
    let mut next = prev.clone();
    next.output_filter.noise_reduction.enabled = true;
    assert!(output_topology_changed(Some(&prev), &next));
}

#[test]
fn output_topology_changed_when_eq_bands_change() {
    let prev = AppSettings {
        output_filter: OutputFilterSettings {
            enabled: true,
            ..OutputFilterSettings::default()
        },
        ..AppSettings::default()
    };
    let mut next = prev.clone();
    next.output_filter.equalizer = EqualizerConfig {
        enabled: true,
        bands: vec![4.0; 10],
        ..EqualizerConfig::default()
    };
    assert!(output_topology_changed(Some(&prev), &next));
}

#[test]
fn successful_completion_records_the_applied_snapshot() {
    let state = AppState::new(AppSettings::default());
    let mut snapshot = AppSettings::default();
    snapshot.noise_reduction.strength = 0.42;
    let request = ApplyRequest::new(ApplyRevision::default(), ApplyGeneration::default());

    let result = state.finish_apply(ApplyCompletion::from_outcome(
        request,
        ApplyOutcome {
            snapshot: snapshot.clone(),
            loopback: None,
            was_persisted: true,
            status: ApplyStatus::Applied,
        },
    ));

    assert_eq!(result, Ok(()));
    assert_eq!(state.last_applied.borrow().as_ref(), Some(&snapshot));
}

#[test]
fn failed_completion_preserves_previous_snapshot_and_reports_cause() {
    let state = AppState::new(AppSettings::default());
    let previous = with_nr_enabled(true);
    *state.last_applied.borrow_mut() = Some(previous.clone());
    let request = ApplyRequest::new(ApplyRevision::default(), ApplyGeneration::default());

    let result = state.finish_apply(ApplyCompletion::from_outcome(
        request,
        ApplyOutcome {
            snapshot: AppSettings::default(),
            loopback: None,
            was_persisted: true,
            status: ApplyStatus::Failed("audio service restart failed".to_owned()),
        },
    ));

    assert_eq!(result, Err("audio service restart failed".to_owned()));
    assert_eq!(state.last_applied.borrow().as_ref(), Some(&previous));
}

#[test]
fn worker_failure_keeps_request_identity_and_reports_internal_error() {
    let state = AppState::new(AppSettings::default());
    let request = ApplyRequest::new(ApplyRevision::default(), ApplyGeneration::default());
    let completion = ApplyCompletion::worker_failed(request);

    assert_eq!(completion.request(), request);
    assert_eq!(
        state.finish_apply(completion),
        Err("internal error while applying settings".to_owned())
    );
}

#[test]
fn external_replace_preserves_apply_baseline_until_worker_succeeds() {
    let state = AppState::new(AppSettings::default());
    let previous = with_nr_enabled(false);
    *state.last_applied.borrow_mut() = Some(previous.clone());
    let incoming = with_nr_enabled(true);

    assert!(state.external_replace(incoming.clone()));
    assert_eq!(&*state.settings(), &incoming);
    assert_eq!(state.last_applied.borrow().as_ref(), Some(&previous));
}

#[test]
fn file_monitor_cannot_roll_newer_edits_back_to_an_in_flight_local_snapshot() {
    let state = AppState::new(AppSettings::default());
    let request = ApplyRequest::new(ApplyRevision::default(), ApplyGeneration::default());
    let local_snapshot = state.settings().clone();
    let _work = state.apply_work(request);
    state.mutate(|settings| settings.noise_reduction.strength = 0.73);

    assert!(!state.external_replace(local_snapshot.clone()));
    assert_eq!(state.settings().noise_reduction.strength, 0.73);

    assert_eq!(
        state.finish_apply(ApplyCompletion::worker_failed(request)),
        Err("internal error while applying settings".to_owned())
    );
    let next_request = ApplyRequest::new(request.revision().next(), request.generation().next());
    let _next_work = state.apply_work(next_request);
    assert!(!state.external_replace(local_snapshot));
    assert_eq!(state.settings().noise_reduction.strength, 0.73);
}

#[test]
fn stereo_nr_toggle_rebuilds_the_conditional_neural_stage() {
    let prev = AppSettings {
        output_filter: OutputFilterSettings {
            enabled: true,
            ..OutputFilterSettings::default()
        },
        ..AppSettings::default()
    };
    let mut next = prev.clone();
    next.output_filter.noise_reduction.enabled = !prev.output_filter.noise_reduction.enabled;
    assert!(output_topology_changed(Some(&prev), &next));
}
