//! End-to-end exercise of [`pipeline::apply_to_dirs`].
//!
//! The unit tests inside `pipeline::tests` already cover individual
//! invariants (file presence, atomic writes, idempotency). This file
//! drives realistic scenarios: enabling a section, flipping it back
//! off, swapping models, switching the master toggle, and verifying
//! the on-disk output stays consistent across runs.

use std::fs;
use std::path::PathBuf;

use biglinux_microphone::config::{
    AppSettings, CompressorConfig, EchoCancelConfig, EqualizerConfig, GateConfig, HpfConfig,
    NoiseModel, NoiseReductionConfig, OutputFilterSettings, StereoConfig,
};
use biglinux_microphone::pipeline::{
    apply_to_dirs, mic_chain_wanted, ECHO_CANCEL_CONF_FILE, MIC_CONF_FILE, MIC_NODE_NAME,
    OUTPUT_CONF_FILE, OUTPUT_NODE_NAME,
};
use tempfile::tempdir;

fn dirs(t: &tempfile::TempDir) -> (PathBuf, PathBuf, PathBuf) {
    (
        t.path().join("pw-dropin"),
        t.path().join("pw-standalone"),
        t.path().join("wp"),
    )
}

fn fully_off() -> AppSettings {
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
        output_filter: OutputFilterSettings::default(),
        ..AppSettings::default()
    }
}

#[test]
fn enabling_only_noise_reduction_writes_mic_conf_with_smart_filter() {
    let dir = tempdir().unwrap();
    let (pw, std_dir, wp) = dirs(&dir);

    let mut s = fully_off();
    s.noise_reduction.enabled = true;
    // No-AEC path: WirePlumber's smart-filter policy inserts
    // mic-biglinux between every default-following recording app and
    // the user's hardware mic. The visible default stays the hw mic in
    // KDE / pavucontrol — only the captured audio gets filtered. AEC
    // defaults to on, so flip it off here to exercise the smart-filter
    // branch.
    s.echo_cancel.enabled = false;
    assert!(mic_chain_wanted(&s));

    apply_to_dirs(&s, &pw, &std_dir, &wp).unwrap();

    let conf = fs::read_to_string(pw.join(MIC_CONF_FILE)).unwrap();
    assert!(conf.contains(&format!("node.name = \"{MIC_NODE_NAME}\"")));
    assert!(conf.contains("filter.smart = true"));
    assert!(conf.contains("filter.smart.name = \"big.filter-microphone\""));
    assert!(
        !conf.contains("filter.smart.target"),
        "no target pin: smart filter must follow whichever source the user picks as default",
    );
    assert!(
        !conf.contains("filter.smart.before"),
        "no EC cascade when AEC is off",
    );
    assert!(conf.contains("\"Enable\" = 1.0"));
}

#[test]
fn switching_models_only_changes_the_model_control() {
    let dir = tempdir().unwrap();
    let (pw, std_dir, wp) = dirs(&dir);

    let mut s = fully_off();
    s.noise_reduction.enabled = true;
    apply_to_dirs(&s, &pw, &std_dir, &wp).unwrap();
    let dns3 = fs::read_to_string(pw.join(MIC_CONF_FILE)).unwrap();
    assert!(dns3.contains("\"Model\" = 0.0"));

    s.noise_reduction.model = NoiseModel::GtcrnVctk;
    apply_to_dirs(&s, &pw, &std_dir, &wp).unwrap();
    let vctk = fs::read_to_string(pw.join(MIC_CONF_FILE)).unwrap();
    assert!(vctk.contains("\"Model\" = 1.0"));
}

