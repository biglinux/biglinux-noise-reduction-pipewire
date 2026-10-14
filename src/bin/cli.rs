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
//! | `autostart`    | Reconcile graph with saved settings (login hook) |
//! | `reload`       | Restart mic + AEC + output pwloader units |
//! | `live-update`  | Push current settings into the running chain |
//! | `toggle-mic`   | Flip the master noise-reduction toggle and re-apply |
//! | `toggle-output`| Flip the output filter master and re-apply |
//! | `set`          | Set one named setting to a given value and re-apply |
//! | `status`       | Print one-line JSON: `{"mic_enabled":…,"output_enabled":…}` |

use std::process::ExitCode;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use biglinux_microphone::config::AppSettings;
use biglinux_microphone::pipeline;
use biglinux_microphone::services::pipewire::{
    AGC_UNIT, StreamDirection, current_streams, recover_capture, snapshot_sources,
};

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
    Calibrate,
    LevelRange,
    Latency,
    Profile,
    RecoverMic,
    Autostart,
    WatchRoute,
    Agc,
    Reload,
    LiveUpdate,
    ToggleMic,
    ToggleOutput,
    Set,
    Status,
    Doctor,
    Repair,
    Models,
    EqPresets,
    PreviewQuantum,
}

/// One writable setting, named on the command line. Deliberately a short
/// allow-list: `settings` already dumps the whole file for anything a
/// caller only needs to read, and every name here is one a surface drives.
#[derive(Debug, Clone, Copy)]
enum Key {
    Mic,
    Denoiser,
    SilenceGate,
    SilenceCurve,
    DfnPostFilter,
    DfnSnrGate,
    DfnStartupMute,
    OutputMaster,
    MicIntensity,
    VoiceGate,
    VoiceGateDepth,
    VoiceSensitivity,
    Echo,
    EchoAsyncDelay,
    EchoTrackDelay,
    EchoHbGuard,
    EchoReferenceBypass,
    OutputVoices,
    Equalizer,
    VoiceChanger,
    VoicePitch,
    Model,
    Hpf,
    Gate,
    GateIntensity,
    Compressor,
    CompressorIntensity,
    MicEqualizer,
    MicEqPreset,
    MicEqBands,
    EqualizerPreset,
}

impl Key {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "mic" => Self::Mic,
            "denoiser" => Self::Denoiser,
            "silence-gate" => Self::SilenceGate,
            "silence-curve" => Self::SilenceCurve,
            "dfn-post-filter" => Self::DfnPostFilter,
            "dfn-snr-gate" => Self::DfnSnrGate,
            "dfn-startup-mute" => Self::DfnStartupMute,
            "output" => Self::OutputMaster,
            "mic-intensity" => Self::MicIntensity,
            "voice-gate" => Self::VoiceGate,
            "voice-gate-depth" => Self::VoiceGateDepth,
            "voice-sensitivity" => Self::VoiceSensitivity,
            "echo" => Self::Echo,
            "echo-async-delay" => Self::EchoAsyncDelay,
            "echo-track-delay" => Self::EchoTrackDelay,
            "echo-hb-guard" => Self::EchoHbGuard,
            "echo-reference-bypass" => Self::EchoReferenceBypass,
            "output-voices" => Self::OutputVoices,
            "equalizer" => Self::Equalizer,
            "voice-changer" => Self::VoiceChanger,
            "voice-pitch" => Self::VoicePitch,
            "model" => Self::Model,
            "hpf" => Self::Hpf,
            "gate" => Self::Gate,
            "gate-intensity" => Self::GateIntensity,
            "compressor" => Self::Compressor,
            "compressor-intensity" => Self::CompressorIntensity,
            "mic-equalizer" => Self::MicEqualizer,
            "mic-eq-preset" => Self::MicEqPreset,
            "mic-eq-bands" => Self::MicEqBands,
            "eq-preset" => Self::EqualizerPreset,
            _ => return None,
        })
    }

    const NAMES: &'static str = "mic, denoiser, silence-gate, silence-curve, dfn-post-filter, dfn-snr-gate, dfn-startup-mute, output, mic-intensity, voice-gate, voice-gate-depth, voice-sensitivity, echo, echo-async-delay, echo-track-delay, echo-hb-guard, echo-reference-bypass, output-voices, \
                                 equalizer, voice-changer, voice-pitch, model, hpf, gate, \
                                 gate-intensity, compressor, compressor-intensity, mic-equalizer, \
                                 mic-eq-preset, mic-eq-bands, eq-preset";
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
            "calibrate" => Self::Calibrate,
            "level-range" => Self::LevelRange,
            "latency" => Self::Latency,
            "profile" => Self::Profile,
            "recover-mic" => Self::RecoverMic,
            "autostart" => Self::Autostart,
            "watch-route" => Self::WatchRoute,
            "agc" => Self::Agc,
            "reload" => Self::Reload,
            "live-update" => Self::LiveUpdate,
            "toggle-mic" => Self::ToggleMic,
            "toggle-output" => Self::ToggleOutput,
            "set" => Self::Set,
            "status" => Self::Status,
            "doctor" => Self::Doctor,
            "models" => Self::Models,
            "eq-presets" => Self::EqPresets,
            "preview-quantum" => Self::PreviewQuantum,
            "repair" => Self::Repair,
            _ => return None,
        })
    }

    fn run(self, args: &mut impl Iterator<Item = String>) -> ExitCode {
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
            Self::Calibrate => calibrate(args.next()),
            Self::LevelRange => level_range(args.next(), args.next()),
            Self::Latency => print_latency(args.next()),
            Self::Profile => profile(args.next(), args.next()),
            Self::RecoverMic => recover_mic(),
            Self::Autostart => autostart(),
            Self::WatchRoute => watch_route(),
            Self::Agc => biglinux_microphone::agc::run(),
            Self::Reload => reload_services(),
            Self::LiveUpdate => live_update(),
            Self::ToggleMic => toggle_mic(),
            Self::ToggleOutput => toggle_output(),
            Self::Set => set_one(args.next(), args.next()),
            Self::Status => print_status(),
            Self::Doctor => biglinux_microphone::diagnostics::doctor(),
            Self::Repair => repair(),
            Self::Models => print_models(),
            Self::EqPresets => print_eq_presets(),
            Self::PreviewQuantum => biglinux_microphone::services::preview::run_child(args.next()),
        }
    }
}

