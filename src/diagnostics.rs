//! End-to-end diagnostic for the noise-reduction stack.
//!
//! `doctor` probes every layer the GUI toggle relies on (LADSPA
//! plugins on disk, PipeWire daemon, WirePlumber, systemd user units,
//! generated configs, live graph nodes) and prints a numbered report.
//! Exit code = number of failed checks (0 = all green) so scripts can
//! branch on it.
//!
//! Kept out of the CLI binary itself so it can be reused from other
//! tools (a future GUI "diagnostics" pane, integration tests, packaging
//! smoke checks) without depending on the binary target.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

fn read_bounded(path: &Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(std::io::Error::other("diagnostic file exceeds size limit"));
    }
    Ok(bytes)
}

use crate::config::{AppSettings, NoiseModel};
use crate::pipeline;
use crate::services::pipewire::{AEC_UNIT, MIC_UNIT, OUTPUT_UNIT, has_capture_consumer};

const SC4_MONO_PLUGIN: &str = "/usr/lib/ladspa/sc4m_1916.so";
const SWH_GATE_PLUGIN: &str = "/usr/lib/ladspa/gate_1410.so";
const MIC_NODE_TAG: &str = "\"mic-biglinux\"";
const OUTPUT_NODE_TAG: &str = "\"output-biglinux\"";
const EC_NODE_TAG: &str = "\"echo-cancel-source\"";
const WP_PACKAGED_LUA: &str = "/usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua";
const WP_USER_LUA_RELATIVE: &str =
    ".local/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua";

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
    let settings = AppSettings::load();
    println!(
        "biglinux-microphone-cli doctor {}\n",
        env!("CARGO_PKG_VERSION")
    );

    check_ladspa_plugins(&mut report);
    check_runtime_daemons(&mut report);
    check_systemd_units(&mut report);
    check_generated_configs(&mut report, &settings);
    check_graph_nodes(&mut report, &settings);
    check_graph_transport(&mut report, &settings);
    check_live_controls(&mut report, &settings);
    check_echo_cancel(&mut report, &settings);
    check_wireplumber_script(&mut report);
    print_unit_state(&settings);

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
        ExitCode::from(report.failed)
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
    for model in NoiseModel::ALL {
        let (path, _label) = model.plugin_and_label();
        report.check(&format!("{model:?} plugin"), Path::new(path).exists(), path);
    }
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
        command_succeeds("/usr/bin/pw-cli", &["info", "0"]),
        "pw-cli info 0",
    );
    report.check(
        "WirePlumber",
        command_succeeds("/usr/bin/wpctl", &["status"]),
        "wpctl status",
    );
}

fn check_systemd_units(report: &mut Report) {
    report.check(
        "biglinux-microphone-mic.service unit",
        unit_known(MIC_UNIT),
        "systemctl --user cat biglinux-microphone-mic.service",
    );
    report.check(
        "biglinux-microphone-aec.service unit",
        unit_known(AEC_UNIT),
        "systemctl --user cat biglinux-microphone-aec.service",
    );
    report.check(
        "biglinux-microphone-output.service unit",
        unit_known(OUTPUT_UNIT),
        "systemctl --user cat biglinux-microphone-output.service",
    );
}

