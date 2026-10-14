//! Settings transactions shared by GUI workers and command-line writers.
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
        Self::at(
            &super::settings_file().with_extension("lock"),
            Duration::from_secs(5),
        )
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
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "Another settings change is still being applied. Try again.",
                    ));
                }
                Err(TryLockError::Error(error)) => return Err(error),
            }
        }
    }
}

/// Merge only fields changed since `baseline`. A conflict on the same field
/// is an error, not permission to silently overwrite another process's edit.
pub fn merge(
    baseline: &AppSettings,
    desired: &AppSettings,
    latest: &AppSettings,
) -> io::Result<AppSettings> {
    let base = serde_json::to_value(baseline).map_err(io::Error::other)?;
    let local = serde_json::to_value(desired).map_err(io::Error::other)?;
    let remote = serde_json::to_value(latest).map_err(io::Error::other)?;
    serde_json::from_value(merge_value(&base, &local, &remote, "settings")?)
        .map_err(io::Error::other)
}

fn merge_value(base: &Value, local: &Value, remote: &Value, path: &str) -> io::Result<Value> {
    merge_entry(Some(base), Some(local), Some(remote), path)?
        .ok_or_else(|| io::Error::other("settings root was removed"))
}

// Absence is not JSON null. A local deletion must survive an unrelated remote
// edit, but deleting a remotely edited entry is a genuine three-way conflict.
fn merge_entry(
    base: Option<&Value>,
    local: Option<&Value>,
    remote: Option<&Value>,
    path: &str,
) -> io::Result<Option<Value>> {
    if local == base {
        return Ok(remote.cloned());
    }
    if remote == base || remote == local {
        return Ok(local.cloned());
    }
    if let (Some(base), Some(local), Some(remote)) = (
        base.and_then(Value::as_object),
        local.and_then(Value::as_object),
        remote.and_then(Value::as_object),
    ) {
        let keys: std::collections::BTreeSet<_> = base
            .keys()
            .chain(local.keys())
            .chain(remote.keys())
            .collect();
        let mut merged = serde_json::Map::new();
        for key in keys {
            if let Some(value) = merge_entry(
                base.get(key),
                local.get(key),
                remote.get(key),
                &format!("{path}.{key}"),
            )? {
                merged.insert(key.clone(), value);
            }
        }
        return Ok(Some(Value::Object(merged)));
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        format!("{path} changed in another application. Reload the settings before trying again."),
    ))
}

/// Incorporate settled worker values while keeping edits made after dispatch.
/// Unlike a disk conflict, a newer edit in this same UI has a known ordering.
pub fn rebase_local(
    submitted: &AppSettings,
    current: &AppSettings,
    settled: &AppSettings,
) -> AppSettings {
    let values = (
        serde_json::to_value(submitted),
        serde_json::to_value(current),
        serde_json::to_value(settled),
    );
    let (Ok(base), Ok(local), Ok(remote)) = values else {
        return current.clone();
    };
    serde_json::from_value(rebase_value(&base, &local, &remote)).unwrap_or_else(|_| current.clone())
}

fn rebase_value(base: &Value, local: &Value, remote: &Value) -> Value {
    rebase_entry(Some(base), Some(local), Some(remote)).unwrap_or_else(|| local.clone())
}
fn rebase_entry(
    base: Option<&Value>,
    local: Option<&Value>,
    remote: Option<&Value>,
) -> Option<Value> {
    if local == base {
        return remote.cloned();
    }
    if let (Some(base), Some(local), Some(remote)) = (
        base.and_then(Value::as_object),
        local.and_then(Value::as_object),
        remote.and_then(Value::as_object),
    ) {
        let keys: std::collections::BTreeSet<_> = base
            .keys()
            .chain(local.keys())
            .chain(remote.keys())
            .collect();
        let mut merged = serde_json::Map::new();
        for key in keys {
            if let Some(value) = rebase_entry(base.get(key), local.get(key), remote.get(key)) {
                merged.insert(key.clone(), value);
            }
        }
        return Some(Value::Object(merged));
    }
    local.cloned() // a newer local deletion is newer local intent too
}

/// Keep unknown schema fields, including fields within retained profiles and
/// calibrations. Only user-owned map keys may be deleted by the known model.
const OWNED_MAPS: [&str; 2] = ["/profiles/profiles", "/mic_calibration/devices"];