/// Print every noise model with the shared object and label that drive it,
/// as JSON.
///
/// Exists so the calibration harness can measure the plugins we actually
/// ship instead of the ONNX files they were built from, without keeping a
/// second copy of the paths that would disagree with this one after any
/// rename.
fn print_models() -> ExitCode {
    use biglinux_microphone::config::NoiseModel;

    // Serialized rather than assembled by hand: the filter string carries
    // plugin paths and labels, and escaping those into JSON by hand is a rule
    // that has to be maintained against whatever a future path contains.
    #[derive(serde::Serialize)]
    struct ModelRow {
        id: u8,
        plugin: &'static str,
        label: &'static str,
        sample_rate: u32,
        loadable: bool,
        ffmpeg_filter: String,
    }

    let rows: Vec<ModelRow> = NoiseModel::ALL
        .into_iter()
        .map(|model| {
            let (plugin, label) = model.plugin_and_label();
            let loadable = model.plugin_available();
            // Full strength: a benchmark measures what the model can do, and
            // anything less mixes the model's output with the input.
            let ffmpeg_filter = if loadable {
                format!("ladspa=file={plugin}:plugin={label}:controls=c0=100.00")
            } else {
                String::new()
            };
            ModelRow {
                id: model.into(),
                plugin,
                label,
                sample_rate: 48_000,
                loadable,
                ffmpeg_filter,
            }
        })
        .collect();
    match serde_json::to_string_pretty(&rows) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => exit_with_error(&format!("models: {error}")),
    }
}

/// Print the equalizer preset ids as a JSON array, so a UI can populate its
/// preset picker from the same source of truth `set eq-preset` validates against.
fn print_eq_presets() -> ExitCode {
    match serde_json::to_string(&biglinux_microphone::config::eq_preset_ids()) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => exit_with_error(&format!("eq-presets: {error}")),
    }
}