fn check_generated_configs(report: &mut Report, settings: &AppSettings) {
    let mic_path = pipeline::mic_conf_path();
    if pipeline::mic_chain_wanted(settings) {
        report.check(
            "mic conf written",
            mic_path.exists(),
            &mic_path.display().to_string(),
        );
    } else {
        report.check(
            "mic conf absent while disabled",
            !mic_path.exists(),
            &mic_path.display().to_string(),
        );
    }
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
fn check_echo_cancel(report: &mut Report, settings: &AppSettings) {
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
    // A stale conf left behind by an older version (e.g. one that shipped the
    // removed WebRTC engine) would keep a different canceller active despite the
    // current setting (review DIAG-02). We ship a single engine, so the conf must
    // declare exactly it.
    if let Ok(bytes) = read_bounded(&ec_path, 64 * 1024) {
        let requested = pipeline::AEC_LIBRARY;
        match aec_backend_in_conf(&String::from_utf8_lossy(&bytes)) {
            Some(active) => report.check(
                "AEC backend matches the requested engine",
                active == requested,
                &format!("requested {requested}, conf loads {active}"),
            ),
            None => report.check(
                "AEC backend declared in conf",
                false,
                "no library.name found in the echo-cancel conf",
            ),
        }
    }
    let graph_dump = crate::services::pipewire::graph_nodes_dump();
    report.check(
        "echo-cancel-source node visible",
        graph_dump.contains(EC_NODE_TAG),
        "pw-cli ls Node | grep echo-cancel-source",
    );

    let link_dump = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-link")
        .arg("-l")
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/pw-link"])
        .build()
        .run()
        .map(|o| o.stdout_lossy())
        .unwrap_or_default();
    match aec_reference_state(&link_dump) {
        AecReference::Linked => report.check(
            "AEC reference linked to physical ALSA sink",
            true,
            "pw-link -l | grep -A1 echo-cancel-sink",
        ),
        AecReference::NothingPlaying => println!(
            "[skip] AEC reference: nothing is playing, so there is no speaker \
             signal to cancel yet"
        ),
        AecReference::NothingRecording => println!(
            "[skip] AEC reference: no application is recording, and the routing \
             hook keeps the reference unlinked until one is"
        ),
        AecReference::Missing => report.check(
            "AEC reference linked to physical ALSA sink",
            false,
            "pw-link -l | grep -A1 echo-cancel-sink",
        ),
    }
}

/// The `library.name` the written echo-cancel conf will load — i.e. the backend
/// that is actually active. Differs from the requested backend when a stale conf
/// was not regenerated after switching engines (review DIAG-02).
fn aec_backend_in_conf(conf: &str) -> Option<String> {
    conf.lines()
        .filter_map(|line| line.split_once("library.name"))
        .find_map(|(_, rest)| {
            rest.trim_start()
                .trim_start_matches('=')
                .split_whitespace()
                .next()
        })
        .map(str::to_owned)
}

/// What the graph says about the canceller's reference signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AecReference {
    /// A physical sink's monitor feeds `echo-cancel-sink`.
    Linked,
    /// No application is playing, so WirePlumber has no reference to route.
    NothingPlaying,
    /// Nothing is recording, so the routing hook's own gate keeps the
    /// reference unlinked on purpose.
    NothingRecording,
    /// Something is playing, something is recording, and the reference is
    /// still absent.
    Missing,
}

/// Decide from a `pw-link -l` dump alone, so the rule can be tested.
///
/// Requiring the reference unconditionally made `doctor` fail on an idle
/// desktop — observed on the maintainer's workstation, every check green except
/// this one, exit code 1 — and a diagnostic that cries wolf is one people learn
/// to ignore. The reference only exists while something plays.
fn aec_reference_state(link_dump: &str) -> AecReference {
    let lines: Vec<&str> = link_dump.lines().collect();
    let monitor_into_sink = lines.iter().enumerate().any(|(index, line)| {
        line.contains("alsa_output.")
            && line.contains(":monitor_")
            && lines
                .get(index + 1)
                .is_some_and(|next| next.contains("echo-cancel-sink:input_"))
    });
    let sink_from_monitor = lines.iter().enumerate().any(|(index, line)| {
        line.contains("echo-cancel-sink:input_")
            && lines.iter().skip(index + 1).take(8).any(|next| {
                next.contains("|<-") && next.contains("alsa_output.") && next.contains(":monitor_")
            })
    });
    if monitor_into_sink || sink_from_monitor {
        return AecReference::Linked;
    }
    // A running playback stream shows up as a link into the sink's own
    // playback ports; without one there is nothing for the canceller to hear.
    let playback_running = lines
        .iter()
        .any(|line| line.contains("alsa_output.") && line.contains(":playback_"));
    if !playback_running {
        return AecReference::NothingPlaying;
    }
    // The routing hook unlinks the reference tap whenever no application is
    // recording, so that playing music does not drag the physical microphone
    // into RUNNING. Demanding the link while that gate is closed made `doctor`
    // fail on a desktop that was working correctly — observed here with a
    // browser playing and nothing recording, and green the moment a recorder
    // appeared.
    if !has_capture_consumer(&lines) {
        return AecReference::NothingRecording;
    }
    AecReference::Missing
}

