//! Settings persistence — load → mutate → save → load cycles and strict
//! rejection of fields outside the current schema.

use big_os_kit::storage::atomic_write;
use biglinux_microphone::config::{
    AppSettings, CompressorConfig, GateConfig, OutputFilterSettings,
};
use std::fs::read_to_string;
use tempfile::tempdir;

#[test]
fn save_then_load_restores_every_field() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");

    let mut s = AppSettings::default();
    s.noise_reduction.enabled = true;
    s.noise_reduction.strength = 0.42;
    s.noise_reduction.model = biglinux_microphone::config::NoiseModel::GtcrnVctk;
    s.gate = GateConfig {
        enabled: true,
        intensity: 17,
    };
    s.compressor = CompressorConfig {
        enabled: true,
        intensity: 0.55,
    };
    s.output_filter = OutputFilterSettings {
        enabled: true,
        ..OutputFilterSettings::default()
    };
    s.window.width = 950;
    s.window.height = 720;
    s.ui.show_advanced = true;

    s.save_to(&path).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["noise_reduction"]["model"], 1);
    let loaded = AppSettings::load_from(&path);
    // Reading preferences must not silently replace the selected model when
    // its runtime is unavailable. Availability is a separate worker decision.
    assert_eq!(loaded, s);
}

#[test]
fn unknown_output_filter_field_rejects_the_document_without_rewriting_it() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let raw = r#"{
        "output_filter": {
            "enabled": true,
            "unrecognized_setting": ["Firefox", "Zoom"]
        }
    }"#;
    atomic_write(&path, raw.as_bytes()).unwrap();

    let s = AppSettings::load_from(&path);
    assert_eq!(s, AppSettings::default());
    assert_eq!(read_to_string(&path).unwrap(), raw);
}

#[test]
fn malformed_file_falls_back_to_defaults() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");
    atomic_write(&path, b"{ this is not json").unwrap();

    let s = AppSettings::load_from(&path);
    assert_eq!(s, AppSettings::default());
}

#[test]
fn missing_file_falls_back_to_defaults() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("absent.json");
    let s = AppSettings::load_from(&path);
    assert_eq!(s, AppSettings::default());
}

#[test]
fn save_does_not_leave_temp_artefact() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");

    AppSettings::default().save_to(&path).unwrap();
    assert!(path.exists());
    assert!(!path.with_extension("tmp").exists());
}

#[test]
fn saved_settings_are_private_and_replace_a_symlink_without_touching_its_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempdir().unwrap();
    let victim = dir.path().join("unrelated");
    std::fs::write(&victim, b"unchanged").unwrap();
    let path = dir.path().join("settings.json");
    symlink(&victim, &path).unwrap();
    AppSettings::default().save_to(&path).unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"unchanged");
    assert!(!std::fs::symlink_metadata(&path).unwrap().is_symlink());
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn legacy_disabled_echo_remains_explicitly_off_without_rewriting_the_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let original = br#"{"echo_cancel":{"enabled":false}}"#;
    std::fs::write(&path, original).unwrap();
    let settings = AppSettings::load_from(&path);
    assert_eq!(
        settings.echo_cancel.mode,
        biglinux_microphone::config::EchoMode::Never
    );
    assert!(!settings.echo_cancel.enabled);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn legacy_selected_model_keeps_manual_quality() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut value = serde_json::to_value(AppSettings::default()).unwrap();
    value.as_object_mut().unwrap().remove("quality");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        AppSettings::load_from(&path).quality,
        biglinux_microphone::config::Quality::Manual
    );
}

#[test]
fn legacy_routed_apps_does_not_discard_valid_output_settings() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let bytes = br#"{"output_filter":{"enabled":true,"routed_apps":["Fixture"]}}"#;
    std::fs::write(&path, bytes).unwrap();
    assert!(AppSettings::load_from(&path).output_filter.enabled);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn a_saved_sixty_millisecond_lookahead_gives_way_to_the_new_default() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let bytes = br#"{"noise_reduction":{"lookahead_ms":60,"strength":0.5},
                     "output_filter":{"noise_reduction":{"lookahead_ms":60}}}"#;
    std::fs::write(&path, bytes).unwrap();
    let settings = AppSettings::load_from(&path);
    let fresh = AppSettings::default().noise_reduction.lookahead_ms;
    assert_eq!(settings.noise_reduction.lookahead_ms, fresh);
    assert_eq!(settings.output_filter.noise_reduction.lookahead_ms, fresh);
    // The 60 was the only thing dropped; everything beside it survives.
    assert!((settings.noise_reduction.strength - 0.5).abs() < f32::EPSILON);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn a_hand_picked_lookahead_is_left_alone() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, br#"{"noise_reduction":{"lookahead_ms":35}}"#).unwrap();
    assert_eq!(
        AppSettings::load_from(&path).noise_reduction.lookahead_ms,
        35
    );
}
