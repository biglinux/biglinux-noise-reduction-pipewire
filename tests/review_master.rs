use biglinux_microphone::config::{AppSettings, EchoMode};
use biglinux_microphone::pipeline::mic_chain_wanted;

#[test]
fn pausing_and_resuming_does_not_erase_selected_effects() {
    let mut settings = AppSettings::default();
    settings.gate.enabled = true;
    settings.equalizer.enabled = true;
    settings.echo_cancel.mode = EchoMode::Automatic;
    let original = settings.clone();
    settings.set_microphone_enabled(false);
    assert!(!mic_chain_wanted(&settings));
    assert!(settings.gate.enabled);
    assert_eq!(settings.echo_cancel.mode, EchoMode::Automatic);
    let runtime = settings.runtime_settings();
    assert!(!runtime.echo_cancel.enabled);
    settings.set_microphone_enabled(true);
    assert_eq!(settings, original);
}
