use biglinux_microphone::config::{AppSettings, OutputChannelMode};
use biglinux_microphone::pipeline::build_output_conf_for;

#[test]
fn default_playback_keeps_two_independent_channels() {
    let settings = AppSettings::default();
    let graph = build_output_conf_for(&settings);
    assert!(graph.contains("right_hpf"));
    assert!(graph.contains("right_eq"));
    assert!(!graph.contains("mixer:In"));
}

#[test]
fn mono_requires_an_explicit_setting() {
    let mut settings = AppSettings::default();
    settings.output_filter.channel_mode = OutputChannelMode::Mono;
    let graph = build_output_conf_for(&settings);
    assert!(graph.contains("mixer:In 1"));
    assert!(!graph.contains("right_hpf"));
}

#[test]
fn equalizer_only_does_not_require_a_neural_runtime() {
    let mut settings = AppSettings::default();
    settings.output_filter.enabled = true;
    settings.output_filter.noise_reduction.enabled = false;
    settings.output_filter.equalizer.enabled = true;
    let graph = build_output_conf_for(&settings);
    assert!(!graph.contains("libgtcrn"));
    assert!(graph.contains("right_eq"));
}
