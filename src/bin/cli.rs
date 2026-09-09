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
use biglinux_microphone::services::pipewire::{StreamDirection, current_streams};

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
    Autostart,
    Watch,
    Reload,
    LiveUpdate,
    ToggleMic,
    ToggleOutput,
    Set,
    Status,
    Doctor,
    Repair,
    Models,
    MeasureModel,
    PreviewQuantum,
}

/// One writable setting, named on the command line. Deliberately a short
/// allow-list: `settings` already dumps the whole file for anything a
/// caller only needs to read, and every name here is one a surface drives.
#[derive(Debug, Clone, Copy)]
enum Key {
    Mic,
    OutputMaster,
    MicIntensity,
    VoiceClarity,
    Echo,
    OutputVoices,
    Equalizer,
    VoiceChanger,
    VoicePitch,
    Quality,
    EqualizerPreset,
}

impl Key {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "mic" => Self::Mic,
            "output" => Self::OutputMaster,
            "mic-intensity" => Self::MicIntensity,
            "voice-clarity" => Self::VoiceClarity,
            "echo" => Self::Echo,
            "output-voices" => Self::OutputVoices,
            "equalizer" => Self::Equalizer,
            "voice-changer" => Self::VoiceChanger,
            "voice-pitch" => Self::VoicePitch,
            "quality" => Self::Quality,
            "eq-preset" => Self::EqualizerPreset,
            _ => return None,
        })
    }

    const NAMES: &'static str = "mic, output, mic-intensity, voice-clarity, echo, output-voices, \
                                 equalizer, voice-changer, voice-pitch, quality, eq-preset";
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
            "autostart" => Self::Autostart,
            "watch" => Self::Watch,
            "reload" => Self::Reload,
            "live-update" => Self::LiveUpdate,
            "toggle-mic" => Self::ToggleMic,
            "toggle-output" => Self::ToggleOutput,
            "set" => Self::Set,
            "status" => Self::Status,
            "doctor" => Self::Doctor,
            "models" => Self::Models,
            "measure-model" => Self::MeasureModel,
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
            Self::Autostart => autostart(),
            Self::Watch => watch_echo(),
            Self::Reload => reload_services(),
            Self::LiveUpdate => live_update(),
            Self::ToggleMic => toggle_mic(),
            Self::ToggleOutput => toggle_output(),
            Self::Set => set_one(args.next(), args.next()),
            Self::Status => print_status(),
            Self::Doctor => biglinux_microphone::diagnostics::doctor(),
            Self::Repair => repair(),
            Self::Models => print_models(),
            Self::PreviewQuantum => biglinux_microphone::services::preview::run_child(args.next()),
            Self::MeasureModel => {
                let Some(model) = args
                    .next()
                    .and_then(|arg| arg.parse::<u8>().ok())
                    .and_then(|id| biglinux_microphone::config::NoiseModel::try_from(id).ok())
                else {
                    return exit_with_error("measure-model requires a model id (0–8)");
                };
                match biglinux_microphone::config::plugin_cost::audio_thread_share(model) {
                    Some(share) => {
                        println!("{share}");
                        ExitCode::SUCCESS
                    }
                    None => exit_with_error("model measurement unavailable"),
                }
            }
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

    // Full strength: a benchmark measures what the model can do, and the
    // attenuation blend at anything less mixes the model's output with the
    // input, which is a different question.

    // Serialized rather than assembled by hand: the filter string carries
    // plugin paths and labels, and escaping those into JSON with a pair of
    // `replace` calls is a rule that has to be maintained against whatever
    // a future path contains.
    #[derive(serde::Serialize)]
    struct ModelRow {
        id: u8,
        plugin: &'static str,
        label: &'static str,
        sample_rate: u32,
        attenuation_only: bool,
        realtime: bool,
        loadable: bool,
        ffmpeg_filter: String,
    }

    let mut rows = Vec::new();
    for value in 0..=8_u8 {
        let Ok(model) = NoiseModel::try_from(value) else {
            continue;
        };
        let (plugin, label) = model.plugin_and_label();
        let loadable = model.plugin_loadable_cached();
        let ffmpeg_filter = if !loadable {
            String::new()
        } else if model.is_attenuation_only() {
            format!("ladspa=file={plugin}:plugin={label}:controls=c0=100.00")
        } else {
            format!(
                "ladspa=file={plugin}:plugin={label}:controls=c0=1|c1=1|c2={}|c3=0.5|c4=0|c5=0|c6=1|c7=-80",
                model.ladspa_control()
            )
        };
        rows.push(ModelRow {
            id: value,
            plugin,
            label,
            sample_rate: model.lavfi_sample_rate(),
            attenuation_only: model.is_attenuation_only(),
            realtime: model.is_realtime_lavfi_supported(),
            loadable,
            ffmpeg_filter,
        });
    }
    match serde_json::to_string_pretty(&rows) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => exit_with_error(&format!("models: {error}")),
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
    models          Print one JSON row per noise model: plugin, label,
                    sample rate and whether that plugin loads here
    measure-model N Measure the installed model in a separate process
    watch           Follow output changes for automatic echo cancellation
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
                    echo, output-voices, equalizer, voice-changer.
                    Percentage keys, 0 to 100: mic-intensity,
                    voice-clarity, voice-pitch. quality takes auto,
                    best, cheapest or manual. eq-preset takes one of the
                    names `settings` reports under equalizer.
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
        biglinux_microphone::services::pipewire::stop_mic_service()?;
        biglinux_microphone::services::pipewire::stop_aec_service()?;
        biglinux_microphone::services::pipewire::stop_output_service()?;
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
    reconcile_saved(false)
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
    match key {
        // Off cascades, exactly as `toggle-mic` does: leaving the chain
        // alive on `echo_cancel`/`stereo` defaults is not "off".
        Key::OutputMaster => match on_or_off(&value) {
            Some(enabled) => settings.output_filter.enabled = enabled,
            None => return not_a_switch(&name, &value),
        },
        Key::Mic => match on_or_off(&value) {
            Some(true) => settings.set_microphone_enabled(true),
            Some(false) => settings.set_microphone_enabled(false),
            None => return not_a_switch(&name, &value),
        },
        Key::MicIntensity => match fraction(&value) {
            Some(part) => settings.noise_reduction.strength = part,
            None => return not_a_percentage(&name, &value),
        },
        Key::VoiceClarity => match fraction(&value) {
            Some(part) => settings.noise_reduction.voice_recovery = part,
            None => return not_a_percentage(&name, &value),
        },
        // §78: three answers, not two. `auto` records the rule and leaves the chain where
        // it is — whoever can see which device the sound is going to is what decides, and
        // this program cannot.
        Key::Echo => match biglinux_microphone::config::EchoMode::parse(&value) {
            // Only the mode: `settle` below derives `enabled` from it, so
            // setting the flag here as well duplicated the rule that owns it.
            Some(mode) => {
                settings.echo_cancel.mode = mode;
                if mode != biglinux_microphone::config::EchoMode::Never {
                    settings.mic_bypass = false;
                }
            }
            None => {
                return exit_with_error(&format!(
                    "set {name}: `{value}` is not one of auto, on, off"
                ));
            }
        },
        // The two playback sub-filters. Each turns the master on with it,
        // because `output_nodes` gates every sub-effect behind that flag: a
        // sub-filter switched on under a master that is off is a control that
        // changes nothing. Neither turns the master off again — the other
        // sub-effects live behind it too, and `output.rs` keeps the graph
        // loaded on purpose so Chromium-based browsers do not pause playback
        // when their target sink disappears.
        Key::OutputVoices => match on_or_off(&value) {
            Some(on) => {
                settings.output_filter.noise_reduction.enabled = on;
                settings.output_filter.enabled |= on;
            }
            None => return not_a_switch(&name, &value),
        },
        Key::Equalizer => match on_or_off(&value) {
            Some(on) => {
                settings.output_filter.equalizer.enabled = on;
                settings.output_filter.enabled |= on;
            }
            None => return not_a_switch(&name, &value),
        },
        // Both fields, exactly as the application's own toggle writes them: `stereo` is
        // an older mic-widening setting, and the voice changer is that setting put in
        // `VoiceChanger` mode. `pitch_controls` builds nothing unless both agree, so
        // writing the flag alone would turn on a widener and call it a voice.
        Key::VoiceChanger => match on_or_off(&value) {
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
            None => return not_a_switch(&name, &value),
        },
        // `stereo.width` is the pitch coefficient the voice changer reads —
        // the field's name is older than the feature that uses it.
        Key::VoicePitch => match fraction(&value) {
            Some(part) => settings.stereo.width = part,
            None => return not_a_percentage(&name, &value),
        },
        // §77. The bands are the preset's, and the preset's name is kept beside them so
        // a screen can say which one is in force — the two are written together because
        // bands without a name read as "custom" and a name without its bands is a label
        // over somebody else's curve.
        Key::EqualizerPreset => {
            let Some(bands) = biglinux_microphone::config::eq_preset_bands(&value) else {
                return exit_with_error(&format!(
                    "set {name}: `{value}` is not one of {}",
                    biglinux_microphone::config::eq_preset_ids().join(", ")
                ));
            };
            settings.output_filter.equalizer.bands = bands.to_vec();
            settings.output_filter.equalizer.preset.clone_from(&value);
            // Choosing a curve is asking to hear it (§37's own screen puts the preset
            // under the switch), and a preset that changed nothing audible would read as
            // a control that does not work.
            settings.output_filter.equalizer.enabled = true;
            settings.output_filter.enabled = true;
        }
        // §38. Setting it decides the model straight away rather than at the next login:
        // a preference that only takes effect after a reboot is a preference somebody
        // tries once, hears no difference from, and never touches again.
        Key::Quality => match biglinux_microphone::config::Quality::parse(&value) {
            Some(wanted) => {
                settings.quality = wanted;
                settings.settle_quality();
            }
            None => {
                return exit_with_error(&format!(
                    "set {name}: `{value}` is not one of auto, best, cheapest, manual"
                ));
            }
        },
    }

    if let Err(error) = apply_settings(&mut settings, false) {
        return exit_with_error(&format!("set {name}: {error}"));
    }
    println!("set {name} = {value}");
    ExitCode::SUCCESS
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
    let observed = biglinux_microphone::services::reconcile::observe();
    let available = observed.is_ok();
    let observed = observed.unwrap_or_default();
    let result = serde_json::json!({
        "mic_enabled": pipeline::mic_chain_wanted(&settings),
        "output_enabled": settings.output_filter.enabled,
        "audio_available": available,
        "mic_running": observed.mic_present,
        "output_running": observed.output_present,
        "aec_running": observed.aec_present,
    });
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

fn watch_echo() -> ExitCode {
    let _settings_watch =
        match biglinux_microphone::services::settings_watch::SettingsWatch::start() {
            Ok(watch) => watch,
            Err(error) => return exit_with_error(&error.to_string()),
        };
    use biglinux_microphone::services::echo::RouteWatch;

    if autostart() != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }
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
        if settings.echo_cancel.mode == biglinux_microphone::config::EchoMode::Automatic
            && settings.echo_cancel.enabled != wanted
            && autostart() != ExitCode::SUCCESS
        {
            return ExitCode::FAILURE;
        }
    }
    let _ = child.wait();
    exit_with_error("PipeWire monitor disconnected")
}

fn apply_settings(settings: &mut AppSettings, force: bool) -> Result<(), String> {
    settings.settle_quality();
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
