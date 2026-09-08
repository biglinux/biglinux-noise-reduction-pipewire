from pathlib import Path
import re
from _common import done, replace, write, commit


def function(path, name, replacement):
    file = Path(path)
    text = file.read_text()
    pattern = rf'(?ms)^fn {name}\([^\n]*(?:\n.*?)*?^}}'
    # Top-level Rust functions end with an unindented closing brace.
    pattern = rf'(?ms)^fn {name}\(.*?^}}'
    matches = list(re.finditer(pattern, text))
    if len(matches) != 1:
        raise RuntimeError(f'{path}: function {name}: {len(matches)} matches')
    match = matches[0]
    file.write_text(text[:match.start()] + replacement + text[match.end():])


TITLE = 'fix(audio): share verified reconciliation between GUI and CLI'
if not done(TITLE):
    state_path = Path('src/ui/state.rs')
    state = state_path.read_text()
    topology = state[state.index('fn needs_mic_reload('):state.index('\n#[cfg(test)]\n#[path = "state_tests.rs"]')]
    topology = topology.replace('fn needs_mic_reload(', 'pub fn needs_mic_reload(').replace('fn output_topology_changed(', 'pub fn output_topology_changed(')
    write('src/services/reconcile.rs', r'''//! One checked reconciliation path for the GUI, CLI and login watcher.
//!
//! Call while holding SettingsLock on a worker. The durable settings express
//! intent; this module separately observes the graph and records a baseline
//! only after the requested nodes exist. Suspended nodes are valid endpoints
//! but cannot accept live controls, so their arguments are reloaded instead.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use serde_json::Value;

use crate::config::AppSettings;
use crate::pipeline;
use super::pipewire::{self, LiveOutcome};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Observed {
    pub mic_present: bool,
    pub output_present: bool,
    pub aec_present: bool,
}

pub fn observe() -> io::Result<Observed> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-dump")
        .allow_list(["/usr/bin/pw-dump"])
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(2))
        .build().run().map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other("PipeWire is not responding"));
    }
    let graph: Vec<Value> = serde_json::from_slice(&output.stdout).map_err(io::Error::other)?;
    Ok(from_graph(&graph))
}

fn from_graph(graph: &[Value]) -> Observed {
    let nodes: HashSet<&str> = graph.iter()
        .filter(|object| object["type"] == "PipeWire:Interface:Node")
        .filter(|object| object["info"]["state"] != "error")
        .filter_map(|object| object["info"]["props"]["node.name"].as_str())
        .collect();
    Observed {
        mic_present: nodes.contains(pipeline::MIC_NODE_NAME) && nodes.contains(pipeline::MIC_CAPTURE_NODE_NAME),
        output_present: nodes.contains(pipeline::OUTPUT_NODE_NAME),
        aec_present: nodes.contains(pipeline::EC_SOURCE_NAME),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action { Keep, Restart, Stop }

fn action(wanted: bool, present: bool, was_wanted: bool, changed: bool, force: bool, pushed: bool) -> Action {
    if !wanted {
        if present || was_wanted { Action::Stop } else { Action::Keep }
    } else if force || changed || !present || !pushed {
        // systemctl restart also starts an inactive unit; unlike start it
        // repairs an active process whose expected node disappeared.
        Action::Restart
    } else {
        Action::Keep
    }
}

fn baseline_path() -> PathBuf {
    dirs::runtime_dir().map(|path| path.join("biglinux-microphone"))
        .unwrap_or_else(crate::config::config_dir).join("last-applied.json")
}

/// Apply saved settings and propagate every required service failure.
pub fn apply(settings: &AppSettings, force: bool) -> io::Result<()> {
    pipeline::apply(settings)?;
    let previous: Option<AppSettings> = std::fs::read(baseline_path()).ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let observed = observe()?;
    let live = pipewire::apply_live(settings)?;
    execute(settings, previous.as_ref(), observed, live, force)?;
    let bytes = serde_json::to_vec(settings).map_err(io::Error::other)?;
    let path = baseline_path();
    if !std::fs::read(&path).is_ok_and(|existing| existing == bytes) {
        crate::config::atomic_write_private(&path, &bytes)?;
    }
    Ok(())
}

fn execute(settings: &AppSettings, previous: Option<&AppSettings>, observed: Observed, live: LiveOutcome, force: bool) -> io::Result<()> {
    let aec = action(settings.echo_cancel.enabled, observed.aec_present,
        previous.is_some_and(|s| s.echo_cancel.enabled), false, force, true);
    let mic = action(pipeline::mic_chain_wanted(settings), observed.mic_present,
        previous.is_some_and(pipeline::mic_chain_wanted), needs_mic_reload(previous, settings), force, live.mic_pushed);
    let output = action(settings.output_filter.enabled, observed.output_present,
        previous.is_some_and(|s| s.output_filter.enabled), output_topology_changed(previous, settings), force, live.output_pushed);

    // The cleaned source must be published before a microphone chain pins it.
    match aec {
        Action::Restart => {
            pipewire::restart_aec_service()?;
            wait_for(|state| state.aec_present, "echo-cancellation source did not appear")?;
        }
        Action::Stop => pipewire::stop_aec_service()?,
        Action::Keep => {}
    }
    match mic {
        Action::Restart => pipewire::restart_mic_service()?,
        Action::Stop => pipewire::stop_mic_service()?,
        Action::Keep => {}
    }
    match output {
        Action::Restart => pipewire::restart_output_service()?,
        Action::Stop => pipewire::stop_output_service()?,
        Action::Keep => {}
    }
    if [aec, mic, output].iter().any(|step| *step != Action::Keep) {
        wait_for(|state| {
            state.mic_present == pipeline::mic_chain_wanted(settings)
                && state.output_present == settings.output_filter.enabled
                && state.aec_present == settings.echo_cancel.enabled
        }, "The audio services did not reach the requested state")?;
    }
    Ok(())
}

fn wait_for(predicate: impl Fn(Observed) -> bool, failure: &str) -> io::Result<()> {
    let started = Instant::now();
    loop {
        if predicate(observe()?) { return Ok(()); }
        if started.elapsed() >= Duration::from_secs(3) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, failure.to_owned()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

''' + topology + r'''

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_enabled_output_or_aec_is_recovered_without_a_setting_change() {
        assert_eq!(action(true, false, true, false, false, false), Action::Restart);
        assert_eq!(action(true, false, true, false, false, true), Action::Restart);
        assert_eq!(action(true, true, true, false, false, true), Action::Keep);
        assert_eq!(action(true, true, true, true, false, true), Action::Restart);
        assert_eq!(action(false, true, true, false, false, false), Action::Stop);
    }

    #[test]
    fn suspended_nodes_exist_but_error_nodes_do_not_count_as_ready() {
        let graph = serde_json::json!([
            {"type":"PipeWire:Interface:Node","info":{"state":"suspended","props":{"node.name":"output-biglinux"}}},
            {"type":"PipeWire:Interface:Node","info":{"state":"error","props":{"node.name":"echo-cancel-source"}}}
        ]);
        let result = from_graph(graph.as_array().unwrap());
        assert!(result.output_present);
        assert!(!result.aec_present);
    }
}
''')
    file = Path('src/services.rs')
    file.write_text(file.read_text() + '\n/// Shared verified apply path for all application surfaces.\npub mod reconcile;\n')
    # Keep existing pure topology regression tests, but run the shared rules.
    start = state.index('use crate::services::pipewire::{')
    end = state.index('\n};', start) + len('\n};')
    state = state[:start] + '#[cfg(test)]\nuse crate::services::reconcile::{needs_mic_reload, output_topology_changed};' + state[end:]
    start = state.index('    // Tier 2 — rewrite on-disk drop-ins')
    end = state.index('\n/// Spawn or kill the `pw-loopback`', start)
    state = state[:start] + r'''    if let Err(error) = crate::services::reconcile::apply(&snapshot, false) {
        return ApplyOutcome {
            snapshot,
            loopback: loopback_in,
            was_persisted: true,
            status: ApplyStatus::Failed(error.to_string()),
        };
    }
    let (loopback, status) = match reconcile_self_listen(prev.as_ref(), &snapshot, loopback_in) {
        Ok(loopback) => (loopback, ApplyStatus::Applied),
        Err(error) => (None, ApplyStatus::Failed(error)),
    };
    ApplyOutcome { snapshot, loopback, was_persisted: true, status }
}
''' + state[end:]
    start = state.index('/// Drive the standalone output unit')
    end = state.index('#[cfg(test)]\n#[path = "state_tests.rs"]', start)
    state = state[:start] + state[end:]
    state = state.replace(') -> Option<Loopback> {', ') -> Result<Option<Loopback>, String> {')
    state = state.replace('        return None;\n    }\n\n    // is_on:', '        return Ok(None);\n    }\n\n    // is_on:')
    state = state.replace('        return loopback;\n', '        return Ok(loopback);\n')
    state = state.replace('            Some(handle)\n', '            Ok(Some(handle))\n')
    state = state.replace('            None\n        }\n    }\n}\n\n#[cfg(test)]', '            Err(format!("Could not start self-listen: {e}"))\n        }\n    }\n}\n\n#[cfg(test)]')
    state = re.sub(r'        match crate::config::storage::merge\(&baseline, &self.settings.borrow\(\), &new\) \{',
        '        let merged = crate::config::storage::merge(&baseline, &self.settings.borrow(), &new);\n        match merged {', state)
    # pipeline now used with its qualified path in the initial migration only.
    state = state.replace('use crate::pipeline;\n', '')
    state_path.write_text(state)

    cli = 'src/bin/cli.rs'
    # A caller keeps the lock alive across this helper and its graph update.
    text = Path(cli).read_text()
    helper = r'''
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
        Ok(()) => { println!("audio settings applied and verified"); ExitCode::SUCCESS }
        Err(error) => exit_with_error(&error),
    }
}
'''
    Path(cli).write_text(text + helper)
    for name, body in {
        'apply_configs': '    reconcile_saved(false)',
        'autostart': '    pipeline::purge_legacy_files();\n    reconcile_saved(false)',
        'reload_services': '    reconcile_saved(true)',
        'repair': '    reconcile_saved(true)',
    }.items():
        function(cli, name, f'fn {name}() -> ExitCode {{\n{body}\n}}')
    for name in ['reconcile_mic_chain', 'reconcile_aec_service', 'reconcile_output_service']:
        function(cli, name, '')
    function(cli, 'remove_configs', r'''fn remove_configs() -> ExitCode {
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
}''')
    for name, mutation in {
        'toggle_mic': '''    if settings.noise_reduction.enabled {
        pipeline::cascade_mic_off(&mut settings);
    } else {
        settings.noise_reduction.enabled = true;
    }''',
        'toggle_output': '    settings.output_filter.enabled = !settings.output_filter.enabled;',
    }.items():
        function(cli, name, f'''fn {name}() -> ExitCode {{
    let _guard = match biglinux_microphone::config::storage::SettingsLock::acquire() {{
        Ok(guard) => guard,
        Err(error) => return exit_with_error(&error.to_string()),
    }};
    let mut settings = match AppSettings::load_strict() {{
        Ok(settings) => settings,
        Err(error) => return exit_with_error(&error.to_string()),
    }};
{mutation}
    match apply_settings(&mut settings, false) {{
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => exit_with_error(&error),
    }}
}}''')
    text = Path(cli).read_text()
    start = text.index('fn set_one(')
    end = text.index('\nfn on_or_off(', start)
    part = text[start:end]
    part = part.replace('let mut settings = AppSettings::load();', '''let mut settings = match AppSettings::load_strict() {
        Ok(settings) => settings,
        Err(error) => return exit_with_error(&error.to_string()),
    };''')
    tail = part.index('    biglinux_microphone::services::echo::settle(&mut settings.echo_cancel);')
    part = part[:tail] + '''    if let Err(error) = apply_settings(&mut settings, false) {
        return exit_with_error(&format!("set {name}: {error}"));
    }
    println!("set {name} = {value}");
    ExitCode::SUCCESS
}
'''
    text = text[:start] + part + text[end:]
    # Topology is no longer guessed from the CLI key. The shared planner
    # compares actual settings and observed nodes for every caller.
    key_start = text.index('    /// Whether honouring this key')
    key_end = text.index('\n}\n\nimpl Cmd', key_start)
    text = text[:key_start] + text[key_end:]
    text = text.replace('use std::io;\n', '')
    text = text.replace('    let _ = autostart();', '''    if autostart() != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }''')
    text = text.replace('        .arg("--monitor")\n', '        .arg("--monitor")\n        .stderr(BigSubprocessOutputMode::Inherit)\n')
    Path(cli).write_text(text)
    # Partial live updates are not a successful application for scripts.
    function(cli, 'live_update', r'''fn live_update() -> ExitCode {
    let settings = AppSettings::load();
    match biglinux_microphone::services::pipewire::apply_live(&settings) {
        Ok(outcome) if outcome.fully_applied(&settings) => ExitCode::SUCCESS,
        Ok(_) => exit_with_error("The filter is not ready. Run reload or repair."),
        Err(error) => exit_with_error(&error.to_string()),
    }
}''')
    commit(TITLE, ['src/services.rs', 'src/services/reconcile.rs', 'src/ui/state.rs', cli])