pub(super) fn serialized_preserving_unknown(
    settings: &AppSettings,
    path: &Path,
) -> io::Result<Vec<u8>> {
    let known = serde_json::to_value(settings).map_err(io::Error::other)?;
    // REPLACE_DESTINATION replaces the link itself. Never read or modify
    // the unrelated file a settings symlink may point at during that write.
    let existing = if path.is_symlink() {
        Err(io::Error::from(io::ErrorKind::NotFound))
    } else {
        std::fs::read(path)
    };
    let mut stored = match existing {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Value::Object(serde_json::Map::default())
        }
        Err(error) => return Err(error),
    };
    if !stored.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "settings must be a JSON object",
        ));
    }
    for path in OWNED_MAPS {
        if let (Some(stored_map), Some(known_map)) = (
            stored.pointer_mut(path).and_then(Value::as_object_mut),
            known.pointer(path).and_then(Value::as_object),
        ) {
            stored_map.retain(|key, _| known_map.contains_key(key));
        }
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
    fn a_scenario_somebody_deleted_is_actually_gone() {
        // The overlay preserves what a newer version wrote, which means it can
        // never delete a key. For sections whose keys are the person's own
        // data that is not preservation, it is a delete button that does
        // nothing: measured here, `profile delete` reported success and left
        // the scenario in the file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            br#"{"profiles":{"active":"live","profiles":{"live":{"label":"Live"}}},
                 "mic_calibration":{"devices":{"mic":{"volume":1.4}}},
                 "noise_reduction":{"enabled":true,"a_field_from_the_future":7}}"#,
        )
        .unwrap();

        let settings = AppSettings::default();
        let bytes = serialized_preserving_unknown(&settings, &path).unwrap();
        let written: Value = serde_json::from_slice(&bytes).unwrap();

        assert!(
            written["profiles"]["profiles"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert!(
            written["mic_calibration"]["devices"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        // And the reason the overlay exists still holds for everything else.
        assert_eq!(written["noise_reduction"]["a_field_from_the_future"], 7);
    }

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
        assert_eq!(
            merge(&base, &ui, &cli).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(merge(&base, &ui, &ui).unwrap(), ui);
    }

    #[test]
    fn worker_normalization_keeps_newer_ui_edits() {
        let submitted = AppSettings::default();
        let mut current = submitted.clone();
        let mut settled = submitted.clone();
        current.noise_reduction.strength = 0.41;
        settled.noise_reduction.model = super::super::NoiseModel::Dpdfnet;
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
        std::fs::write(
            &path,
            br#"{"extension":{"value":42},"noise_reduction":{"future":true}}"#,
        )
        .unwrap();
        let bytes = serialized_preserving_unknown(&AppSettings::default(), &path).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["extension"]["value"], 42);
        assert_eq!(value["noise_reduction"]["future"], true);
        std::fs::write(&path, "{broken").unwrap();
        assert!(serialized_preserving_unknown(&AppSettings::default(), &path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "{broken");
    }
    #[test]
    fn three_way_merge_and_rebase_preserve_deletions_not_just_changes() {
        let base = serde_json::json!({"devices":{"a":1,"b":2},"other":0});
        let local = serde_json::json!({"devices":{"b":2},"other":0});
        let remote = serde_json::json!({"devices":{"a":1,"b":3},"other":4});
        let expected = serde_json::json!({"devices":{"b":3},"other":4});
        assert_eq!(
            merge_value(&base, &local, &remote, "root").unwrap(),
            expected
        );
        assert_eq!(rebase_value(&base, &local, &remote), expected);
    }
    #[test]
    fn deleting_a_concurrently_changed_entry_conflicts_but_null_is_a_value() {
        let base = serde_json::json!({"a":1});
        let local = serde_json::json!({});
        let remote = serde_json::json!({"a":2});
        assert_eq!(
            merge_value(&base, &local, &remote, "root")
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(rebase_value(&base, &local, &remote), local);
        assert_eq!(
            merge_entry(None, Some(&Value::Null), None, "a").unwrap(),
            Some(Value::Null)
        );
        assert_eq!(
            merge_entry(Some(&Value::Null), None, Some(&Value::Null), "a").unwrap(),
            None
        );
    }
    #[test]
    fn saving_keeps_future_schema_fields_inside_retained_calibrations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, br#"{"mic_calibration":{"future":7,"devices":{"mic":{"volume":1.0,"future":8},"deleted":{"volume":1}}}}"#).unwrap();
        let mut settings = AppSettings::default();
        settings.mic_calibration.devices.insert(
            "mic".into(),
            super::super::DeviceCalibration {
                volume: 1.0,
                ..Default::default()
            },
        );
        let value: Value =
            serde_json::from_slice(&serialized_preserving_unknown(&settings, &path).unwrap())
                .unwrap();
        assert_eq!(value["mic_calibration"]["future"], 7);
        assert_eq!(value["mic_calibration"]["devices"]["mic"]["future"], 8);
        assert!(value["mic_calibration"]["devices"].get("deleted").is_none());
    }
}
