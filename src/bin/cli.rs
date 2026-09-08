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

use std::io;
use std::process::ExitCode;

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
}

/// One writable setting, named on the command line. Deliberately a short
/// allow-list: `settings` already dumps the whole file for anything a
/// caller only needs to read, and every name here is one a surface drives.
#[derive(Debug, Clone, Copy)]
enum Key {
    Mic,
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

    const NAMES: &'static str = "mic, mic-intensity, voice-clarity, echo, output-voices, \
                                 equalizer, voice-changer, voice-pitch, quality, eq-preset";

    /// Whether this key takes a percentage rather than on/off.
    fn is_a_quantity(self) -> bool {
        matches!(
            self,
            Self::MicIntensity | Self::VoiceClarity | Self::VoicePitch
        )
    }

    /// Whether honouring this key means a filter chain has to come or go.
    /// Everything else is a port of a chain that is already loaded, which
    /// `apply_live` moves without an interruption.
    fn changes_the_graph(self) -> bool {
        !self.is_a_quantity()
    }
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

    let mut rows = Vec::new();
    for value in 0..=8_u8 {
        let Ok(model) = NoiseModel::try_from(value) else {
            continue;
        };
        let (plugin, label) = model.plugin_and_label();
        let filter = if !model.plugin_loadable() {
            String::new()
        } else if model.is_attenuation_only() {
            format!("ladspa=file={plugin}:plugin={label}:controls=c0=100.00")
        } else {
            format!(
                "ladspa=file={plugin}:plugin={label}:controls=c0=1|c1=1|c2={}|c3=0.5|c4=0|c5=0|c6=1|c7=-80",
                model.ladspa_control()
            )
        };
        rows.push(format!(
            "    {{\"id\": {value}, \"plugin\": \"{plugin}\", \"label\": \"{label}\", \
             \"sample_rate\": {}, \"attenuation_only\": {}, \"realtime\": {}, \
             \"loadable\": {}, \"ffmpeg_filter\": \"{}\"}}",
            model.lavfi_sample_rate(),
            model.is_attenuation_only(),
            model.is_realtime_lavfi_supported(),
            model.plugin_loadable(),
            filter.replace('\\', "\\\\").replace('"', "\\\""),
        ));
    }
    println!("[\n{}\n]", rows.join(",\n"));
    ExitCode::SUCCESS
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
        // `pipeline::apply` always writes the args file (bypass-mode
        // graph); only the service is kept stopped while disabled.
        println!(
            "output filter disabled — {} written in bypass mode, service stopped",
            out_path.display()
        );
    }

    reconcile_aec_service(&s);
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

fn exit_with_error(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::FAILURE
}

/// Reconcile the live PipeWire graph with whatever `settings.json`
/// currently asks for. Called by the systemd user unit on login and
/// available manually for `biglinux-microphone-cli autostart`.
fn autostart() -> ExitCode {
    pipeline::purge_legacy_files();
    let mut settings = AppSettings::load();

    // §38 is decided here and not on every read: login is when the machine's answer can
    // actually have changed — a laptop that was on mains yesterday is on battery now, and
    // a model chosen for the wrong one of those is the difference between a filter that
    // fits and one that misses blocks. Saved only when it moved, so an unchanged machine
    // does not rewrite its settings file at every login.
    let before = (settings.noise_reduction.model, settings.echo_cancel.enabled);
    biglinux_microphone::services::echo::settle(&mut settings.echo_cancel);
    let machine = biglinux_microphone::config::Machine::read(settings.filters_running());
    settings.settle_quality(&machine);
    if (settings.noise_reduction.model, settings.echo_cancel.enabled) != before
        && let Err(e) = settings.save()
    {
        eprintln!("warning: autostart: saving the chosen model: {e}");
    }

    if let Err(e) = pipeline::apply(&settings) {
        return exit_with_error(&format!("autostart apply: {e}"));
    }

    reconcile_aec_service(&settings);
    reconcile_mic_chain(&settings);
    reconcile_output_service(&settings);

    println!("autostart: configuration reconciled");
    ExitCode::SUCCESS
}

/// Force-reload every pwloader unit from scratch. Useful after a
/// topology change the live path can't handle (e.g. a new LADSPA
/// plugin, or manual config editing). Does **not** touch WirePlumber.
fn reload_services() -> ExitCode {
    let settings = AppSettings::load();
    if let Err(e) = pipeline::apply(&settings) {
        return exit_with_error(&format!("reload apply: {e}"));
    }
    // AEC first so `echo-cancel-source` exists by the time the mic
    // loader resolves `target.object`.
    reconcile_aec_service(&settings);
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
        "reload: mic + AEC + output pwloader units reconciled \
         (wireplumber untouched)"
    );
    ExitCode::SUCCESS
}

/// Reconcile the mic loader unit with the master mic-side switches.
/// The unit must be running whenever any mic filter is wanted; stop
/// it otherwise so `mic-biglinux` doesn't hang around as a dead node.
fn reconcile_mic_chain(settings: &AppSettings) {
    use biglinux_microphone::services::pipewire::{restart_mic_service, stop_mic_service};
    if pipeline::mic_chain_wanted(settings) {
        if let Err(e) = restart_mic_service() {
            eprintln!("warning: mic loader reload failed: {e}");
        }
    } else if let Err(e) = stop_mic_service() {
        eprintln!("warning: mic loader stop failed: {e}");
    }
}

