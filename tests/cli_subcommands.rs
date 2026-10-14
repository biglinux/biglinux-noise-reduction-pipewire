//! CLI subcommand integration tests.
//!
//! Exercises the read-only subcommands of `biglinux-microphone-cli`
//! that don't touch the user's PipeWire daemon (`mic-conf`,
//! `output-conf`, `settings`, `help`). Subcommands that talk to
//! systemd or the live graph are out of scope here — they need a real
//! session bus and are covered by manual / integration runs.
//!
//! Each test runs the binary built by `cargo` with a custom
//! `XDG_CONFIG_HOME` so it never reads or writes the developer's real
//! settings file.

#![cfg(not(miri))]

use big_os_kit::subprocess::{BigSubprocessOutput, BigSubprocessSpec};

use tempfile::tempdir;

fn cli_path() -> String {
    // `CARGO_BIN_EXE_<name>` is set by Cargo for every test target so
    // we can locate the just-compiled binary regardless of the build
    // profile.
    env!("CARGO_BIN_EXE_biglinux-microphone-cli").to_owned()
}

fn run(args: &[&str], xdg: &std::path::Path) -> BigSubprocessOutput {
    let program = cli_path();
    BigSubprocessSpec::builder()
        .program(program.clone())
        .args(args)
        .env("XDG_CONFIG_HOME", xdg)
        .env_inherit(false)
        .allow_list([program])
        .build()
        .run()
        .expect("failed to spawn cli")
}

#[test]
fn help_subcommand_lists_known_commands() {
    let dir = tempdir().unwrap();
    let out = run(&["help"], dir.path());
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for cmd in [
        "settings",
        "mic-conf",
        "output-conf",
        "apply",
        "remove",
        "live-update",
        "toggle-mic",
        "toggle-output",
        "status",
        "doctor",
    ] {
        assert!(text.contains(cmd), "help missing `{cmd}`:\n{text}");
    }
}

#[test]
fn unknown_subcommand_exits_with_failure() {
    let dir = tempdir().unwrap();
    let out = run(&["does-not-exist"], dir.path());
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown command"));
}

#[test]
fn settings_dump_returns_pretty_json() {
    let dir = tempdir().unwrap();
    let out = run(&["settings"], dir.path());
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    // Pretty-printed JSON: at least one newline, opens with a brace.
    assert!(text.starts_with('{'));
    assert!(text.contains('\n'));
    // Field names from the typed model.
    for k in [
        "noise_reduction",
        "gate",
        "compressor",
        "hpf",
        "output_filter",
    ] {
        assert!(text.contains(k), "settings dump missing `{k}`:\n{text}");
    }
}

#[test]
fn corrupt_settings_are_reported_without_replacing_them() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("biglinux-microphone/settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{broken").unwrap();
    let out = run(&["settings"], dir.path());
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "{broken");
}

#[test]
fn status_prints_single_line_json_with_two_booleans() {
    let dir = tempdir().unwrap();
    let out = run(&["status"], dir.path());
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.trim();
    // Plasma applet parses this with JSON.parse; assert the shape.
    assert!(line.starts_with('{') && line.ends_with('}'));
    assert!(line.contains("\"mic_enabled\":"));
    assert!(line.contains("\"output_enabled\":"));
}

#[test]
fn mic_conf_prints_module_args_body() {
    // The mic conf is now a bare `{ ... }` module args body fed to
    // `libpipewire-module-filter-chain` by `biglinux-microphone-pwloader`
    // — no `context.modules` wrapper, no comment header. Default
    // settings have AEC on, so the body must still pin the capture
    // stream to the private echo-cancel source and expose mic-biglinux
    // as the public smart filter.
    let dir = tempdir().unwrap();
    let out = run(&["mic-conf"], dir.path());
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.trim_start().starts_with('{'));
    assert!(text.trim_end().ends_with('}'));
    assert!(!text.contains("context.modules"));
    assert!(!text.contains("libpipewire-module-filter-chain"));
    assert!(text.contains("filter.smart.name = \"big.filter-microphone\""));
    assert!(text.contains("target.object = \"echo-cancel-source\""));
    assert!(!text.contains("filter.smart.before = [ \"big.aec\" ]"));
    assert!(text.contains("mic-biglinux"));
}

#[test]
fn output_conf_prints_module_args_body() {
    // Output conf is now also a bare module-args body — the standalone
    // `pipewire -c` wrapper is gone; the same pwloader binary loads
    // libpipewire-module-filter-chain inside its own client context.
    let dir = tempdir().unwrap();
    let out = run(&["output-conf"], dir.path());
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.trim_start().starts_with('{'));
    assert!(text.trim_end().ends_with('}'));
    assert!(!text.contains("context.properties"));
    assert!(!text.contains("libpipewire-module-protocol-native"));
    assert!(text.contains("output-biglinux"));
}

#[test]
fn writing_eq_bands_or_a_preset_never_switches_the_equalizer_on() {
    // The equalizer is off until somebody asks for it. Sending the sliders'
    // current position — which the window does whenever a preset is rendered —
    // must stay a write to `bands`, not a decision to shape the microphone.
    // The value is persisted before the graph is reconciled, and reconciling
    // needs a PipeWire this harness deliberately does not have — so read the
    // file rather than the exit status.
    let dir = tempdir().unwrap();
    let saved = |dir: &std::path::Path| -> serde_json::Value {
        let raw = std::fs::read_to_string(dir.join("biglinux-microphone/settings.json"))
            .expect("settings written");
        serde_json::from_str(&raw).expect("valid json")
    };
    for (key, value) in [
        ("mic-eq-bands", "3,0,0,0,0,0,0,0,0,0"),
        ("mic-eq-preset", "voice_boost"),
    ] {
        run(&["set", key, value], dir.path());
        assert_eq!(
            saved(dir.path())["equalizer"]["enabled"],
            false,
            "`set {key}` enabled the equalizer by itself"
        );
    }
    run(&["set", "mic-equalizer", "on"], dir.path());
    assert_eq!(saved(dir.path())["equalizer"]["enabled"], true);
}

#[test]
fn eq_rejects_non_finite_gains_without_saving() {
    for invalid in ["NaN", "inf", "-inf"] {
        let dir = tempdir().unwrap();
        let value = format!("{invalid},0,0,0,0,0,0,0,0,0");
        let out = run(&["set", "mic-eq-bands", &value], dir.path());
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("finite"));
        assert!(
            !dir.path()
                .join("biglinux-microphone/settings.json")
                .exists()
        );
    }
}
