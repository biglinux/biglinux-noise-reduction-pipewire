use biglinux_microphone::config::{AppSettings, GainSafety, OutputChannelMode};
use biglinux_microphone::pipeline::{build_mic_conf_for, build_output_conf_for};

#[test]
fn default_protection_is_last_on_each_processed_channel() {
    let settings = AppSettings::default();
    let mic = build_mic_conf_for(&settings);
    assert!(mic.contains(r#"{ output = "headroom:Out" input = "sample_ceiling:In" }"#));
    assert!(mic.contains(r#"{ output = "sample_ceiling:Out" input = "copy_l:In" }"#));
    let output = build_output_conf_for(&settings);
    assert!(output.contains("right_sample_ceiling"));
    assert!(output.contains(r#"outputs = [ "sample_ceiling:Out" "right_sample_ceiling:Out" ]"#));
}

#[test]
fn expert_opt_out_preserves_the_unrestricted_topology() {
    let settings = AppSettings {
        gain_safety: GainSafety::Unrestricted,
        ..AppSettings::default()
    };
    assert!(!build_mic_conf_for(&settings).contains("sample_ceiling"));
    assert!(!build_output_conf_for(&settings).contains("sample_ceiling"));
}

#[test]
fn mono_protection_is_applied_once_before_the_fanout() {
    let mut settings = AppSettings::default();
    settings.output_filter.channel_mode = OutputChannelMode::Mono;
    let output = build_output_conf_for(&settings);
    assert_eq!(output.matches(r#"name = "sample_ceiling""#).count(), 1);
    assert!(output.contains(r#"{ output = "sample_ceiling:Out" input = "copy_r:In" }"#));
}
