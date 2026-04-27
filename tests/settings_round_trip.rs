//! Settings persistence — load → mutate → save → load cycles, plus
//! schema-tolerance against legacy field names.

use biglinux_microphone::config::{
    AppSettings, CompressorConfig, GateConfig, NoiseModel, OutputFilterSettings,
};
use tempfile::tempdir;

#[test]
fn save_then_load_restores_every_field() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");

    let mut s = AppSettings::default();
    s.noise_reduction.enabled = true;
    s.noise_reduction.strength = 0.42;
    s.noise_reduction.model = NoiseModel::GtcrnVctk;
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
    let loaded = AppSettings::load_from(&path);
    assert_eq!(loaded, s);
}

#[test]
fn legacy_routed_apps_field_is_silently_dropped_on_load() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let raw = r#"{
        "output_filter": {
            "enabled": true,
            "routed_apps": ["Firefox", "Zoom"]
        }
    }"#;
    std::fs::write(&path, raw).unwrap();

    let s = AppSettings::load_from(&path);
    assert!(s.output_filter.enabled);
    // No `routed_apps` field exists on the new schema.
}

#[test]
fn malformed_file_falls_back_to_defaults() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, b"{ this is not json").unwrap();

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
