//! End-to-end diagnostic for the noise-reduction stack.
//!
//! [`doctor`] probes every layer the GUI toggle relies on (LADSPA
//! plugins on disk, PipeWire daemon, WirePlumber, systemd user units,
//! generated configs, live graph nodes) and prints a numbered report.
//! Exit code = number of failed checks (0 = all green) so scripts can
//! branch on it.
//!
//! Kept out of the CLI binary itself so it can be reused from other
//! tools (a future GUI "diagnostics" pane, integration tests, packaging
//! smoke checks) without depending on the binary target.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::config::gtcrn_plugin;
use crate::pipeline;

const SC4_MONO_PLUGIN: &str = "/usr/lib/ladspa/sc4m_1916.so";
const SWH_GATE_PLUGIN: &str = "/usr/lib/ladspa/gate_1410.so";
const MIC_NODE_TAG: &str = "\"mic-biglinux\"";
const OUTPUT_NODE_TAG: &str = "\"output-biglinux\"";
const EC_NODE_TAG: &str = "\"echo-cancel-source\"";
const FILTER_CHAIN_UNIT: &str = "filter-chain.service";
const OUTPUT_UNIT: &str = "biglinux-microphone-output.service";
const WP_PACKAGED_LUA: &str = "/usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua";
const WP_USER_LUA_RELATIVE: &str = ".local/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua";

/// Where WirePlumber loads a user-local override of the AEC routing
/// script from. Anything in this path silently shadows the packaged
/// copy and is the single most common reason fresh package updates do
/// nothing on a developer's machine.
#[must_use]
pub fn user_local_wp_script_override() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(WP_USER_LUA_RELATIVE);
    if path.is_file() { Some(path) } else { None }
}

/// Run every probe and return an [`ExitCode`] equal to the failure count.
#[must_use]
pub fn doctor() -> ExitCode {
    let mut report = Report::default();
    println!(
        "biglinux-microphone-cli doctor {}\n",
        env!("CARGO_PKG_VERSION")
    );

    check_ladspa_plugins(&mut report);
    check_runtime_daemons(&mut report);
    check_systemd_units(&mut report);
    check_generated_configs(&mut report);
    check_graph_nodes(&mut report);
    check_echo_cancel(&mut report);
    check_wireplumber_script(&mut report);
    print_unit_state();

    println!();
    if report.failed == 0 {
        println!("doctor: all green");
        ExitCode::SUCCESS
    } else {
        println!(
            "doctor: {} check(s) failed — fix the FAIL lines above before \
             toggling from the GUI",
            report.failed,
        );
        ExitCode::FAILURE
    }
}

#[derive(Default)]
struct Report {
    failed: u8,
}

impl Report {
    fn check(&mut self, label: &str, ok: bool, detail: &str) {
        let mark = if ok { "ok " } else { "FAIL" };
        println!("[{mark}] {label}: {detail}");
        if !ok {
            self.failed = self.failed.saturating_add(1);
        }
    }
}

fn check_ladspa_plugins(report: &mut Report) {
    let gtcrn = gtcrn_plugin();
    report.check("GTCRN plugin", gtcrn.exists(), &gtcrn.display().to_string());
    let sc4 = Path::new(SC4_MONO_PLUGIN);
    report.check(
        "SC4 mono compressor (swh-plugins)",
        sc4.exists(),
        &sc4.display().to_string(),
    );
    let gate = Path::new(SWH_GATE_PLUGIN);
    report.check(
        "SWH gate plugin",
        gate.exists(),
        &gate.display().to_string(),
    );
}

fn check_runtime_daemons(report: &mut Report) {
    report.check(
        "PipeWire daemon",
        command_succeeds("pw-cli", &["info", "0"]),
        "pw-cli info 0",
    );
    report.check(
        "WirePlumber",
        command_succeeds("wpctl", &["status"]),
        "wpctl status",
    );
}

fn check_systemd_units(report: &mut Report) {
    report.check(
        "filter-chain.service unit",
        unit_known(FILTER_CHAIN_UNIT),
        "systemctl --user cat filter-chain.service",
    );
    report.check(
        "biglinux-microphone-output.service unit",
        unit_known(OUTPUT_UNIT),
        "systemctl --user cat biglinux-microphone-output.service",
    );
}

fn check_generated_configs(report: &mut Report) {
    let mic_path = pipeline::mic_conf_path();
    report.check(
        "mic conf written",
        mic_path.exists(),
        &mic_path.display().to_string(),
    );
    let out_path = pipeline::output_conf_path();
    report.check(
        "output conf written",
        out_path.exists(),
        &out_path.display().to_string(),
    );
}

