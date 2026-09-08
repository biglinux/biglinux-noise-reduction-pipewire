from pathlib import Path
import re
from _common import done, replace, write, commit

TITLE = 'fix(systemd): resolve loader configuration using the service XDG environment'
if not done(TITLE):
    path = 'src/bin/pwloader.rs'
    replace(path, '''        let args_path = &pair[1];
        let module_args = read_to_string(std::path::Path::new(args_path))
            .map_err(|e| format!("read {args_path}: {e}"))?;''', '''        let args_path = resolve_args_path(&pair[1])?;
        let module_args = read_to_string(&args_path)
            .map_err(|e| format!("read {}: {e}", args_path.display()))?;''')
    replace(path, 'fn main() -> ExitCode {', '''fn resolve_args_path(argument: &str) -> Result<std::path::PathBuf, String> {
    if let Some(name) = argument.strip_prefix("@config/") {
        if !matches!(name, "mic.args" | "aec.args" | "output.args") {
            return Err("unknown application configuration name".into());
        }
        let root = dirs::config_dir().ok_or("XDG configuration directory is unavailable")?;
        Ok(root.join("biglinux-microphone").join(name))
    } else {
        Ok(std::path::PathBuf::from(argument))
    }
}

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() == Some("--check-config") {
        return match arguments.next().map(|name| resolve_args_path(&name)) {
            Some(Ok(path)) if path.is_file() => ExitCode::SUCCESS,
            _ => ExitCode::FAILURE,
        };
    }
''')
    for kind in ['mic', 'aec', 'output']:
        unit = Path(f'usr/lib/systemd/user/biglinux-microphone-{kind}.service')
        text = unit.read_text()
        text = re.sub(r'^ConditionPathExists=.*\n', '', text, flags=re.M)
        text = text.replace('[Service]\n', f'[Service]\nExecCondition=/usr/bin/biglinux-microphone-pwloader --check-config @config/{kind}.args\n')
        text = text.replace(f'%h/.config/biglinux-microphone/{kind}.args', f'@config/{kind}.args')
        unit.write_text(text)
    write('tests/review_xdg_units.rs', r'''#[test]
fn user_units_resolve_configuration_in_the_process_environment() {
    for (unit, name) in [
        (include_str!("../usr/lib/systemd/user/biglinux-microphone-mic.service"), "mic"),
        (include_str!("../usr/lib/systemd/user/biglinux-microphone-aec.service"), "aec"),
        (include_str!("../usr/lib/systemd/user/biglinux-microphone-output.service"), "output"),
    ] {
        assert!(!unit.contains("%h/.config"));
        assert!(unit.contains(&format!("--check-config @config/{name}.args")));
    }
}
''')
    commit(TITLE, [path, 'usr/lib/systemd/user', 'tests/review_xdg_units.rs'])

