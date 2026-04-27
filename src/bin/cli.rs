//! CLI entry point (`biglinux-microphone-cli`).
//!
//! Subcommands:
//!
//! | Command | Action |
//! |---------|--------|
//! | *(none)* / `settings` | Dump current settings as JSON |
//! | `mic-conf`     | Print the mic filter-chain config that would be written |
//! | `output-conf`  | Print the output filter-chain config |
//! | `apply`        | Write every config file under the user's XDG directories |
//! | `remove`       | Delete every file previously written by `apply` |
//! | `list-apps`    | Scan the PipeWire graph for routable audio streams |
//! | `spectrum`     | Stream a low-resolution spectrum ASCII meter |
//! | `autostart`    | Reconcile graph with saved settings (login hook) |
//! | `reload`       | Restart filter-chain.service + output unit |
//! | `live-update`  | Push current settings into the running chain |
//! | `toggle-mic`   | Flip the master noise-reduction toggle and re-apply |
//! | `toggle-output`| Flip the output filter master and re-apply |
//! | `status`       | Print one-line JSON: `{"mic_enabled":…,"output_enabled":…}` |

use std::io;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use biglinux_microphone::config::AppSettings;
use biglinux_microphone::pipeline;
use biglinux_microphone::services::audio_monitor::{
    AudioMonitor, Event as MonitorEvent, MonitorConfig,
};
use biglinux_microphone::services::pipewire::{Event, PwService, StreamDirection};

/// Subcommand parsed from `argv[1]`. Keeping the dispatch in an enum
/// (rather than a 16-arm string match) lets `clippy::match_same_arms`
/// stay strict and gives a single source of truth for `print_help`.
#[derive(Debug, Clone, Copy)]
enum Cmd {
    Help,
    Settings,
    MicConf,
    OutputConf,
    Apply,
    Remove,
    ListApps,
    Spectrum,
    Autostart,
    Reload,
    LiveUpdate,
    ToggleMic,
    ToggleOutput,
    Status,
    Doctor,
    Repair,
}

impl Cmd {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "--help" | "-h" | "help" => Self::Help,
            "settings" => Self::Settings,
            "mic-conf" => Self::MicConf,
            "output-conf" => Self::OutputConf,
            "apply" => Self::Apply,
            "remove" => Self::Remove,
            "list-apps" => Self::ListApps,
            "spectrum" => Self::Spectrum,
            "autostart" => Self::Autostart,
            "reload" => Self::Reload,
            "live-update" => Self::LiveUpdate,
            "toggle-mic" => Self::ToggleMic,
            "toggle-output" => Self::ToggleOutput,
            "status" => Self::Status,
            "doctor" => Self::Doctor,
            "repair" => Self::Repair,
            _ => return None,
        })
    }

    fn run(self) -> ExitCode {
        match self {
            Self::Help => {
                print_help();
                ExitCode::SUCCESS
            }
            Self::Settings => dump_settings(),
            Self::MicConf => dump_mic_conf(),
            Self::OutputConf => dump_output_conf(),
            Self::Apply => apply_configs(),
            Self::Remove => remove_configs(),
            Self::ListApps => list_audio_apps(),
            Self::Spectrum => print_spectrum(),
            Self::Autostart => autostart(),
            Self::Reload => reload_services(),
            Self::LiveUpdate => live_update(),
            Self::ToggleMic => toggle_mic(),
            Self::ToggleOutput => toggle_output(),
            Self::Status => print_status(),
            Self::Doctor => biglinux_microphone::diagnostics::doctor(),
            Self::Repair => repair(),
        }
    }
}

fn main() -> ExitCode {
    pretty_env_logger::init_custom_env("BIGLINUX_MICROPHONE_LOG");

    let mut args = std::env::args().skip(1);
    let raw = args.next().unwrap_or_else(|| "settings".to_owned());

    if let Some(cmd) = Cmd::parse(&raw) {
        cmd.run()
    } else {
        eprintln!("unknown command: {raw}\n");
        print_help();
        ExitCode::FAILURE
    }
}

fn print_help() {
    println!(
        "biglinux-microphone-cli {version}

USAGE:
    biglinux-microphone-cli <command>

COMMANDS:
    settings        Dump persisted settings as JSON (default)
    mic-conf        Print the generated mic filter-chain config
    output-conf     Print the generated output filter-chain config
    apply           Write every config file under the user's XDG dirs
    remove          Delete every config file previously written by apply
    list-apps       Scan the PipeWire graph for routable audio streams
    spectrum        Stream a low-resolution spectrum ASCII meter (Ctrl-C to stop)
    autostart       Reconcile the PipeWire graph with the saved settings
                    (runs at login via the systemd user unit)
    reload          Explicitly restart filter-chain.service + output unit
                    (needed after a graph-topology change)
    live-update     Push current settings into the running filter chain
                    without restarting any service
    toggle-mic      Flip the master mic noise-reduction switch and apply
    toggle-output   Flip the output filter master switch and apply
    status          Print one-line JSON with current enable flags
    doctor          Run end-to-end diagnostics (use this when the GUI
                    toggle does nothing on a freshly-installed system)
    repair          Regenerate every config file and force-restart the
                    user units (clears `failed` state from previous
                    versions). Run this after upgrading the package.
    help            Show this message",
        version = env!("CARGO_PKG_VERSION"),
    );
}