/// AEC is opt-in: when the user has enabled it, the standalone unit is
/// expected to be active and `echo-cancel-source` should be visible in
/// the graph. When disabled, neither check applies — silently skip
/// instead of failing.
fn check_echo_cancel(report: &mut Report) {
    let settings = crate::config::AppSettings::load();
    if !settings.echo_cancel.enabled {
        println!("[skip] echo-cancel: AEC is disabled in settings");
        return;
    }
    let ec_path = pipeline::echo_cancel_conf_path();
    report.check(
        "echo-cancel conf written",
        ec_path.exists(),
        &ec_path.display().to_string(),
    );
    let graph_dump = Command::new("pw-cli")
        .args(["ls", "Node"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    report.check(
        "echo-cancel-source node visible",
        graph_dump.contains(EC_NODE_TAG),
        "pw-cli ls Node | grep echo-cancel-source",
    );

    let link_dump = Command::new("pw-link")
        .arg("-l")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let aec_ref_to_alsa = link_dump.lines().any(|l| {
        l.contains("alsa_output.") && l.contains(":monitor_") && {
            let next_line_aec = link_dump
                .lines()
                .skip_while(|x| *x != l)
                .nth(1)
                .is_some_and(|n| n.contains("echo-cancel-sink:input_"));
            next_line_aec
        }
    }) || link_dump.lines().any(|l| {
        l.contains("echo-cancel-sink:input_")
            && link_dump
                .lines()
                .skip_while(|x| *x != l)
                .take(8)
                .any(|n| n.contains("|<-") && n.contains("alsa_output.") && n.contains(":monitor_"))
    });
    report.check(
        "AEC reference linked to physical ALSA sink",
        aec_ref_to_alsa,
        "pw-link -l | grep -A1 echo-cancel-sink",
    );
}

/// Detect a stale `~/.local/share/wireplumber/scripts/biglinux/` copy
/// that masks the packaged routing hook. WirePlumber's base-dirs
/// lookup picks the user copy first, so an old version here means the
/// fix you just installed via pacman never runs.
fn check_wireplumber_script(report: &mut Report) {
    let installed = Path::new(WP_PACKAGED_LUA);
    report.check(
        "packaged AEC routing script installed",
        installed.exists(),
        WP_PACKAGED_LUA,
    );

    if let Some(override_path) = user_local_wp_script_override() {
        let same = match (
            std::fs::read(WP_PACKAGED_LUA),
            std::fs::read(&override_path),
        ) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        if same {
            println!(
                "[warn] WirePlumber script override at {} matches the packaged copy — \
                 harmless but redundant; consider removing it",
                override_path.display(),
            );
        } else {
            report.check(
                "no stale WirePlumber script override",
                false,
                &format!(
                    "{} shadows {} — delete the override so package updates apply",
                    override_path.display(),
                    WP_PACKAGED_LUA,
                ),
            );
        }
    }
}

fn check_graph_nodes(report: &mut Report) {
    let graph_dump = Command::new("pw-cli")
        .args(["ls", "Node"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    report.check(
        "mic-biglinux node visible",
        graph_dump.contains(MIC_NODE_TAG),
        "pw-cli ls Node | grep mic-biglinux",
    );
    report.check(
        "output-biglinux node visible",
        graph_dump.contains(OUTPUT_NODE_TAG),
        "pw-cli ls Node | grep output-biglinux",
    );
}

fn print_unit_state() {
    println!();
    let mic_state = unit_active_state(FILTER_CHAIN_UNIT);
    let out_state = unit_active_state(OUTPUT_UNIT);
    println!("filter-chain.service ................. {mic_state}");
    println!("biglinux-microphone-output.service ... {out_state}");

    if mic_state != "active" {
        dump_journal(FILTER_CHAIN_UNIT);
    }
    if out_state != "active" {
        dump_journal(OUTPUT_UNIT);
    }
}

fn command_succeeds(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn unit_known(name: &str) -> bool {
    command_succeeds("systemctl", &["--user", "cat", name])
}

fn unit_active_state(name: &str) -> String {
    Command::new("systemctl")
        .args(["--user", "is-active", name])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_or_else(
            |_| "unknown".to_owned(),
            |o| {
                let state = String::from_utf8_lossy(&o.stdout).trim().to_owned();
                if state.is_empty() {
                    "unknown".to_owned()
                } else {
                    state
                }
            },
        )
}

/// When a unit reports `failed` or `inactive`, dump the tail of its
/// journal so users don't need a second `journalctl` round trip.
fn dump_journal(unit: &str) {
    println!();
    println!("--- last journal lines for {unit} ---");
    let out = Command::new("journalctl")
        .args([
            "--user",
            "-u",
            unit,
            "-n",
            "30",
            "--no-pager",
            "--output",
            "short",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => {
            print!("{}", String::from_utf8_lossy(&o.stdout));
        }
        Ok(_) => println!("(journalctl returned non-zero — not enough permissions?)"),
        Err(e) => println!("(journalctl unavailable: {e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_starts_at_zero_failures() {
        let r = Report::default();
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn report_increments_on_failure_only() {
        let mut r = Report::default();
        r.check("ok", true, "passes");
        r.check("bad", false, "fails");
        r.check("ok2", true, "passes");
        assert_eq!(r.failed, 1);
    }

    #[test]
    fn report_failure_count_saturates() {
        let mut r = Report {
            failed: u8::MAX - 1,
        };
        r.check("a", false, "");
        r.check("b", false, "");
        r.check("c", false, "");
        assert_eq!(r.failed, u8::MAX);
    }

    #[test]
    fn unit_active_state_returns_string_for_unknown_unit() {
        let s = unit_active_state("definitely-not-a-real-unit-xyz.service");
        assert!(!s.is_empty());
    }
}