TITLE = 'feat(performance): make CPU affinity and memory reservation explicit choices'
if not done(TITLE):
    write('src/config/runtime.rs', '''//! Optional loader resource policies; automatic system scheduling is default.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub prefer_fast_cpus: bool,
    pub reserve_memory: bool,
}
''')
    replace('src/config.rs', 'mod quality;\n', 'mod quality;\nmod runtime;\npub use runtime::RuntimeConfig;\n')
    replace('src/config.rs', 'pub struct AppSettings {', 'pub struct AppSettings {\n    pub runtime: RuntimeConfig,')
    path = 'src/bin/pwloader.rs'
    text = Path(path).read_text()
    start = text.index('/// On Intel/ARM hybrid CPUs'); end = text.index('fn pin_to_p_cores()', start)
    text = text[:start] + '''/// Optional frequency-based preference, restricted to the existing affinity
/// mask. Frequency is only a heuristic, not proof of a P/E-core topology.
''' + text[end:]
    text = text.replace('fn pin_to_p_cores()', 'fn prefer_fast_allowed_cpus()')
    text = text.replace('    let mut cpus: Vec<(usize, u64)> = Vec::new();', '''    // SAFETY: cpu_set_t is an initialized bitset of the size passed to libc.
    let mut allowed: libc::cpu_set_t = unsafe { mem::zeroed() };
    if unsafe { libc::sched_getaffinity(0, mem::size_of::<libc::cpu_set_t>(), &raw mut allowed) } != 0 {
        return;
    }
    let mut cpus: Vec<(usize, u64)> = Vec::new();''')
    text = text.replace('if cpu >= libc::CPU_SETSIZE as usize {', 'if cpu >= libc::CPU_SETSIZE as usize || !unsafe { libc::CPU_ISSET(cpu, &allowed) } {')
    text = text.replace('pinned to P-cores', 'opted in to allowed high-frequency CPUs')
    start = text.index('    // Pin BEFORE pw::init()'); end = text.index('    pw::init();', start)
    text = text[:start] + '''    let preferences = loader_preferences();
    if preferences.prefer_fast_cpus { prefer_fast_allowed_cpus(); }

''' + text[end:]
    text = text.replace('libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE)', 'libc::mlockall(libc::MCL_CURRENT)')
    text = text.replace('expect occasional pw-top spikes from page faults; check RLIMIT_MEMLOCK', 'memory reservation was not enabled; automatic paging remains in use')
    start = text.index('/// `mlockall('); end = text.index('fn lock_pages_in_ram()', start)
    text = text[:start] + '''/// Opt-in reservation of mappings already loaded. Never use MCL_FUTURE:
/// later runtime allocations must not fail merely because a lock budget is
/// exhausted. PipeWire retains ownership of its own real-time buffers.
''' + text[end:]
    text = text.replace('    // Watch whether the denoiser keeps up.', '''    if preferences.reserve_memory { lock_pages_in_ram(); }

    // Watch whether the denoiser keeps up.''')
    at = text.index('/// How often to sample')
    text = text[:at] + '''#[path = "../config/runtime.rs"]
mod runtime_preferences;

fn loader_preferences() -> runtime_preferences::RuntimeConfig {
    let Some(root) = dirs::config_dir() else { return Default::default(); };
    let Some(value) = std::fs::read(root.join("biglinux-microphone/settings.json")).ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()) else {
        return Default::default();
    };
    serde_json::from_value(value["runtime"].clone()).unwrap_or_default()
}

''' + text[at:]
    Path(path).write_text(text)
    path = 'src/services/reconcile.rs'
    replace(path, '    execute(settings, previous.as_ref(), observed, live, force)?;', '''    let force = force || previous.as_ref().is_some_and(|old| old.runtime != settings.runtime);
    execute(settings, previous.as_ref(), observed, live, force)?;''')
    path = 'src/ui/mic_shell.rs'
    replace(path, '    QualityChanged(crate::config::Quality),', '''    QualityChanged(crate::config::Quality),
    PreferFastCpusChanged(bool),
    ReserveMemoryChanged(bool),''')
    replace(path, '            MicInput::QualityChanged(quality) => {', '''            MicInput::PreferFastCpusChanged(enabled) => {
                self.mutate_settings(&sender, |settings| settings.runtime.prefer_fast_cpus = enabled);
            }
            MicInput::ReserveMemoryChanged(enabled) => {
                self.mutate_settings(&sender, |settings| settings.runtime.reserve_memory = enabled);
            }
            MicInput::QualityChanged(quality) => {''')
    path = 'src/ui/views/mic.rs'
    replace(path, '    content.append(quality_card(state, input).widget());', '''    content.append(quality_card(state, input).widget());
    content.append(&resource_options(state, input));''')
    with Path(path).open('a') as file:
        file.write('''
fn resource_options(state: &Rc<AppState>, input: &relm4::Sender<MicInput>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let details = adw::ExpanderRow::builder()
        .title(i18n("Performance options"))
        .subtitle(i18n("Leave these off unless audio still stutters. Changing them briefly restarts the filters."))
        .build();
    let fast = adw::SwitchRow::builder()
        .title(i18n("Prefer faster processor cores"))
        .subtitle(i18n("May help on some computers, but can use more power. The automatic system choice is usually best."))
        .active(state.settings().runtime.prefer_fast_cpus).build();
    let sender = input.clone();
    fast.connect_active_notify(move |row| { let _ = sender.send(MicInput::PreferFastCpusChanged(row.is_active())); });
    let memory = adw::SwitchRow::builder()
        .title(i18n("Keep loaded audio data in memory"))
        .subtitle(i18n("May reduce pauses under memory pressure, but leaves less RAM for other applications. This is optional and limited by the system."))
        .active(state.settings().runtime.reserve_memory).build();
    let sender = input.clone();
    memory.connect_active_notify(move |row| { let _ = sender.send(MicInput::ReserveMemoryChanged(row.is_active())); });
    details.add_row(&fast);
    details.add_row(&memory);
    group.add(&details);
    group
}
''')
    commit(TITLE, ['src/config.rs', 'src/config/runtime.rs', 'src/bin/pwloader.rs', 'src/services/reconcile.rs', 'src/ui/mic_shell.rs', path])

TITLE = 'fix(diagnostics): send native journal fields and release plugin handles'
if not done(TITLE):
    path = 'src/pwloader/denoise_watch.rs'
    text = Path(path).read_text()
    start = text.index('    fn announce('); end = text.index('\n}\n', start)
    text = text[:start] + '''    fn announce(degraded: bool, ratio: f64) {
        let message = journal_message(degraded, ratio);
        let sent = std::os::unix::net::UnixDatagram::unbound().and_then(|socket| {
            socket.set_nonblocking(true)?;
            socket.send_to(message.as_bytes(), "/run/systemd/journal/socket")
        });
        if sent.is_err() {
            eprintln!("noise reduction: {} ({:.1}% processed)",
                if degraded { "not keeping up" } else { "recovered" }, ratio * 100.0);
        }
    }
''' + text[end:]
    pos = text.index('/// Every `.so`')
    text = text[:pos] + '''fn journal_message(degraded: bool, ratio: f64) -> String {
    format!("MESSAGE_ID={MESSAGE_ID}\\nPRIORITY={}\\nDENOISE_RATIO={:.1}\\nMESSAGE=Noise reduction {}\\n",
        if degraded { 4 } else { 6 }, ratio * 100.0,
        if degraded { "is not keeping up" } else { "has recovered" })
}

impl Drop for DenoiseWatch {
    fn drop(&mut self) {
        // SAFETY: this is our owned dlopen reference. No callback outlives
        // the watcher, and PipeWire owns its separate plugin reference.
        unsafe { libc::dlclose(self._handle) };
    }
}

''' + text[pos:]
    text += '''
#[cfg(test)]
mod journal_tests {
    #[test]
    fn journal_fields_belong_to_one_native_datagram() {
        let message = super::journal_message(true, 0.85);
        assert!(message.contains("\\nPRIORITY=4\\n"));
        assert!(message.contains("\\nDENOISE_RATIO=85.0\\n"));
        assert!(message.contains("\\nMESSAGE=Noise reduction is not keeping up\\n"));
    }
}
'''
    Path(path).write_text(text)
    commit(TITLE, [path])