fn main() -> ExitCode {
    env_logger::init_from_env(env_logger::Env::new().filter("BIGLINUX_MICROPHONE_LOG"));

    let mut args = std::env::args().skip(1);
    let raw = args.next().unwrap_or_else(|| "settings".to_owned());

    if let Some(cmd) = Cmd::parse(&raw) {
        cmd.run(&mut args)
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
    calibrate [quick|position]
                    Level the default microphone while somebody speaks, for a
                    few seconds, and save where it landed. quick (the default)
                    starts a round and sets the everyday volume; position
                    measures another place the same person speaks from and
                    widens the window the level daemon works inside
    level-range <low%> <high%>|off
                    Draw by hand the range the level daemon may move the capture
                    volume inside, for the default microphone. Two measurements
                    draw it automatically; this is the fine adjustment, and
                    `off` gives up following the voice at all
    latency [model] What the microphone path costs, stage by stage, with the
                    provenance of every figure and a list of what no cheap
                    measurement can see
    profile list|save <name>|apply <name>|delete <name>
                    Scenarios: a whole set of preferences under a name. A
                    scenario remembers the microphone it wants but never a
                    calibration, which is a measurement rather than a choice
    recover-mic     Activate a capture profile and a default microphone when
                    the session has none; prints the device it restored
    models          Print one JSON row per noise model: plugin, label,
                    sample rate and whether that plugin is installed
    eq-presets      Print the equalizer preset ids as a JSON array
    watch-route     Follow output changes for automatic echo cancellation
    autostart       Reconcile the PipeWire graph with the saved settings
                    (runs at login via the systemd user unit)
    reload          Explicitly restart mic + AEC + output pwloader units
                    (needed after a graph-topology change)
    live-update     Push current settings into the running filter chain
                    without restarting any service
    toggle-mic      Flip the master mic noise-reduction switch and apply
    toggle-output   Flip the output filter master switch and apply
    set <key> <value>
                    Set one named setting and apply. On/off keys: mic,
                    denoiser, silence-gate, echo, output-voices, equalizer,
                    voice-changer.
                    Percentage keys, 0 to 100: mic-intensity,
                    voice-clarity, voice-pitch, gate-intensity,
                    compressor-intensity. model takes dfn3, dfn3-ll or
                    dpdfnet. voice-gate is on/off for the chosen model and
                    voice-gate-depth takes 1 to 60 dB, voice-sensitivity
                    1 (keeps quiet voices) to 5 (mutes more noise).
                    On/off keys: hpf, gate, compressor, mic-equalizer.
                    mic-eq-preset (mic) and eq-preset (output) take one
                    of the names from the eq-presets command.
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
    let s = match AppSettings::load_strict() {
        Ok(settings) => settings,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    match serde_json::to_string_pretty(&s) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => exit_with_error(&format!("serialise settings: {e}")),
    }
}

fn dump_mic_conf() -> ExitCode {
    // The effective snapshot, so the dump is the graph `apply` would write.
    let s = AppSettings::load().runtime_settings();
    print!("{}", pipeline::build_mic_conf_for(&s));
    ExitCode::SUCCESS
}

fn dump_output_conf() -> ExitCode {
    let s = AppSettings::load().runtime_settings();
    print!("{}", pipeline::build_output_conf_for(&s));
    ExitCode::SUCCESS
}

fn apply_configs() -> ExitCode {
    reconcile_saved(false)
}

fn remove_configs() -> ExitCode {
    let _guard = match biglinux_microphone::config::storage::SettingsLock::acquire() {
        Ok(guard) => guard,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    let result = (|| -> std::io::Result<()> {
        use biglinux_microphone::services::pipewire as pw;
        pw::stop_mic_service()?;
        pw::stop_aec_service()?;
        pw::stop_output_service()?;
        pw::set_unit_running(pw::AGC_UNIT, false)?;
        pw::set_unit_running(pw::ROUTE_UNIT, false)?;
        pipeline::remove_all()
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => exit_with_error(&error.to_string()),
    }
}

fn exit_with_error(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::FAILURE
}

/// Reconcile the live PipeWire graph with whatever `settings.json`
/// currently asks for. Called by the systemd user unit on login and
/// available manually for `biglinux-microphone-cli autostart`.
fn autostart() -> ExitCode {
    pipeline::purge_legacy_files();
    // Before reconciling a chain, make sure there is something to put in it: a
    // saved card profile the device no longer offers leaves a machine with no
    // capture source at all, and that survives every reboot until somebody
    // notices the picker is empty.
    recover_capture();
    reconcile_saved(false)
}

/// Scenarios: whole sets of preferences under a name.
///
/// `apply` is the only verb here that touches audio, and it does it once: one
/// settings write, one reconcile. Anything that looked like a loop of `set`
/// calls would be a dozen writes and a dozen chances of an audible gap.
fn profile(verb: Option<String>, name: Option<String>) -> ExitCode {
    use biglinux_microphone::services::profiles;

    let verb = verb.unwrap_or_else(|| "list".into());
    match (verb.as_str(), name) {
        ("list", _) => {
            let listed = profiles::list(&AppSettings::load());
            match serde_json::to_string(&listed) {
                Ok(json) => {
                    println!("{json}");
                    ExitCode::SUCCESS
                }
                Err(error) => exit_with_error(&error.to_string()),
            }
        }
        ("save", Some(label)) => match profiles::save(&label) {
            Ok(name) => {
                println!("{name}");
                ExitCode::SUCCESS
            }
            Err(error) => exit_with_error(&error),
        },
        ("apply", Some(name)) => match profiles::apply(&name) {
            Ok(applied) => {
                println!("{}", serde_json::to_string(&applied).unwrap_or_default());
                ExitCode::SUCCESS
            }
            Err(error) => exit_with_error(&error),
        },
        ("delete", Some(name)) => match profiles::delete(&name) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => exit_with_error(&error),
        },
        ("save" | "apply" | "delete", None) => {
            exit_with_error(&format!("profile {verb}: needs a name"))
        }
        _ => exit_with_error("profile: use list, save, apply or delete"),
    }
}

/// What the microphone path costs, stage by stage, as JSON.
///
/// With a model name, the figure that model *would* contribute — so a picker
/// can label each option with a real number without switching to it first.
fn print_latency(model: Option<String>) -> ExitCode {
    let model = match model
        .as_deref()
        .map(biglinux_microphone::config::NoiseModel::parse)
    {
        Some(Some(model)) => Some(model),
        Some(None) => return exit_with_error("latency: unknown model"),
        None => None,
    };
    let report = biglinux_microphone::services::latency::report(&AppSettings::load(), model);
    match serde_json::to_string(&report) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => exit_with_error(&error.to_string()),
    }
}

/// How long a pass may run before it gives up on hearing anybody. Long enough to
/// say a sentence, short enough that nobody wonders whether it is stuck.
const CALIBRATION_SECONDS: f64 = 8.0;

/// Level this microphone while the person speaks, and write down where it landed.
///
/// Every pass is the same measurement; only what is done with the number
/// differs. `quick` is the first one: it sets the everyday volume and forgets
/// any window an earlier round left behind. `position` is the same person
/// speaking from somewhere else, and all it does is widen the window to hold
/// that place too — which is all the level daemon needs, since it only ever
/// asks where this microphone is quietest and where it is loudest.
fn calibrate(pass: Option<String>) -> ExitCode {
    let pass = pass.unwrap_or_else(|| "quick".into());
    if !matches!(pass.as_str(), "quick" | "position") {
        return exit_with_error(&format!(
            "calibrate: unknown pass `{pass}`. Use quick or position"
        ));
    }
    let completed = match biglinux_microphone::agc::calibrate_held(CALIBRATION_SECONDS) {
        Ok(outcome) => outcome,
        Err(error) => return exit_with_error(&error),
    };
    let outcome = &completed.outcome;
    let report = serde_json::to_string(outcome).unwrap_or_default();
    if !outcome.heard_speech {
        println!("{report}");
        // Saving this would record the room, not the person. The pass has already
        // restored an owned change when it was safe; see volume_restored.
        return exit_with_error("no speech was heard; nothing was saved");
    }
    let _lock = match biglinux_microphone::config::storage::SettingsLock::acquire() {
        Ok(lock) => lock,
        Err(error) => {
            return exit_with_error(&format!(
                "calibration settings were not saved: {error}; microphone volume may already have changed"
            ));
        }
    };
    let mut settings = match AppSettings::load_strict() {
        Ok(settings) => settings,
        Err(error) => {
            return exit_with_error(&format!(
                "calibration settings were not saved: {error}; microphone volume may already have changed"
            ));
        }
    };
    let device = settings
        .mic_calibration
        .devices
        .entry(outcome.device.clone())
        .or_default();
    if pass == "position" {
        device.add_place(outcome.volume, outcome.voice);
    } else {
        device.start_round(outcome.volume, outcome.voice);
    }
    // Save the number, and touch nothing else. A calibration changes no graph
    // topology, so reconciling the whole graph here tears down and re-routes
    // playback that has nothing to do with the microphone — measured on this
    // machine, every pass stopped the route watcher and restarted output units
    // underneath whatever was playing.
    if let Err(error) = settings.save() {
        return exit_with_error(&format!(
            "calibration settings were not saved: {error}; microphone volume may already have changed"
        ));
    }
    // Success becomes visible only after persistence completed.
    println!("{report}");
    drop(completed); // the daemon may resume only after the new range was saved
    // The one thing that does depend on this file: the level daemon exists only
    // while some device has a window for it to work inside.
    let wanted = settings
        .mic_calibration
        .devices
        .values()
        .any(|d| d.window().is_some());
    if let Err(error) = biglinux_microphone::services::pipewire::set_unit_running(AGC_UNIT, wanted)
    {
        eprintln!(
            "warning: the level daemon could not be {}: {error}",
            if wanted { "started" } else { "stopped" }
        );
    }
    ExitCode::SUCCESS
}

/// Draw the level daemon's window by hand, for the default microphone.
///
/// Saves and touches nothing else, for the same reason the calibration pass
/// does not: a window is a number in a file, no graph changes with it, and
/// reconciling the whole graph here tears down playback that has nothing to do
/// with the microphone. The one thing that does follow is the daemon itself,
/// which exists only while some device has a window for it to work inside.
fn level_range(low: Option<String>, high: Option<String>) -> ExitCode {
    let Some(low) = low else {
        return exit_with_error("level-range: give a low and a high percentage, or `off`");
    };
    let (sources, default_id) = snapshot_sources();
    let Some(device) = sources
        .into_iter()
        .find(|s| Some(s.node_id) == default_id && !s.node_name.is_empty())
        .map(|s| s.node_name)
    else {
        return exit_with_error("level-range: no default microphone to draw a range for");
    };
    let _lock = match biglinux_microphone::config::storage::SettingsLock::acquire() {
        Ok(lock) => lock,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    let mut settings = match AppSettings::load_strict() {
        Ok(settings) => settings,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    let entry = settings.mic_calibration.devices.entry(device).or_default();
    if low == "off" {
        entry.clear_window();
    } else {
        let (Ok(low), Some(Ok(high))) = (low.parse::<f32>(), high.map(|h| h.parse::<f32>())) else {
            return exit_with_error("level-range: two percentages, for example `150 210`");
        };
        if let Err(error) = entry.set_window_percent(low, high) {
            return exit_with_error(&format!("level-range: {error}"));
        }
    }
    if let Err(error) = settings.save() {
        return exit_with_error(&error.to_string());
    }
    let wanted = settings
        .mic_calibration
        .devices
        .values()
        .any(|d| d.window().is_some());
    if let Err(error) = biglinux_microphone::services::pipewire::set_unit_running(AGC_UNIT, wanted)
    {
        eprintln!(
            "warning: the level daemon could not be {}: {error}",
            if wanted { "started" } else { "stopped" }
        );
    }
    ExitCode::SUCCESS
}

/// Bring a capture device back and print the one that answered, for the GUI's
/// "no microphone found" path. Silent success when a microphone was already
/// there; failure is a machine with no usable capture hardware at all.
fn recover_mic() -> ExitCode {
    match recover_capture() {
        Some(description) => {
            println!("{description}");
            ExitCode::SUCCESS
        }
        None => ExitCode::SUCCESS,
    }
}

/// Force-reload every pwloader unit from scratch. Useful after a
/// topology change the live path can't handle (e.g. a new LADSPA
/// plugin, or manual config editing). Does **not** touch WirePlumber.
fn reload_services() -> ExitCode {
    reconcile_saved(true)
}

/// Push current settings into the already-loaded filter-chain without
/// restarting any service. Safe to call repeatedly.
fn live_update() -> ExitCode {
    let settings = AppSettings::load();
    match biglinux_microphone::services::pipewire::apply_live(&settings) {
        Ok(outcome) if outcome.fully_applied(&settings) => ExitCode::SUCCESS,
        Ok(_) => exit_with_error("The filter is not ready. Run reload or repair."),
        Err(error) => exit_with_error(&error.to_string()),
    }
}

/// Flip the mic master. Off cascades through every mic-side flag so
/// the Plasma applet (which has no fine-grained controls) actually
/// stops the mic loader instead of leaving it alive on
/// `echo_cancel`/`stereo` defaults. On only re-enables `noise_reduction`
/// — the user can re-enable individual sub-filters from the GUI.
fn toggle_mic() -> ExitCode {
    let _guard = match biglinux_microphone::config::storage::SettingsLock::acquire() {
        Ok(guard) => guard,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    let mut settings = match AppSettings::load_strict() {
        Ok(settings) => settings,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    let enabled = !pipeline::mic_chain_wanted(&settings);
    settings.set_microphone_enabled(enabled);
    match apply_settings(&mut settings, false) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => exit_with_error(&error),
    }
}

/// Flip `output_filter.enabled` and reconcile the standalone output
/// unit. Used by the Plasma applet.
fn toggle_output() -> ExitCode {
    let _guard = match biglinux_microphone::config::storage::SettingsLock::acquire() {
        Ok(guard) => guard,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    let mut settings = match AppSettings::load_strict() {
        Ok(settings) => settings,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    settings.output_filter.enabled = !settings.output_filter.enabled;
    match apply_settings(&mut settings, false) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => exit_with_error(&error),
    }
}

/// Set one named setting and bring the graph in line with it.
///
/// Switches take `on`/`off` rather than flipping, because a caller that
/// draws the current state can be looking at a stale read: the value it
/// sends is the value the person asked for, not a guess about the other
/// one. Every unit is reconciled afterwards — each reconciler decides
/// from the saved settings, so calling all three is right for any key.
/// What one group of `set` keys did with the value it was handed.
enum Set {
    /// The key does not belong to this group; ask the next one.
    Other,
    Applied,
    Rejected(ExitCode),
}

fn set_one(key: Option<String>, value: Option<String>) -> ExitCode {
    let _settings_lock = match biglinux_microphone::config::storage::SettingsLock::acquire() {
        Ok(lock) => lock,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    let (Some(name), Some(value)) = (key, value) else {
        return exit_with_error(&format!(
            "set: needs a key and a value. Keys: {}",
            Key::NAMES
        ));
    };
    let Some(key) = Key::parse(&name) else {
        return exit_with_error(&format!("set: unknown key `{name}`. Keys: {}", Key::NAMES));
    };

    let mut settings = match AppSettings::load_strict() {
        Ok(settings) => settings,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    // Every key belongs to exactly one group. They are asked in order, and the
    // first one that recognises the key owns it.
    const GROUPS: [fn(Key, &str, &str, &mut AppSettings) -> Set; 5] = [
        set_denoiser,
        set_denoiser_tuning,
        set_echo_and_output,
        set_mic_effects,
        set_mic_equalizer,
    ];
    let mut applied = false;
    for group in GROUPS {
        match group(key, &name, &value, &mut settings) {
            Set::Other => {}
            Set::Rejected(code) => return code,
            Set::Applied => {
                applied = true;
                break;
            }
        }
    }
    if !applied {
        return exit_with_error(&format!(
            "set {name}: no writer for this key; this is a bug"
        ));
    }

    if let Err(error) = apply_settings(&mut settings, false) {
        return exit_with_error(&format!("set {name}: {error}"));
    }
    println!("set {name} = {value}");
    ExitCode::SUCCESS
}

fn set_denoiser(key: Key, name: &str, value: &str, settings: &mut AppSettings) -> Set {
    match key {
        Key::Mic => match on_or_off(value) {
            Some(on) => settings.set_microphone_enabled(on),
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        // The denoiser alone, so the equalizer (or any other effect) can run
        // without it. Enabling it lifts a master bypass; disabling it never
        // sets one — the chain stays up for whatever else is on.
        Key::Denoiser => match on_or_off(value) {
            Some(on) => {
                settings.noise_reduction.enabled = on;
                if on {
                    settings.mic_bypass = false;
                }
            }
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        // The plugin's silence expander depth, as a 0..=100 slider onto 0..=40 dB
        // (0 = off). The low-latency model leaves a louder residual, so this is
        // variable rather than a fixed switch.
        Key::SilenceGate => match fraction(value) {
            Some(part) => {
                let max = f32::from(biglinux_microphone::config::SILENCE_DEPTH_MAX);
                settings.noise_reduction.silence_depth = (part * max).round() as u8;
            }
            None => return Set::Rejected(not_a_percentage(name, value)),
        },
        Key::MicIntensity => match fraction(value) {
            Some(part) => settings.noise_reduction.strength = part,
            None => return Set::Rejected(not_a_percentage(name, value)),
        },
        Key::VoiceGate => match on_or_off(value) {
            // Each model keeps its own choice: the gate is off on DPDFNet until
            // asked for.
            Some(on) if settings.noise_reduction.model.is_dfn3() => {
                settings.noise_reduction.voice_gate = on;
            }
            Some(on) => settings.noise_reduction.dpdfnet_voice_gate = on,
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        Key::VoiceGateDepth => match value.parse::<u8>() {
            Ok(depth)
                if (1..=biglinux_microphone::config::VOICE_GATE_DEPTH_MAX).contains(&depth) =>
            {
                settings.noise_reduction.voice_gate_depth_db = depth;
            }
            _ => {
                return Set::Rejected(exit_with_error(&format!(
                    "set {name}: 1..60 dB (60 mutes); voice-gate off turns it off"
                )));
            }
        },
        Key::VoiceSensitivity => match value.parse::<u8>() {
            Ok(level)
                if (1..=biglinux_microphone::config::VOICE_SENSITIVITY_MAX).contains(&level) =>
            {
                settings.noise_reduction.voice_sensitivity = level;
            }
            _ => {
                return Set::Rejected(exit_with_error(&format!(
                    "set {name}: 1 (sensitive) to 5 (strict), 3 is normal"
                )));
            }
        },
        // Both chains follow the pick so the mic and the output filter run the
        // same network.
        Key::Model => match biglinux_microphone::config::NoiseModel::parse(value) {
            Some(model) => {
                settings.noise_reduction.model = model;
                settings.output_filter.noise_reduction.model = model;
            }
            None => {
                return Set::Rejected(exit_with_error(&format!(
                    "set {name}: `{value}` is not one of dfn3, dfn3-ll, dpdfnet"
                )));
            }
        },
        _ => return Set::Other,
    }
    Set::Applied
}

fn set_denoiser_tuning(key: Key, name: &str, value: &str, settings: &mut AppSettings) -> Set {
    match key {
        Key::DfnPostFilter => match value.parse::<f32>() {
            Ok(v) if v.is_finite() && (0.0..=1.0).contains(&v) => {
                settings.noise_reduction.post_filter_beta = v;
            }
            _ => {
                return Set::Rejected(exit_with_error(
                    "dfn-post-filter: finite beta in 0..1 (0 = off; test 0.02)",
                ));
            }
        },
        Key::DfnSnrGate => {
            if value == "auto" {
                settings.noise_reduction.min_snr_override_db = None;
            } else {
                match value.parse::<f32>() {
                    Ok(v) if v.is_finite() && (-40.0..=0.0).contains(&v) => {
                        settings.noise_reduction.min_snr_override_db = Some(v);
                    }
                    _ => {
                        return Set::Rejected(exit_with_error(
                            "dfn-snr-gate: -40..0 dB or auto; AEC voice cap still applies",
                        ));
                    }
                }
            }
        }
        Key::DfnStartupMute => match value.parse::<u16>() {
            Ok(v) if v <= 1000 => settings.noise_reduction.startup_mute_ms = v,
            _ => {
                return Set::Rejected(exit_with_error(
                    "dfn-startup-mute: 0..1000 ms; activation-only, requires reload",
                ));
            }
        },
        Key::SilenceCurve => match parse_silence_curve(name, value) {
            Ok(curve) => settings.noise_reduction.silence_curve_db = curve,
            Err(code) => return Set::Rejected(code),
        },
        _ => return Set::Other,
    }
    Set::Applied
}

/// The per-frequency silence floor, as comma-separated dB anchors. Empty,
/// `off` or `none` clears it.
fn parse_silence_curve(name: &str, value: &str) -> Result<Option<[f32; 10]>, ExitCode> {
    use biglinux_microphone::config::{SILENCE_ANCHOR_SENTINEL, SILENCE_ANCHORS};
    const _: () = assert!(SILENCE_ANCHORS == 10, "return type tracks SILENCE_ANCHORS");
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "off" || trimmed == "none" {
        return Ok(None);
    }
    let parsed: Result<Vec<f32>, _> = trimmed
        .split(',')
        .map(|p| p.trim().parse::<f32>())
        .collect();
    let Ok(vals) = parsed else {
        return Err(exit_with_error(&format!(
            "set {name}: `{value}` is not numbers"
        )));
    };
    if vals.len() != SILENCE_ANCHORS {
        return Err(exit_with_error(&format!(
            "set {name}: expected {SILENCE_ANCHORS} comma-separated dB values (or empty to clear)"
        )));
    }
    if vals.iter().any(|v| !v.is_finite()) {
        return Err(exit_with_error(&format!(
            "set {name}: values must be finite"
        )));
    }
    let mut curve = [SILENCE_ANCHOR_SENTINEL as f32; SILENCE_ANCHORS];
    for (dst, v) in curve.iter_mut().zip(vals) {
        *dst = v.clamp(SILENCE_ANCHOR_SENTINEL as f32, 0.0);
    }
    Ok(Some(curve))
}

fn set_echo_and_output(key: Key, name: &str, value: &str, settings: &mut AppSettings) -> Set {
    match key {
        // §78: three answers, not two. `auto` records the rule and leaves the chain where
        // it is — whoever can see which device the sound is going to is what decides, and
        // this program cannot.
        Key::Echo => match biglinux_microphone::config::EchoMode::parse(value) {
            // Only the mode: `settle` below derives `enabled` from it, so
            // setting the flag here as well duplicated the rule that owns it.
            Some(mode) => {
                settings.echo_cancel.mode = mode;
                if mode != biglinux_microphone::config::EchoMode::Never {
                    settings.mic_bypass = false;
                }
            }
            None => {
                return Set::Rejected(exit_with_error(&format!(
                    "set {name}: `{value}` is not one of auto, on, off"
                )));
            }
        },
        Key::EchoAsyncDelay | Key::EchoTrackDelay | Key::EchoHbGuard | Key::EchoReferenceBypass => {
            let Some(on) = on_or_off(value) else {
                return Set::Rejected(not_a_switch(name, value));
            };
            match key {
                Key::EchoAsyncDelay => settings.echo_cancel.gtcrn_async_delay = on,
                Key::EchoTrackDelay => settings.echo_cancel.gtcrn_track_delay = on,
                Key::EchoHbGuard => settings.echo_cancel.gtcrn_hb_guard = on,
                _ => settings.echo_cancel.gtcrn_reference_bypass = on,
            }
        }
        // Off cascades, exactly as `toggle-mic` does: leaving the chain
        // alive on `echo_cancel`/`stereo` defaults is not "off".
        Key::OutputMaster => match on_or_off(value) {
            Some(enabled) => settings.output_filter.enabled = enabled,
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        // The two playback sub-filters. Each turns the master on with it,
        // because `output_nodes` gates every sub-effect behind that flag: a
        // sub-filter switched on under a master that is off is a control that
        // changes nothing. Neither turns the master off again — the other
        // sub-effects live behind it too, and `output.rs` keeps the graph
        // loaded on purpose so Chromium-based browsers do not pause playback
        // when their target sink disappears.
        Key::OutputVoices => match on_or_off(value) {
            Some(on) => {
                settings.output_filter.noise_reduction.enabled = on;
                settings.output_filter.enabled |= on;
            }
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        Key::Equalizer => match on_or_off(value) {
            Some(on) => {
                settings.output_filter.equalizer.enabled = on;
                settings.output_filter.enabled |= on;
            }
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        // §77. The bands are the preset's, and the preset's name is kept beside them so
        // a screen can say which one is in force — the two are written together because
        // bands without a name read as "custom" and a name without its bands is a label
        // over somebody else's curve.
        Key::EqualizerPreset => {
            let Some(bands) = biglinux_microphone::config::eq_preset_bands(value) else {
                return Set::Rejected(exit_with_error(&format!(
                    "set {name}: `{value}` is not one of {}",
                    biglinux_microphone::config::eq_preset_ids().join(", ")
                )));
            };
            settings.output_filter.equalizer.bands = bands.to_vec();
            value.clone_into(&mut settings.output_filter.equalizer.preset);
            // Choosing a curve is asking to hear it (§37's own screen puts the preset
            // under the switch), and a preset that changed nothing audible would read as
            // a control that does not work.
            settings.output_filter.equalizer.enabled = true;
            settings.output_filter.enabled = true;
        }
        _ => return Set::Other,
    }
    Set::Applied
}

/// Advanced mic-chain sub-effects. Each is a direct field on the mic settings;
/// the graph is reconciled by the caller like every other key.
fn set_mic_effects(key: Key, name: &str, value: &str, settings: &mut AppSettings) -> Set {
    match key {
        Key::Hpf => match on_or_off(value) {
            Some(on) => settings.hpf.enabled = on,
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        Key::Gate => match on_or_off(value) {
            Some(on) => settings.gate.enabled = on,
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        // A 0..=100 slider onto the gate's own 0..=GATE_INTENSITY_MAX scale.
        Key::GateIntensity => match fraction(value) {
            Some(part) => {
                let max = f32::from(biglinux_microphone::config::GATE_INTENSITY_MAX);
                settings.gate.intensity = (part * max).round() as u8;
            }
            None => return Set::Rejected(not_a_percentage(name, value)),
        },
        Key::Compressor => match on_or_off(value) {
            Some(on) => settings.compressor.enabled = on,
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        Key::CompressorIntensity => match fraction(value) {
            Some(part) => settings.compressor.intensity = part,
            None => return Set::Rejected(not_a_percentage(name, value)),
        },
        _ => return Set::Other,
    }
    Set::Applied
}

fn set_mic_equalizer(key: Key, name: &str, value: &str, settings: &mut AppSettings) -> Set {
    match key {
        // Both fields, exactly as the application's own toggle writes them: `stereo` is
        // an older mic-widening setting, and the voice changer is that setting put in
        // `VoiceChanger` mode. `pitch_controls` builds nothing unless both agree, so
        // writing the flag alone would turn on a widener and call it a voice.
        Key::VoiceChanger => match on_or_off(value) {
            Some(on) => {
                settings.stereo.enabled = on;
                if on {
                    settings.mic_bypass = false;
                }
                settings.stereo.mode = if on {
                    biglinux_microphone::config::StereoMode::VoiceChanger
                } else {
                    biglinux_microphone::config::StereoMode::Mono
                };
            }
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        // `stereo.width` is the pitch coefficient the voice changer reads —
        // the field's name is older than the feature that uses it.
        Key::VoicePitch => match fraction(value) {
            Some(part) => settings.stereo.width = part,
            None => return Set::Rejected(not_a_percentage(name, value)),
        },
        // The microphone's own equalizer (settings.equalizer), distinct from the
        // output/playback one. This key is the *only* one that switches it on:
        // writing a curve or a preset used to enable it as a side effect, which
        // is how an equalizer nobody asked for ended up shaping every call.
        Key::MicEqualizer => match on_or_off(value) {
            Some(on) => {
                settings.equalizer.enabled = on;
                if on {
                    settings.mic_bypass = false;
                }
            }
            None => return Set::Rejected(not_a_switch(name, value)),
        },
        Key::MicEqPreset => {
            let Some(bands) = biglinux_microphone::config::eq_preset_bands(value) else {
                return Set::Rejected(exit_with_error(&format!(
                    "set {name}: `{value}` is not one of {}",
                    biglinux_microphone::config::eq_preset_ids().join(", ")
                )));
            };
            settings.equalizer.bands = bands.to_vec();
            value.clone_into(&mut settings.equalizer.preset);
        }
        // Ten comma-separated band gains from the graphic EQ's vertical sliders.
        // A hand-drawn curve is a custom preset by definition.
        Key::MicEqBands => match parse_eq_bands(name, value) {
            Ok(bands) => {
                settings.equalizer.bands = bands;
                "custom".clone_into(&mut settings.equalizer.preset);
            }
            Err(code) => return Set::Rejected(code),
        },
        _ => return Set::Other,
    }
    Set::Applied
}

fn parse_eq_bands(name: &str, value: &str) -> Result<Vec<f32>, ExitCode> {
    use biglinux_microphone::config::{EQ_BAND_COUNT, EQ_BAND_MAX, EQ_BAND_MIN};
    let parsed: Result<Vec<f32>, _> = value
        .split(',')
        .map(|part| part.trim().parse::<f32>())
        .collect();
    let Ok(bands) = parsed else {
        return Err(exit_with_error(&format!(
            "set {name}: `{value}` is not numbers"
        )));
    };
    if bands.len() != EQ_BAND_COUNT {
        return Err(exit_with_error(&format!(
            "set {name}: expected {EQ_BAND_COUNT} comma-separated gains"
        )));
    }
    if bands.iter().any(|band| !band.is_finite()) {
        return Err(exit_with_error(&format!(
            "set {name}: gains must be finite"
        )));
    }
    Ok(bands
        .iter()
        .map(|b| b.clamp(EQ_BAND_MIN, EQ_BAND_MAX))
        .collect())
}

fn on_or_off(value: &str) -> Option<bool> {
    match value {
        "on" | "true" | "1" => Some(true),
        "off" | "false" | "0" => Some(false),
        _ => None,
    }
}

/// A percentage on the command line, as the 0.0..=1.0 the settings store.
///
/// Percent on the outside because that is what the surfaces show and what a
/// person types; a fraction on the inside because that is what every one of
/// these fields already holds.
fn fraction(value: &str) -> Option<f32> {
    let percent = value.parse::<f32>().ok()?;
    (0.0..=100.0).contains(&percent).then_some(percent / 100.0)
}

fn not_a_switch(name: &str, value: &str) -> ExitCode {
    exit_with_error(&format!("set {name}: `{value}` is not on or off"))
}

fn not_a_percentage(name: &str, value: &str) -> ExitCode {
    exit_with_error(&format!(
        "set {name}: `{value}` is not a percentage from 0 to 100"
    ))
}

/// One-line JSON for the Plasma applet's status poll. Reports the same
/// fields the GTK Simple-mode and the plasmoid switches mutate
/// (`noise_reduction.enabled` / `output_filter.enabled`) so the two UIs
/// stay in lockstep. `mic_chain_wanted` would be wider — it stays true
/// while any other mic filter (HPF, gate, EQ, …) is on — and would
/// leave the plasmoid switch stuck after the user disables NR alone.
fn print_status() -> ExitCode {
    let settings = match AppSettings::load_strict() {
        Ok(settings) => settings,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    let result = biglinux_microphone::services::reconcile::status(&settings);
    println!("{result}");
    ExitCode::SUCCESS
}

/// Query the current PipeWire nodes, then print and exit.
fn list_audio_apps() -> ExitCode {
    let mut collected = match current_streams() {
        Ok(streams) => streams,
        Err(error) => return exit_with_error(&format!("list-apps: {error}")),
    };

    collected.sort_by(|a, b| {
        a.application_name
            .cmp(&b.application_name)
            .then(a.node_id.cmp(&b.node_id))
    });
    print_streams(&collected);
    ExitCode::SUCCESS
}

/// Repair stale user-level filter-chain files and unit state.
///
/// Package changes can leave an on-disk `.conf` referencing builtins or
/// controls unavailable to the installed binary. The daemon then crash-loops
/// until systemd pins the unit in `failed` state. Bringing the chain back means
/// three steps that the GUI toggle does not perform on its own:
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
    reconcile_saved(true)
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

/// Follow output route changes so echo cancellation can react to them.
///
/// Applying saved settings is not part of this job: `biglinux-microphone.path`
/// wakes the reconcile unit when the settings file changes, so nothing has to
/// stay alive watching a file. What does need a live process is the comparison
/// below — a route change only exists relative to the route seen before it.
fn watch_route() -> ExitCode {
    use biglinux_microphone::services::echo::RouteWatch;

    let mut child = match BigSubprocessSpec::builder()
        .program("/usr/bin/pw-dump")
        .arg("--monitor")
        .stderr(BigSubprocessOutputMode::Inherit)
        .allow_list(["/usr/bin/pw-dump"])
        .stdout(BigSubprocessOutputMode::Capture)
        .build()
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return exit_with_error(&format!("watch: {error}")),
    };
    let stdout = child.take_stdout().expect("piped monitor stdout");

    // The monitor stream *is* the graph, so the answer comes out of it rather
    // than out of another `pw-dump`. Asking cost the session dearly: one dump
    // connects a client, the monitor reports that three times, and each report
    // spawned another dump — the watcher fed itself and the fork count grew
    // without a single user action.
    let mut route = RouteWatch::default();
    for event in serde_json::Deserializer::from_reader(std::io::BufReader::new(stdout))
        .into_iter::<Vec<serde_json::Value>>()
    {
        let batch = match event {
            Ok(batch) => batch,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return exit_with_error(&format!("watch: {error}"));
            }
        };
        // Only a route change is worth reading settings for; anything else and
        // this loop would `dlopen` the inference runtimes once per event.
        let Some(wanted) = route.absorb(batch) else {
            continue;
        };
        let settings = AppSettings::load();
        use biglinux_microphone::services::echo::{RouteAction, route_action};
        match route_action(
            settings.echo_cancel.mode,
            settings.echo_cancel.enabled,
            wanted,
        ) {
            RouteAction::Toggle => {
                if autostart() != ExitCode::SUCCESS {
                    return ExitCode::FAILURE;
                }
            }
            // The route moved but AEC stays on: re-prime its coarse delay for
            // the new acoustic path. A failed restart must not kill the watcher
            // — the next route change (or a manual repair) retries (AEC-02).
            RouteAction::RepriimeAec => {
                if let Err(error) = biglinux_microphone::services::reconcile::reprime_aec() {
                    eprintln!("watch: could not re-prime AEC after route change: {error}");
                }
            }
            RouteAction::Nothing => {}
        }
    }
    let _ = child.wait();
    exit_with_error("PipeWire monitor disconnected")
}

fn apply_settings(settings: &mut AppSettings, force: bool) -> Result<(), String> {
    biglinux_microphone::services::echo::settle(&mut settings.echo_cancel);
    settings.save().map_err(|error| error.to_string())?;
    biglinux_microphone::services::reconcile::apply(settings, force)
        .map_err(|error| error.to_string())
}

fn reconcile_saved(force: bool) -> ExitCode {
    let _guard = match biglinux_microphone::config::storage::SettingsLock::acquire() {
        Ok(guard) => guard,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    let mut settings = match AppSettings::load_strict() {
        Ok(settings) => settings,
        Err(error) => return exit_with_error(&error.to_string()),
    };
    match apply_settings(&mut settings, force) {
        Ok(()) => {
            println!("audio settings applied and verified");
            ExitCode::SUCCESS
        }
        Err(error) => exit_with_error(&error),
    }
}
