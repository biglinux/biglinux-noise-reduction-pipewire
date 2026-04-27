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

use std::path::PathBuf;
use std::process::Command;

use tempfile::tempdir;

fn cli_path() -> PathBuf {
    // `CARGO_BIN_EXE_<name>` is set by Cargo for every test target so
    // we can locate the just-compiled binary regardless of the build
    // profile.
    PathBuf::from(env!("CARGO_BIN_EXE_biglinux-microphone-cli"))
}

fn run(args: &[&str], xdg: &std::path::Path) -> std::process::Output {
    Command::new(cli_path())
        .args(args)
        .env("XDG_CONFIG_HOME", xdg)
        .env_remove("HOME")
        .env_remove("XDG_DATA_HOME")
        .output()
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
fn mic_conf_prints_pipewire_filter_chain_block() {
    // Default settings have AEC on, so the rendered conf must register
    // mic-biglinux as the public WirePlumber smart filter while pinning
    // its capture stream to the private echo-cancel source.
    let dir = tempdir().unwrap();
    let out = run(&["mic-conf"], dir.path());
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("libpipewire-module-filter-chain"));
    assert!(text.contains("filter.smart.name = \"big.filter-microphone\""));
    assert!(text.contains("target.object = \"echo-cancel-source\""));
    assert!(!text.contains("filter.smart.before = [ \"big.aec\" ]"));
    assert!(text.contains("mic-biglinux"));
}

#[test]
fn output_conf_prints_standalone_pipewire_wrapper() {
    let dir = tempdir().unwrap();
    let out = run(&["output-conf"], dir.path());
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("context.properties"));
    assert!(text.contains("libpipewire-module-protocol-native"));
    assert!(text.contains("output-biglinux"));
}