fn dump_settings() -> ExitCode {
    let s = AppSettings::load();
    match serde_json::to_string_pretty(&s) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => exit_with_error(&format!("serialise settings: {e}")),
    }
}

fn dump_mic_conf() -> ExitCode {
    let s = AppSettings::load();
    print!("{}", pipeline::build_mic_conf_for(&s));
    ExitCode::SUCCESS
}

fn dump_output_conf() -> ExitCode {
    let s = AppSettings::load();
    print!("{}", pipeline::build_output_conf_for(&s));
    ExitCode::SUCCESS
}

fn apply_configs() -> ExitCode {
    let s = AppSettings::load();
    if let Err(e) = pipeline::apply(&s) {
        return exit_with_error(&format!("apply: {e}"));
    }
    println!("applied {}", pipeline::mic_conf_path().display());
    let out_path = pipeline::output_conf_path();
    if s.output_filter.enabled {
        println!("applied {}", out_path.display());
    } else {
        println!(
            "output filter disabled — {} not written",
            out_path.display()
        );
    }

    reconcile_mic_chain(&s);
    reconcile_output_service(&s);
    ExitCode::SUCCESS
}

fn remove_configs() -> ExitCode {
    match pipeline::remove_all() {
        Ok(()) => {
            println!("removed generated configs");
            ExitCode::SUCCESS
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => ExitCode::SUCCESS,
        Err(e) => exit_with_error(&format!("remove: {e}")),
    }
}

fn exit_with_error(msg: &str) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::FAILURE
}

/// Reconcile the live PipeWire graph with whatever `settings.json`
/// currently asks for. Called by the systemd user unit on login and
/// available manually for `biglinux-microphone-cli autostart`.
fn autostart() -> ExitCode {
    // Best-effort migration: scrub any config file the Python
    // configurator (or an older Rust revision) might have left behind
    // before regenerating the active layout. Idempotent.
    pipeline::purge_legacy_files();

    let settings = AppSettings::load();

    if let Err(e) = pipeline::apply(&settings) {
        return exit_with_error(&format!("autostart apply: {e}"));
    }

    reconcile_mic_chain(&settings);
    reconcile_output_service(&settings);

    println!("autostart: configuration reconciled");
    ExitCode::SUCCESS
}

/// Force-reload both filter-chains from scratch. Useful after a
/// topology change the live path can't handle (e.g. a new LADSPA
/// plugin, or manual config editing). Does **not** touch WirePlumber.
fn reload_services() -> ExitCode {
    let settings = AppSettings::load();
    if let Err(e) = pipeline::apply(&settings) {
        return exit_with_error(&format!("reload apply: {e}"));
    }
    // AEC and mic both live inside `filter-chain.service` as
    // drop-ins, so a single restart covers both. The `05-` prefix on
    // the AEC drop-in ensures `echo-cancel-source` is loaded before
    // the mic filter chain resolves `target.object`.
    reconcile_mic_chain(&settings);
    // Output unit only runs when its master is on — restart it from
    // scratch when wanted (covers fresh start + topology pickup),
    // otherwise stop it so no idle worker remains.
    if settings.output_filter.enabled {
        if let Err(e) = biglinux_microphone::services::pipewire::restart_output_service() {
            eprintln!("warning: output service restart failed: {e}");
        }
    } else if let Err(e) = biglinux_microphone::services::pipewire::stop_output_service() {
        eprintln!("warning: output service stop failed: {e}");
    }
    println!(
        "reload: filter-chain (mic + AEC drop-ins) + output unit reconciled \
         (wireplumber untouched)"
    );
    ExitCode::SUCCESS
}

/// Reconcile the system-level `filter-chain.service` with the
/// user-visible master switches. The unit hosts both the mic chain
/// drop-in and the AEC drop-in, so it must be running whenever either
/// is wanted; stop it only when both are off so no `mic-biglinux` /
/// `echo-cancel-source` virtual nodes remain hanging in the graph.
fn reconcile_mic_chain(settings: &AppSettings) {
    use biglinux_microphone::services::pipewire::{reload_mic_chain, stop_filter_chain_service};
    if pipeline::mic_chain_wanted(settings) || pipeline::echo_cancel_wanted(settings) {
        if let Err(e) = reload_mic_chain() {
            eprintln!("warning: filter-chain reload failed: {e}");
        }
    } else if let Err(e) = stop_filter_chain_service() {
        eprintln!("warning: filter-chain.service stop failed: {e}");
    }
}