#[test]
fn output_chain_renders_smart_filter_in_bypass_when_master_off() {
    let dir = tempdir().unwrap();
    let (pw, std_dir, wp) = dirs(&dir);

    let s = AppSettings::default();
    assert!(!s.output_filter.enabled);
    apply_to_dirs(&s, &pw, &std_dir, &wp).unwrap();

    let conf = fs::read_to_string(std_dir.join(OUTPUT_CONF_FILE)).unwrap();
    // Smart-filter sink stays attached so streams don't get yanked when
    // the user toggles the master off — the chain just goes to bypass.
    assert!(conf.contains(&format!("node.name = \"{OUTPUT_NODE_NAME}\"")));
    assert!(conf.contains(&format!("filter.smart.name = \"{OUTPUT_NODE_NAME}\"")));
    // GTCRN stays wired with Enable=0; SWH gate goes to bypass via its
    // Output select control. Same node layout as the active path so the
    // live update can re-enable everything without restarting the unit.
    assert!(conf.contains("name = \"ai\""));
    assert!(conf.contains("\"Enable\" = 0.0"));
    assert!(conf.contains("\"Output select (-1 = key listen, 0 = gate, 1 = bypass)\" = 1.0"));
}

#[test]
fn enabling_master_then_disabling_keeps_conf_present() {
    let dir = tempdir().unwrap();
    let (pw, std_dir, wp) = dirs(&dir);

    let mut s = AppSettings::default();
    s.output_filter.enabled = true;
    apply_to_dirs(&s, &pw, &std_dir, &wp).unwrap();
    assert!(std_dir.join(OUTPUT_CONF_FILE).exists());

    s.output_filter.enabled = false;
    apply_to_dirs(&s, &pw, &std_dir, &wp).unwrap();
    assert!(
        std_dir.join(OUTPUT_CONF_FILE).exists(),
        "conf must remain so the unit can keep running in bypass"
    );
}

#[test]
fn legacy_per_app_routing_drop_in_is_scrubbed() {
    let dir = tempdir().unwrap();
    let (pw, std_dir, wp) = dirs(&dir);
    fs::create_dir_all(&wp).unwrap();
    let legacy = wp.join("50-biglinux-output-routing.conf");
    fs::write(&legacy, b"# stale\n").unwrap();

    apply_to_dirs(&AppSettings::default(), &pw, &std_dir, &wp).unwrap();
    assert!(!legacy.exists());
}

#[test]
fn echo_cancel_conf_is_written_into_filter_chain_dropin_dir_and_removed_when_disabled() {
    // Earlier revisions hosted the AEC config as a standalone
    // `pipewire -c` worker with its own systemd unit. The drop-in
    // consolidation puts it next to the mic chain so a single
    // `filter-chain.service` worker hosts both — one fewer pipewire
    // process. This test pins the path and the toggle behaviour.
    let dir = tempdir().unwrap();
    let (pw, std_dir, wp) = dirs(&dir);

    let on = AppSettings {
        echo_cancel: EchoCancelConfig { enabled: true },
        ..AppSettings::default()
    };
    apply_to_dirs(&on, &pw, &std_dir, &wp).unwrap();

    let dropin_path = pw.join(ECHO_CANCEL_CONF_FILE);
    assert!(
        dropin_path.exists(),
        "AEC drop-in must live in filter-chain.conf.d, not the standalone dir"
    );
    assert!(!std_dir.join(ECHO_CANCEL_CONF_FILE).exists());
    let body = fs::read_to_string(&dropin_path).unwrap();
    assert!(body.contains("libpipewire-module-echo-cancel"));
    // Drop-in must NOT redefine bootstrap modules / context properties:
    // those are owned by the host filter-chain.conf and would conflict.
    assert!(!body.contains("libpipewire-module-rt"));
    assert!(!body.contains("context.properties"));

    let off = AppSettings {
        echo_cancel: EchoCancelConfig { enabled: false },
        ..AppSettings::default()
    };
    apply_to_dirs(&off, &pw, &std_dir, &wp).unwrap();
    assert!(
        !dropin_path.exists(),
        "AEC drop-in must be removed when the toggle goes off"
    );
}

#[test]
fn round_trip_through_disk_produces_identical_conf() {
    let dir = tempdir().unwrap();
    let (pw, std_dir, wp) = dirs(&dir);

    let s = AppSettings::default();
    apply_to_dirs(&s, &pw, &std_dir, &wp).unwrap();
    let first = fs::read_to_string(pw.join(MIC_CONF_FILE)).unwrap();

    apply_to_dirs(&s, &pw, &std_dir, &wp).unwrap();
    let second = fs::read_to_string(pw.join(MIC_CONF_FILE)).unwrap();
    assert_eq!(first, second);
}