/// Reconcile the AEC loader unit. Independent lifecycle from the mic
/// loader: when AEC is wanted the EC source must exist before the mic
/// chain resolves its capture target, so callers run this *before*
/// reconciling the mic unit.
fn reconcile_aec_service(settings: &AppSettings) {
    use biglinux_microphone::services::pipewire::{restart_aec_service, stop_aec_service};
    if settings.echo_cancel.enabled {
        if let Err(e) = restart_aec_service() {
            eprintln!("warning: AEC loader reload failed: {e}");
        }
    } else if let Err(e) = stop_aec_service() {
        eprintln!("warning: AEC loader stop failed: {e}");
    }
}

/// Bring the standalone output unit up when the user wants the chain
/// running, and tear it down when they turn the master off so no idle
/// `pipewire -c` worker remains. The conf carries `filter.smart = true`,
/// so WirePlumber transparently inserts us before the current default sink.
/// Stopping the unit
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
/// stops the mic loader instead of leaving it alive on
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
    reconcile_aec_service(&settings);
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

/// Set one named setting and bring the graph in line with it.
///
/// Switches take `on`/`off` rather than flipping, because a caller that
/// draws the current state can be looking at a stale read: the value it
/// sends is the value the person asked for, not a guess about the other
/// one. Every unit is reconciled afterwards — each reconciler decides
/// from the saved settings, so calling all three is right for any key.
fn set_one(key: Option<String>, value: Option<String>) -> ExitCode {
    let (Some(name), Some(value)) = (key, value) else {
        return exit_with_error(&format!(
            "set: needs a key and a value. Keys: {}",
            Key::NAMES
        ));
    };
    let Some(key) = Key::parse(&name) else {
        return exit_with_error(&format!("set: unknown key `{name}`. Keys: {}", Key::NAMES));
    };

    let mut settings = AppSettings::load();
    match key {
        // Off cascades, exactly as `toggle-mic` does: leaving the chain
        // alive on `echo_cancel`/`stereo` defaults is not "off".
        Key::Mic => match on_or_off(&value) {
            Some(true) => settings.noise_reduction.enabled = true,
            Some(false) => pipeline::cascade_mic_off(&mut settings),
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
            Some(mode) => {
                settings.echo_cancel.mode = mode;
                match mode {
                    biglinux_microphone::config::EchoMode::Always => {
                        settings.echo_cancel.enabled = true;
                    }
                    biglinux_microphone::config::EchoMode::Never => {
                        settings.echo_cancel.enabled = false;
                    }
                    biglinux_microphone::config::EchoMode::Automatic => {}
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
                let machine =
                    biglinux_microphone::config::Machine::read(settings.filters_running());
                settings.settle_quality(&machine);
            }
            None => {
                return exit_with_error(&format!(
                    "set {name}: `{value}` is not one of auto, best, cheapest, manual"
                ));
            }
        },
    }

    biglinux_microphone::services::echo::settle(&mut settings.echo_cancel);
    if let Err(e) = settings.save() {
        return exit_with_error(&format!("set {name}: save: {e}"));
    }
    if let Err(e) = pipeline::apply(&settings) {
        return exit_with_error(&format!("set {name}: apply: {e}"));
    }
    // A unit restart is what makes a chain appear or disappear, and it costs a
    // gap in the audio. A control that only moves a port of a chain already
    // loaded must not pay for one: a slider dragged across its range would
    // restart the graph at every stop.
    if key.changes_the_graph() {
        reconcile_aec_service(&settings);
        reconcile_mic_chain(&settings);
        reconcile_output_service(&settings);
    }
    if let Err(e) = biglinux_microphone::services::pipewire::apply_live(&settings) {
        eprintln!("warning: live update failed: {e}");
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
    let s = AppSettings::load();
    let mic = s.noise_reduction.enabled;
    let output = s.output_filter.enabled;
    println!("{{\"mic_enabled\":{mic},\"output_enabled\":{output}}}");
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
    use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

    let settings = AppSettings::load();
    if let Err(e) = pipeline::apply(&settings) {
        return exit_with_error(&format!("repair apply: {e}"));
    }
    println!("regenerated {}", pipeline::mic_conf_path().display());
    println!("regenerated {}", pipeline::output_conf_path().display());

    let reset = |unit: &str| {
        let _ = BigSubprocessSpec::builder()
            .program("/usr/bin/systemctl")
            .args(["--user", "reset-failed", unit])
            .stdout(BigSubprocessOutputMode::Null)
            .stderr(BigSubprocessOutputMode::Null)
            .allow_list(["/usr/bin/systemctl"])
            .build()
            .run();
    };
    reset("biglinux-microphone-mic.service");
    reset("biglinux-microphone-aec.service");
    reset("biglinux-microphone-output.service");

    reconcile_aec_service(&settings);
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

fn watch_echo() -> ExitCode {
    let _ = autostart();
    let mut child = match std::process::Command::new("/usr/bin/pw-dump")
        .arg("--monitor")
        .stdout(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return exit_with_error(&format!("watch: {error}")),
    };
    let stdout = child.stdout.take().expect("piped monitor stdout");
    for event in serde_json::Deserializer::from_reader(std::io::BufReader::new(stdout))
        .into_iter::<Vec<serde_json::Value>>()
    {
        if let Err(error) = event {
            let _ = child.kill();
            let _ = child.wait();
            return exit_with_error(&format!("watch: {error}"));
        }
        let settings = AppSettings::load();
        let mut echo = settings.echo_cancel.clone();
        biglinux_microphone::services::echo::settle(&mut echo);
        if echo != settings.echo_cancel {
            let _ = autostart();
        }
    }
    let _ = child.wait();
    exit_with_error("PipeWire monitor disconnected")
}