/// Bring the standalone output unit up when the user wants the chain
/// running, and tear it down when they turn the master off so no idle
/// `pipewire -c` worker remains. The conf carries `filter.smart = true`
/// plus the pinned `filter.smart.target` (captured by the GUI on first
/// enable), so when the unit is up WirePlumber transparently inserts us
/// between every stream and the user's hardware sink. Stopping the unit
/// removes the virtual sink — Chromium-based browsers pause playback
/// when their target sink disappears, which is the accepted price for
/// not keeping a dormant worker running.
fn reconcile_output_service(settings: &AppSettings) {
    use biglinux_microphone::services::pipewire::{start_output_service, stop_output_service};

    if settings.output_filter.enabled {
        if let Err(e) = start_output_service() {
            eprintln!("warning: output service start failed: {e}");
        }
    } else if let Err(e) = stop_output_service() {
        eprintln!("warning: output service stop failed: {e}");
    }
}

/// Push current settings into the already-loaded filter-chain without
/// restarting any service. Safe to call repeatedly.
fn live_update() -> ExitCode {
    let settings = AppSettings::load();
    match biglinux_microphone::services::pipewire::apply_live(&settings) {
        Ok(outcome) => {
            if outcome.fully_applied(&settings) {
                println!("live-update: ok");
            } else {
                println!(
                    "live-update: partial — mic_pushed={} output_pushed={} \
                     (run `reload` if a filter-chain node is missing)",
                    outcome.mic_pushed, outcome.output_pushed,
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => exit_with_error(&format!("live-update: {e}")),
    }
}

/// Flip the mic master. Off cascades through every mic-side flag so
/// the Plasma applet (which has no fine-grained controls) actually
/// stops `filter-chain.service` instead of leaving it alive on
/// `echo_cancel`/`stereo` defaults. On only re-enables `noise_reduction`
/// — the user can re-enable individual sub-filters from the GUI.
fn toggle_mic() -> ExitCode {
    let mut settings = AppSettings::load();
    let new_state = !settings.noise_reduction.enabled;
    if new_state {
        settings.noise_reduction.enabled = true;
    } else {
        pipeline::cascade_mic_off(&mut settings);
    }

    if let Err(e) = settings.save() {
        return exit_with_error(&format!("toggle-mic save: {e}"));
    }
    if let Err(e) = pipeline::apply(&settings) {
        return exit_with_error(&format!("toggle-mic apply: {e}"));
    }
    if let Err(e) = biglinux_microphone::services::pipewire::apply_live(&settings) {
        eprintln!("warning: live update failed: {e}");
    }
    reconcile_mic_chain(&settings);

    println!(
        "toggle-mic: noise_reduction.enabled = {} (mic_chain_wanted = {})",
        new_state,
        pipeline::mic_chain_wanted(&settings),
    );
    ExitCode::SUCCESS
}

/// Flip `output_filter.enabled` and reconcile the standalone output
/// unit. Used by the Plasma applet.
fn toggle_output() -> ExitCode {
    let mut settings = AppSettings::load();
    let new_state = !settings.output_filter.enabled;
    settings.output_filter.enabled = new_state;

    if let Err(e) = settings.save() {
        return exit_with_error(&format!("toggle-output save: {e}"));
    }
    if let Err(e) = pipeline::apply(&settings) {
        return exit_with_error(&format!("toggle-output apply: {e}"));
    }
    reconcile_output_service(&settings);
    if let Err(e) = biglinux_microphone::services::pipewire::apply_live(&settings) {
        eprintln!("warning: live update failed: {e}");
    }

    println!("toggle-output: output_filter.enabled = {new_state}");
    ExitCode::SUCCESS
}

/// One-line JSON for the Plasma applet's status poll. Reports the same
/// fields the GTK Simple-mode and the plasmoid switches mutate
/// (`noise_reduction.enabled` / `output_filter.enabled`) so the two UIs
/// stay in lockstep. `mic_chain_wanted` would be wider — it stays true
/// while any other mic filter (HPF, gate, EQ, …) is on — and would
/// leave the plasmoid switch stuck after the user disables NR alone.
fn print_status() -> ExitCode {
    let s = AppSettings::load();
    let mic = s.noise_reduction.enabled;
    let output = s.output_filter.enabled;
    println!("{{\"mic_enabled\":{mic},\"output_enabled\":{output}}}");
    ExitCode::SUCCESS
}

/// Start the PipeWire service, collect the initial graph snapshot (the
/// daemon reports every existing global right after we bind the
/// registry), then print and exit.
fn list_audio_apps() -> ExitCode {
    const QUIET_WINDOW: Duration = Duration::from_millis(300);
    const HARD_DEADLINE: Duration = Duration::from_secs(3);
    const POLL_INTERVAL: Duration = Duration::from_millis(25);

    let service = PwService::start();
    let events = service.events();

    let started = Instant::now();
    let mut last_event_at = Instant::now();
    let mut collected: Vec<_> = Vec::new();

    loop {
        match events.try_recv() {
            Ok(Event::StreamAppeared(s)) => {
                last_event_at = Instant::now();
                collected.push(s);
            }
            Ok(Event::StreamDisappeared { .. }) => {
                last_event_at = Instant::now();
            }
            Ok(Event::Fatal(e)) => {
                service.shutdown();
                return exit_with_error(&e);
            }
            Err(async_channel::TryRecvError::Closed) => break,
            Err(async_channel::TryRecvError::Empty) => {
                if last_event_at.elapsed() >= QUIET_WINDOW && !collected.is_empty() {
                    break;
                }
                if started.elapsed() >= HARD_DEADLINE {
                    break;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    }

    service.shutdown();

    collected.sort_by(|a, b| {
        a.application_name
            .cmp(&b.application_name)
            .then(a.node_id.cmp(&b.node_id))
    });
    print_streams(&collected);
    ExitCode::SUCCESS
}

/// Stream the spectrum analyser to stdout as an ASCII meter.
fn print_spectrum() -> ExitCode {
    let monitor = AudioMonitor::start(MonitorConfig::default());
    let events = monitor.events();

    const PRINT_EVERY: u64 = 6;

    loop {
        match events.recv_blocking() {
            Ok(MonitorEvent::Frame(frame)) => {
                if frame.seq % PRINT_EVERY != 0 {
                    continue;
                }
                let bars = render_bars(&frame.bands_db);
                println!("rms={:>6.1} dB  {}", frame.rms_db, bars);
            }
            Ok(MonitorEvent::Fatal(e)) => {
                monitor.shutdown();
                return exit_with_error(&e);
            }
            Err(_) => break,
        }
    }
    monitor.shutdown();
    ExitCode::SUCCESS
}

fn render_bars(bands_db: &[f32]) -> String {
    const GLYPHS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    bands_db
        .iter()
        .map(|&db| {
            let norm = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            let idx = (norm * (GLYPHS.len() as f32 - 1.0)).round() as usize;
            GLYPHS[idx]
        })
        .collect()
}

/// Reset the user-level filter-chain units after an upgrade.
///
/// After bumping the package, the on-disk `.conf` files generated by a
/// previous version may reference builtins or controls that the new
/// binary no longer emits — the daemon then crash-loops until systemd
/// gives up and pins the unit in `failed` state. Bringing the chain
/// back means three steps that the GUI toggle does not perform on its
/// own:
///
/// 1. Regenerate every `.conf` from the current binary
///    ([`pipeline::apply`]).
/// 2. Clear the `failed` flag with `systemctl --user reset-failed` so
///    the next start is not refused with "Start request repeated too
///    quickly".
/// 3. Re-issue `restart` for both units.
///
/// Exits non-zero if the *final* state still doesn't have both nodes
/// visible in the PipeWire graph — that's the same signal the GUI
/// toggle would have to recover from.
fn repair() -> ExitCode {
    use std::process::{Command, Stdio};

    pipeline::purge_legacy_files();

    let settings = AppSettings::load();
    if let Err(e) = pipeline::apply(&settings) {
        return exit_with_error(&format!("repair apply: {e}"));
    }
    println!("regenerated {}", pipeline::mic_conf_path().display());
    println!("regenerated {}", pipeline::output_conf_path().display());

    let reset = |unit: &str| {
        let _ = Command::new("systemctl")
            .args(["--user", "reset-failed", unit])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    };
    reset("filter-chain.service");
    reset("biglinux-microphone-output.service");

    reconcile_mic_chain(&settings);
    reconcile_output_service(&settings);

    println!("repair: configs rewritten, units reset and restarted");
    println!("run `biglinux-microphone-cli doctor` to verify");
    ExitCode::SUCCESS
}

fn print_streams(streams: &[biglinux_microphone::services::pipewire::AppStream]) {
    if streams.is_empty() {
        println!("no audio streams found");
        return;
    }
    println!(
        "{:>6}  {:<12}  {:<30}  TITLE",
        "NODE", "DIRECTION", "APPLICATION",
    );
    for s in streams {
        let dir = match s.direction {
            StreamDirection::Playback => "playback",
            StreamDirection::Capture => "capture",
        };
        let title = s.media_name.as_deref().unwrap_or("-");
        let app = if s.application_name.is_empty() {
            "(unnamed)"
        } else {
            s.application_name.as_str()
        };
        println!("{:>6}  {dir:<12}  {app:<30}  {title}", s.node_id);
    }
}