/// Detect a stale `~/.local/share/wireplumber/scripts/biglinux/` copy
/// that masks the packaged routing hook. WirePlumber's base-dirs
/// lookup picks the user copy first, so a stale copy here prevents the
/// packaged script installed by pacman from running.
fn check_wireplumber_script(report: &mut Report) {
    let installed = Path::new(WP_PACKAGED_LUA);
    report.check(
        "packaged AEC routing script installed",
        installed.exists(),
        WP_PACKAGED_LUA,
    );

    if let Some(override_path) = user_local_wp_script_override() {
        let same = match (
            read_bounded(Path::new(WP_PACKAGED_LUA), 512 * 1024),
            read_bounded(&override_path, 512 * 1024),
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

fn check_graph_nodes(report: &mut Report, settings: &AppSettings) {
    let graph_dump = crate::services::pipewire::graph_nodes_dump();
    // Judge the graph against the *desired* state: a node that the user
    // turned off is correctly absent, not a failure.
    if pipeline::mic_chain_wanted(settings) {
        report.check(
            "mic-biglinux node visible",
            graph_dump.contains(MIC_NODE_TAG),
            "pw-cli ls Node | grep mic-biglinux",
        );
    } else {
        report.check(
            "mic-biglinux node absent while disabled",
            !graph_dump.contains(MIC_NODE_TAG),
            "pw-cli ls Node | grep mic-biglinux",
        );
    }
    if settings.output_filter.enabled {
        report.check(
            "output-biglinux node visible",
            graph_dump.contains(OUTPUT_NODE_TAG),
            "pw-cli ls Node | grep output-biglinux",
        );
    } else {
        report.check(
            "output-biglinux node absent while disabled",
            !graph_dump.contains(OUTPUT_NODE_TAG),
            "pw-cli ls Node | grep output-biglinux",
        );
    }
}

/// Distinguish a node that merely exists from one that is actually transporting
/// frames (review HEALTH-01). Presence is [`check_graph_nodes`]' job; this adds
/// the node's PipeWire state and whether any link is carrying audio. Only a node
/// stuck in `error` is a hard failure — a `running` node with no links is idle
/// (no app is recording/playing yet), not broken, so it must not cry wolf.
fn check_graph_transport(report: &mut Report, settings: &AppSettings) {
    let Ok(graph) = crate::services::pipewire::graph_snapshot() else {
        return;
    };
    let wanted = [
        (
            "mic",
            pipeline::MIC_CAPTURE_NODE_NAME,
            pipeline::mic_chain_wanted(settings),
        ),
        (
            "output",
            pipeline::OUTPUT_NODE_NAME,
            settings.output_filter.enabled,
        ),
    ];
    for (chain, node_name, want) in wanted {
        if !want {
            continue;
        }
        let Some(transport) = crate::services::pipewire::node_transport(&graph, node_name) else {
            continue;
        };
        let detail = if transport.active_links > 0 {
            format!(
                "state={}, {} active link(s) — transporting",
                transport.state, transport.active_links
            )
        } else {
            format!(
                "state={}, no active links (idle: no consumer yet)",
                transport.state
            )
        };
        report.check(
            &format!("{chain} node not stuck in error"),
            transport.state != "error",
            &detail,
        );
    }
}

/// Confirm the control keys the daemon pushes live are actually recognised by
/// the running node. `pw-cli` drops unknown keys silently, so a typo or a stale
/// control name looks like a successful apply while the value never takes effect
/// (review API-01). Only runs against a live, running node; a suspended or
/// absent node has no schema to check and is left to [`check_graph_nodes`].
fn check_live_controls(report: &mut Report, settings: &AppSettings) {
    let Ok(graph) = crate::services::pipewire::graph_snapshot() else {
        return;
    };
    let effective = settings.runtime_settings();
    let audits: [(&str, &str, Vec<(String, f64)>); 2] = [
        (
            "mic",
            pipeline::MIC_CAPTURE_NODE_NAME,
            crate::services::pipewire::mic_params(&effective),
        ),
        (
            "output",
            pipeline::OUTPUT_NODE_NAME,
            crate::services::pipewire::output_params(&effective),
        ),
    ];
    for (chain, node_name, sent) in audits {
        let Some(unknown) =
            crate::services::pipewire::unknown_controls_for(&graph, node_name, &sent)
        else {
            continue;
        };
        let detail = if unknown.is_empty() {
            format!("{} live controls recognised", sent.len())
        } else {
            format!("silently dropped by pw-cli: {}", unknown.join(", "))
        };
        report.check(
            &format!("{chain} live controls accepted"),
            unknown.is_empty(),
            &detail,
        );
    }
}

fn print_unit_state(settings: &AppSettings) {
    println!();
    let mic_state = unit_active_state(MIC_UNIT);
    let aec_state = unit_active_state(AEC_UNIT);
    let out_state = unit_active_state(OUTPUT_UNIT);
    println!("biglinux-microphone-mic.service ...... {mic_state}");
    println!("biglinux-microphone-aec.service ...... {aec_state}");
    println!("biglinux-microphone-output.service ... {out_state}");

    if pipeline::mic_chain_wanted(settings) && mic_state != "active" {
        dump_journal(MIC_UNIT);
    }
    if settings.echo_cancel.enabled && aec_state != "active" {
        dump_journal(AEC_UNIT);
    }
    if settings.output_filter.enabled && out_state != "active" {
        dump_journal(OUTPUT_UNIT);
    }
}

/// Run a command purely for its exit status. Shared with the startup
/// health probe so the banner and `doctor` cannot disagree about whether
/// the same check passed.
pub(crate) fn command_succeeds(cmd: &str, args: &[&str]) -> bool {
    BigSubprocessSpec::builder()
        .program(cmd)
        .args(args.iter().copied())
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list([cmd])
        .build()
        .run()
        .is_ok_and(|o| o.status.success())
}

/// Whether systemd can find the unit file at all.
pub(crate) fn unit_known(name: &str) -> bool {
    command_succeeds("/usr/bin/systemctl", &["--user", "cat", name])
}

fn unit_active_state(name: &str) -> String {
    BigSubprocessSpec::builder()
        .program("/usr/bin/systemctl")
        .args(["--user", "is-active", name])
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/systemctl"])
        .build()
        .run()
        .map_or_else(
            |_| "unknown".to_owned(),
            |o| {
                let state = o.stdout_lossy().trim().to_owned();
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
    let out = BigSubprocessSpec::builder()
        .program("/usr/bin/journalctl")
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
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/journalctl"])
        .build()
        .run();
    match out {
        Ok(o) if o.status.success() => {
            print!("{}", o.stdout_lossy());
        }
        Ok(_) => println!("(journalctl returned non-zero — not enough permissions?)"),
        Err(e) => println!("(journalctl unavailable: {e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::{AecReference, aec_backend_in_conf, aec_reference_state};

    #[test]
    fn aec_backend_in_conf_reads_the_loaded_library() {
        let conf = "aec.args = {\n    library.name = aec/libspa-aec-gtcrn\n    foo = 1\n}\n";
        assert_eq!(
            aec_backend_in_conf(conf).as_deref(),
            Some("aec/libspa-aec-gtcrn")
        );
        // No declaration → None, so the doctor reports the conf as incomplete.
        assert_eq!(aec_backend_in_conf("aec.method = webrtc\n"), None);
    }

    const MONITOR_LINKED: &str = "\
alsa_output.pci-0000_00_1f.3.analog-stereo:monitor_FL
  |-> echo-cancel-sink:input_FL
alsa_output.pci-0000_00_1f.3.analog-stereo:playback_FL
  |<- Firefox:output_FL
";

    const PLAYING_WITHOUT_REFERENCE: &str = "\
alsa_output.pci-0000_00_1f.3.analog-stereo:playback_FL
  |<- Firefox:output_FL
echo-cancel-sink:input_FL
mic-biglinux:capture_FL
  |-> pw-record:input_FL
";

    /// The state observed on the maintainer's desktop: a browser playing,
    /// nothing recording, and the routing hook holding the reference open.
    const PLAYING_WITHOUT_A_RECORDER: &str = "\
alsa_output.pci-0000_00_1f.3.analog-stereo:playback_FL
  |<- Google Chrome:output_FL
echo-cancel-source:capture_MONO
  |-> mic-biglinux-capture:input_MONO
mic-biglinux-capture:input_MONO
  |<- echo-cancel-source:capture_MONO
";

    const IDLE_DESKTOP: &str = "\
alsa_input.usb-AKG:capture_AUX0
  |-> echo-cancel-capture:input_FL
echo-cancel-source:capture_MONO
  |-> mic-biglinux-capture:input_MONO
";

    #[test]
    fn the_reference_counts_as_linked_from_either_direction_of_the_dump() {
        assert_eq!(aec_reference_state(MONITOR_LINKED), AecReference::Linked);
        let reversed = "\
echo-cancel-sink:input_FL
  |<- alsa_output.pci-0000_00_1f.3.analog-stereo:monitor_FL
";
        assert_eq!(aec_reference_state(reversed), AecReference::Linked);
    }

    #[test]
    fn a_missing_reference_is_only_a_failure_while_something_plays() {
        assert_eq!(
            aec_reference_state(PLAYING_WITHOUT_REFERENCE),
            AecReference::Missing
        );
        assert_eq!(
            aec_reference_state(IDLE_DESKTOP),
            AecReference::NothingPlaying
        );
        assert_eq!(aec_reference_state(""), AecReference::NothingPlaying);
    }

    #[test]
    fn playback_alone_does_not_demand_a_reference_the_hook_refuses_to_link() {
        assert_eq!(
            aec_reference_state(PLAYING_WITHOUT_A_RECORDER),
            AecReference::NothingRecording
        );
    }
}
