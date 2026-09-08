from pathlib import Path
import re
from _common import done, replace, write, commit

TITLE = 'fix(settings): serialize writers and preserve concurrent independent edits'
if not done(TITLE):
    write('src/config/storage.rs', r'''//! Settings transactions shared by GUI workers and command-line writers.
//!
//! The lock lives beside settings.json, not on its replaceable inode. Hold
//! it across read, merge, save and graph reconciliation. Only worker threads
//! may acquire it: contention is bounded and never blocks the GTK main loop.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::AppSettings;

pub struct SettingsLock {
    _file: File,
}

impl SettingsLock {
    pub fn acquire() -> io::Result<Self> {
        Self::at(&super::settings_file().with_extension("lock"), Duration::from_secs(5))
    }

    pub fn at(path: &Path, timeout: Duration) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("settings lock is not a regular file"));
        }
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) if started.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(io::Error::new(io::ErrorKind::WouldBlock,
                        "Another settings change is still being applied. Try again."));
                }
                Err(TryLockError::Error(error)) => return Err(error),
            }
        }
    }
}

/// Merge only fields changed since `baseline`. A conflict on the same field
/// is an error, not permission to silently overwrite another process's edit.
pub fn merge(baseline: &AppSettings, desired: &AppSettings, latest: &AppSettings) -> io::Result<AppSettings> {
    let base = serde_json::to_value(baseline).map_err(io::Error::other)?;
    let local = serde_json::to_value(desired).map_err(io::Error::other)?;
    let remote = serde_json::to_value(latest).map_err(io::Error::other)?;
    serde_json::from_value(merge_value(&base, &local, &remote, "settings")?)
        .map_err(io::Error::other)
}

fn merge_value(base: &Value, local: &Value, remote: &Value, path: &str) -> io::Result<Value> {
    if local == base {
        return Ok(remote.clone());
    }
    if remote == base || remote == local {
        return Ok(local.clone());
    }
    if let (Some(base), Some(local), Some(remote)) = (base.as_object(), local.as_object(), remote.as_object()) {
        let mut merged = remote.clone();
        for (key, value) in local {
            let old = base.get(key).unwrap_or(&Value::Null);
            let current = remote.get(key).unwrap_or(&Value::Null);
            merged.insert(key.clone(), merge_value(old, value, current, &format!("{path}.{key}"))?);
        }
        return Ok(Value::Object(merged));
    }
    Err(io::Error::new(io::ErrorKind::WouldBlock,
        format!("{path} changed in another application. Reload the settings before trying again.")))
}

/// Incorporate settled worker values while keeping edits made after dispatch.
/// Unlike a disk conflict, a newer edit in this same UI has a known ordering.
pub fn rebase_local(submitted: &AppSettings, current: &AppSettings, settled: &AppSettings) -> AppSettings {
    let values = (serde_json::to_value(submitted), serde_json::to_value(current), serde_json::to_value(settled));
    let (Ok(base), Ok(local), Ok(remote)) = values else { return current.clone(); };
    serde_json::from_value(rebase_value(&base, &local, &remote)).unwrap_or_else(|_| current.clone())
}

fn rebase_value(base: &Value, local: &Value, remote: &Value) -> Value {
    if local == base {
        return remote.clone();
    }
    if let (Some(base), Some(local), Some(remote)) = (base.as_object(), local.as_object(), remote.as_object()) {
        let mut result = remote.clone();
        for (key, value) in local {
            result.insert(key.clone(), rebase_value(base.get(key).unwrap_or(&Value::Null), value, remote.get(key).unwrap_or(&Value::Null)));
        }
        Value::Object(result)
    } else {
        local.clone()
    }
}

/// Keep unrecognized extension fields when saving a supported configuration.
/// A corrupt file is never replaced with defaults implicitly.
pub(super) fn serialized_preserving_unknown(settings: &AppSettings, path: &Path) -> io::Result<Vec<u8>> {
    let known = serde_json::to_value(settings).map_err(io::Error::other)?;
    let mut stored = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Value::Object(Default::default()),
        Err(error) => return Err(error),
    };
    if !stored.is_object() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "settings must be a JSON object"));
    }
    overlay(&mut stored, &known);
    let mut bytes = serde_json::to_vec_pretty(&stored).map_err(io::Error::other)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn overlay(stored: &mut Value, known: &Value) {
    if let (Some(stored), Some(known)) = (stored.as_object_mut(), known.as_object()) {
        for (key, value) in known {
            overlay(stored.entry(key).or_insert(Value::Null), value);
        }
    } else {
        *stored = known.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_edits_survive_in_either_order() {
        let base = AppSettings::default();
        let mut ui = base.clone();
        let mut cli = base.clone();
        ui.window.width = 920;
        cli.noise_reduction.strength = 0.37;
        let merged = merge(&base, &ui, &cli).unwrap();
        assert_eq!(merged.window.width, 920);
        assert_eq!(merged.noise_reduction.strength, 0.37);
        assert_eq!(merged, merge(&base, &cli, &ui).unwrap());
    }

    #[test]
    fn conflicting_edits_are_not_silently_lost() {
        let base = AppSettings::default();
        let mut ui = base.clone();
        let mut cli = base.clone();
        ui.noise_reduction.strength = 0.25;
        cli.noise_reduction.strength = 0.75;
        assert_eq!(merge(&base, &ui, &cli).unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(merge(&base, &ui, &ui).unwrap(), ui);
    }

    #[test]
    fn worker_normalization_keeps_newer_ui_edits() {
        let submitted = AppSettings::default();
        let mut current = submitted.clone();
        let mut settled = submitted.clone();
        current.noise_reduction.strength = 0.41;
        settled.noise_reduction.model = super::super::NoiseModel::DeepFilterNet3;
        let result = rebase_local(&submitted, &current, &settled);
        assert_eq!(result.noise_reduction.strength, 0.41);
        assert_eq!(result.noise_reduction.model, settled.noise_reduction.model);
    }

    #[cfg(not(miri))]
    #[test]
    fn lock_has_a_deadline_and_releases_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.lock");
        let first = SettingsLock::at(&path, Duration::ZERO).unwrap();
        assert!(SettingsLock::at(&path, Duration::from_millis(20)).is_err());
        drop(first);
        assert!(SettingsLock::at(&path, Duration::ZERO).is_ok());
    }

    #[test]
    fn saving_preserves_extension_fields_and_refuses_corrupt_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, br#"{"extension":{"value":42},"noise_reduction":{"future":true}}"#).unwrap();
        let bytes = serialized_preserving_unknown(&AppSettings::default(), &path).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["extension"]["value"], 42);
        assert_eq!(value["noise_reduction"]["future"], true);
        std::fs::write(&path, "{broken").unwrap();
        assert!(serialized_preserving_unknown(&AppSettings::default(), &path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "{broken");
    }
}
''')
    replace('src/config.rs', 'mod quality;\n', 'mod quality;\npub mod storage;\n')
    replace('src/config.rs', '''        let mut json = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        json.push(b'\\n');''', '''        let json = storage::serialized_preserving_unknown(self, path)?;''')
    replace('src/config.rs', '''        atomic_write_private(path, &json)?;''', '''        if std::fs::read(path).is_ok_and(|existing| existing == json) {
            return Ok(());
        }
        atomic_write_private(path, &json)?;''')
    replace('src/config.rs', '''    std::fs::File::open(parent)?.sync_all()''', '''    std::fs::File::open(path)?.sync_all()?;
    std::fs::File::open(parent)?.sync_all()''')
    # A strict entry point for transactions. Keep read-only legacy callers
    # compatible, but never let a failed read become a destructive write.
    replace('src/config.rs', '''impl AppSettings {''', '''impl AppSettings {
    pub fn load_strict() -> io::Result<Self> {
        let path = settings_file();
        match std::fs::read(&path) {
            Ok(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if !value.is_object() {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "settings must be a JSON object"));
                }
                // Validate the schema before using the compatibility loader.
                // Old quality spelling is accepted by its serde alias.
                serde_json::from_value::<Self>(value).map_err(|error|
                    io::Error::new(io::ErrorKind::InvalidData, error))?;
                Ok(Self::load_from(&path))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }
''')
    # All production CLI writers take the stable lock before the first read.
    cli = 'src/bin/cli.rs'
    for name in ['apply_configs', 'remove_configs', 'autostart', 'reload_services', 'toggle_mic', 'toggle_output', 'repair']:
        replace(cli, f'fn {name}() -> ExitCode {{', f'''fn {name}() -> ExitCode {{
    let _settings_lock = match biglinux_microphone::config::storage::SettingsLock::acquire() {{
        Ok(lock) => lock,
        Err(error) => return exit_with_error(&error.to_string()),
    }};''')
    replace(cli, '''fn set_one(key: Option<String>, value: Option<String>) -> ExitCode {''', '''fn set_one(key: Option<String>, value: Option<String>) -> ExitCode {
    let _settings_lock = match biglinux_microphone::config::storage::SettingsLock::acquire() {
        Ok(lock) => lock,
        Err(error) => return exit_with_error(&error.to_string()),
    };''')
    state = 'src/ui/state.rs'
    replace(state, '''    previous: Option<AppSettings>,
    snapshot: AppSettings,''', '''    previous: Option<AppSettings>,
    baseline: AppSettings,
    snapshot: AppSettings,''')
    replace(state, '''run_apply(self.previous, self.snapshot, self.loopback)''', '''run_apply(self.previous, self.baseline, self.snapshot, self.loopback)''')
    replace(state, '''        Rc::new(Self {
            settings: RefCell::new(settings),''', '''        let persisted = settings.clone();
        Rc::new(Self {
            settings: RefCell::new(settings),''')
    replace(state, '''last_persisted: RefCell::new(None),''', '''last_persisted: RefCell::new(Some(persisted)),''')
    replace(state, '''            previous: prev,
            snapshot,''', '''            previous: prev,
            baseline: self.last_persisted.borrow().clone().unwrap_or_else(|| snapshot.clone()),
            snapshot,''')
    replace(state, '''        if outcome.was_persisted {
            *self.last_persisted.borrow_mut() = Some(outcome.snapshot.clone());
        }''', '''        if outcome.was_persisted {
            if let Some(local) = &local_snapshot {
                let rebased = crate::config::storage::rebase_local(
                    &local.snapshot, &self.settings.borrow(), &outcome.snapshot);
                *self.settings.borrow_mut() = rebased;
            }
            *self.last_persisted.borrow_mut() = Some(outcome.snapshot.clone());
        }''')
    replace(state, '''    prev: Option<AppSettings>,
    mut snapshot: AppSettings,
    loopback_in: Option<Loopback>,
) -> ApplyOutcome {
''', '''    prev: Option<AppSettings>,
    baseline: AppSettings,
    mut snapshot: AppSettings,
    loopback_in: Option<Loopback>,
) -> ApplyOutcome {
    let transaction = crate::config::storage::SettingsLock::acquire()
        .and_then(|guard| {
            let latest = AppSettings::load_strict()?;
            let merged = crate::config::storage::merge(&baseline, &snapshot, &latest)?;
            Ok((guard, merged))
        });
    let (_settings_lock, merged) = match transaction {
        Ok(transaction) => transaction,
        Err(error) => return ApplyOutcome {
            snapshot,
            loopback: loopback_in,
            was_persisted: false,
            status: ApplyStatus::Failed(error.to_string()),
        },
    };
    snapshot = merged;
''')
    replace(state, '''        *self.settings.borrow_mut() = new;
        true''', '''        let baseline = self.last_persisted.borrow().clone()
            .unwrap_or_else(|| self.settings.borrow().clone());
        match crate::config::storage::merge(&baseline, &self.settings.borrow(), &new) {
            Ok(merged) => {
                *self.last_persisted.borrow_mut() = Some(new);
                *self.settings.borrow_mut() = merged;
                true
            }
            Err(error) => {
                log::warn!("settings: preserving local edits after external conflict: {error}");
                false
            }
        }''')
    # Do not reload partially settled local writes from the file monitor.
    replace(state, '''    pub fn settings(&self) -> std::cell::Ref<'_, AppSettings> {''', '''    pub(super) fn has_active_apply(&self) -> bool {
        self.local_apply_snapshot.borrow().is_some()
    }

    pub fn settings(&self) -> std::cell::Ref<'_, AppSettings> {''')
    shell = 'src/ui/mic_shell.rs'
    replace(shell, '''    settings_loads: SettingsLoadTracker,''', '''    settings_loads: SettingsLoadTracker,
    settings_reload_pending: bool,''')
    replace(shell, '''            settings_loads: SettingsLoadTracker::default(),''', '''            settings_loads: SettingsLoadTracker::default(),
            settings_reload_pending: false,''')
    replace(shell, '''            MicInput::ExternalSettingsChanged => {
''', '''            MicInput::ExternalSettingsChanged => {
                if self.state.has_active_apply() {
                    self.settings_reload_pending = true;
                    return;
                }
''')
    replace(shell, '''                if !tracking.is_settled {
                    return;
                }
''', '''                if !tracking.is_settled {
                    return;
                }
                if std::mem::take(&mut self.settings_reload_pending) && !self.is_closing {
                    let _ = sender.input_sender().send(MicInput::ExternalSettingsChanged);
                }
''')
    commit(TITLE, ['src/config.rs', 'src/config/storage.rs', cli, state, shell])
