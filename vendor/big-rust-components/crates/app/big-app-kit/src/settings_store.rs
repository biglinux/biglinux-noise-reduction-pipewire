// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! The suite settings store: one JSON document per app, debounced atomic
//! persistence, and key-scoped change listeners.
//!
//! Apps own their keys and defaults; this store owns loading, default
//! merging, dirty tracking, the save debounce, and the on-disk envelope
//! (`{_version, metadata, settings}`). Promoted from big-terminal
//! (platform-consolidation M1); big-filemanager and big-editor adopt next.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtk::glib;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use big_os_kit::storage::{BigStorageError, atomic_write};

/// On-disk envelope schema version (`_version`).
pub const BIG_SETTINGS_SCHEMA_VERSION: u32 = 1;

const SAVE_DEBOUNCE: Duration = Duration::from_millis(350);

/// Wildcard key passed to listeners when every setting may have changed.
pub const BIG_SETTINGS_ALL_KEYS: &str = "*";

/// Provenance block persisted next to the settings map.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BigSettingsMetadata {
    /// App version that last wrote the file.
    pub version: String,
    /// Unix seconds of first save.
    pub created_at: f64,
    /// Unix seconds of last save.
    pub modified_at: f64,
}

/// Change listener. Receives the changed key (or
/// [`BIG_SETTINGS_ALL_KEYS`]); return `false` to be pruned.
pub type BigSettingsListener = Rc<dyn Fn(&str) -> bool>;

struct SettingsState {
    path: PathBuf,
    app_version: String,
    entries: Map<String, Value>,
    metadata: Option<BigSettingsMetadata>,
    dirty: bool,
    save_source: Option<glib::SourceId>,
    listeners: Vec<BigSettingsListener>,
}

/// Shared handle to one app settings document. Clone freely; all clones
/// see the same state. Main-loop only (GTK thread), like the widgets it
/// feeds.
#[derive(Clone)]
pub struct BigSettingsStore {
    state: Rc<RefCell<SettingsState>>,
}

impl BigSettingsStore {
    /// Load `path` (or start blank when missing/corrupt is an error the
    /// caller handles), then merge `defaults` for unset keys.
    ///
    /// # Errors
    ///
    /// Returns [`BigStorageError`] when the file exists but cannot be read
    /// or is not a JSON object. Callers typically log and fall back to
    /// [`BigSettingsStore::blank`].
    pub fn open(
        path: &Path,
        app_version: &str,
        defaults: &Map<String, Value>,
    ) -> Result<Self, BigStorageError> {
        let (entries, metadata) = load_document(path)?;
        Ok(Self::assemble(
            path,
            app_version,
            defaults,
            entries,
            metadata,
        ))
    }

    /// Fresh in-memory document for `path` with `defaults` applied.
    #[must_use]
    pub fn blank(path: &Path, app_version: &str, defaults: &Map<String, Value>) -> Self {
        Self::assemble(path, app_version, defaults, Map::new(), None)
    }

    fn assemble(
        path: &Path,
        app_version: &str,
        defaults: &Map<String, Value>,
        mut entries: Map<String, Value>,
        metadata: Option<BigSettingsMetadata>,
    ) -> Self {
        for (key, value) in defaults {
            entries.entry(key.clone()).or_insert_with(|| value.clone());
        }
        Self {
            state: Rc::new(RefCell::new(SettingsState {
                path: path.to_path_buf(),
                app_version: app_version.to_owned(),
                entries,
                metadata,
                dirty: false,
                save_source: None,
                listeners: Vec::new(),
            })),
        }
    }

    /// Raw value for `key`, cloned out of the document.
    #[must_use]
    pub fn value(&self, key: &str) -> Option<Value> {
        self.state.borrow().entries.get(key).cloned()
    }

    /// String value or `fallback` when unset/mistyped.
    #[must_use]
    pub fn string_or(&self, key: &str, fallback: &str) -> String {
        let state = self.state.borrow();
        state
            .entries
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    }

    /// Integer value or `fallback` when unset/mistyped.
    #[must_use]
    pub fn i64_or(&self, key: &str, fallback: i64) -> i64 {
        self.state
            .borrow()
            .entries
            .get(key)
            .and_then(Value::as_i64)
            .unwrap_or(fallback)
    }

    /// Float value or `fallback` when unset/mistyped.
    #[must_use]
    pub fn f64_or(&self, key: &str, fallback: f64) -> f64 {
        self.state
            .borrow()
            .entries
            .get(key)
            .and_then(Value::as_f64)
            .unwrap_or(fallback)
    }

    /// Boolean value or `fallback` when unset/mistyped.
    #[must_use]
    pub fn bool_or(&self, key: &str, fallback: bool) -> bool {
        self.state
            .borrow()
            .entries
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(fallback)
    }

    /// All present keys, for diagnostics and export.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.state.borrow().entries.keys().cloned().collect()
    }

    /// Fill unset keys from `defaults` without overwriting user values or
    /// marking the document dirty. For staged boot flows that compute
    /// defaults after peeking at loaded values.
    pub fn merge_defaults(&self, defaults: &Map<String, Value>) {
        let mut state = self.state.borrow_mut();
        for (key, value) in defaults {
            state
                .entries
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }

    /// Set `key` to `value`; no-op when unchanged. Schedules the debounced
    /// save and notifies listeners with `key`.
    pub fn set(&self, key: &str, value: Value) {
        {
            let mut state = self.state.borrow_mut();
            if state.entries.get(key) == Some(&value) {
                return;
            }
            state.entries.insert(key.to_owned(), value);
            state.dirty = true;
        }
        self.schedule_save();
        self.notify(key);
    }

    /// Remove `key`; no-op when absent. Debounces and notifies like
    /// [`BigSettingsStore::set`].
    pub fn remove(&self, key: &str) {
        {
            let mut state = self.state.borrow_mut();
            if state.entries.remove(key).is_none() {
                return;
            }
            state.dirty = true;
        }
        self.schedule_save();
        self.notify(key);
    }

    /// Drop every entry, re-apply `defaults`, save immediately, and notify
    /// listeners with [`BIG_SETTINGS_ALL_KEYS`].
    ///
    /// # Errors
    ///
    /// Returns [`BigStorageError`] when the immediate save fails; the
    /// in-memory reset still holds and stays dirty for a later flush.
    pub fn reset_to_defaults(&self, defaults: &Map<String, Value>) -> Result<(), BigStorageError> {
        {
            let mut state = self.state.borrow_mut();
            cancel_pending_save(&mut state);
            state.entries.clear();
            for (key, value) in defaults {
                state.entries.insert(key.clone(), value.clone());
            }
            state.dirty = true;
        }
        let result = self.flush_now();
        self.notify(BIG_SETTINGS_ALL_KEYS);
        result
    }

    /// Cancel any pending debounce and save now when dirty.
    ///
    /// # Errors
    ///
    /// Returns [`BigStorageError`] from the atomic write; the document
    /// stays dirty so a later flush retries.
    pub fn flush_now(&self) -> Result<(), BigStorageError> {
        let mut state = self.state.borrow_mut();
        cancel_pending_save(&mut state);
        save_if_dirty(&mut state)
    }

    /// Register a change listener. It fires on the GTK main loop with the
    /// changed key; return `false` to be pruned.
    pub fn subscribe(&self, listener: BigSettingsListener) {
        self.state.borrow_mut().listeners.push(listener);
    }

    /// Notify listeners about a change made by an external writer (host
    /// bridge, module boundary) without touching the document.
    pub fn notify_external_change(&self, key: &str) {
        self.notify(key);
    }

    /// Number of live change listeners currently registered.
    ///
    /// Test seam for the `big-testkit` listener-prune assertion: a
    /// dialog-scoped subscription must not accumulate across open/close cycles.
    /// Not part of the app-facing contract.
    #[doc(hidden)]
    #[must_use]
    pub fn listener_count(&self) -> usize {
        self.state.borrow().listeners.len()
    }

    fn notify(&self, key: &str) {
        let snapshot = self.state.borrow().listeners.clone();
        let stale: Vec<_> = snapshot.into_iter().filter(|cb| !cb(key)).collect();
        if stale.is_empty() {
            return;
        }
        self.state
            .borrow_mut()
            .listeners
            .retain(|cb| !stale.iter().any(|dead| Rc::ptr_eq(cb, dead)));
    }

    fn schedule_save(&self) {
        let weak = Rc::downgrade(&self.state);
        let mut state = self.state.borrow_mut();
        cancel_pending_save(&mut state);
        state.save_source = Some(glib::timeout_add_local_once(SAVE_DEBOUNCE, move || {
            let Some(state) = weak.upgrade() else { return };
            let mut state = state.borrow_mut();
            state.save_source = None;
            if let Err(e) = save_if_dirty(&mut state) {
                log::warn!("debounced settings save failed: {e}");
            }
        }));
    }
}

fn cancel_pending_save(state: &mut SettingsState) {
    if let Some(source) = state.save_source.take() {
        source.remove();
    }
}

fn save_if_dirty(state: &mut SettingsState) -> Result<(), BigStorageError> {
    if !state.dirty {
        return Ok(());
    }
    let now = now_secs();
    let metadata = match state.metadata.take() {
        Some(mut m) => {
            m.modified_at = now;
            state.app_version.clone_into(&mut m.version);
            m
        }
        None => BigSettingsMetadata {
            version: state.app_version.clone(),
            created_at: now,
            modified_at: now,
        },
    };
    let mut doc = Map::new();
    doc.insert(
        "_version".to_owned(),
        Value::from(BIG_SETTINGS_SCHEMA_VERSION),
    );
    doc.insert(
        "metadata".to_owned(),
        serde_json::to_value(&metadata).map_err(BigStorageError::Json)?,
    );
    doc.insert("settings".to_owned(), Value::Object(state.entries.clone()));
    let bytes = serde_json::to_vec_pretty(&Value::Object(doc)).map_err(BigStorageError::Json)?;
    atomic_write(&state.path, &bytes).map_err(BigStorageError::Io)?;
    state.metadata = Some(metadata);
    state.dirty = false;
    Ok(())
}

fn load_document(
    path: &Path,
) -> Result<(Map<String, Value>, Option<BigSettingsMetadata>), BigStorageError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Map::new(), None));
        }
        Err(e) => return Err(BigStorageError::Io(e)),
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(BigStorageError::Json)?;
    let Value::Object(obj) = value else {
        return Err(BigStorageError::Io(std::io::Error::other(
            "settings file must be a JSON object",
        )));
    };
    if obj.contains_key("settings") && obj.contains_key("metadata") {
        let entries = obj
            .get("settings")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let metadata = obj
            .get("metadata")
            .cloned()
            .and_then(|m| serde_json::from_value(m).ok());
        Ok((entries, metadata))
    } else {
        // Legacy layout: bare settings map at the document root.
        Ok((obj, None))
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

// ---------------------------------------------------------------------------
// Suite-level settings: one file for keys that are cross-app BY INTENT.
// ---------------------------------------------------------------------------

/// Directory under the XDG config home that holds suite-wide state.
pub const SUITE_SETTINGS_DIR_NAME: &str = "biglinux";
/// File name of the suite-wide settings document.
pub const SUITE_SETTINGS_FILE_NAME: &str = "suite-settings.json";

/// The suite-settings key registry. A key lives in the suite store ONLY
/// when it appears here; everything else belongs to the owning app's own
/// `settings.json`. Add keys deliberately — a key is suite-wide when two
/// or more apps must observe the same value (the shared AI provider
/// configuration is the founding case). Per-app UI state (panel sides,
/// widths, layouts) never qualifies. API keys never qualify either —
/// secrets live in the keyring.
pub const SUITE_SETTING_KEYS: [&str; 6] = [
    "ai_assistant_enabled",
    "ai_assistant_provider",
    "ai_assistant_model",
    "ai_local_base_url",
    "ai_openrouter_site_name",
    "ai_openrouter_site_url",
];

/// Whether `key` belongs to the suite store per [`SUITE_SETTING_KEYS`].
#[must_use]
pub fn is_suite_setting_key(key: &str) -> bool {
    SUITE_SETTING_KEYS.contains(&key)
}

/// Path of the suite-wide settings document:
/// `$XDG_CONFIG_HOME/biglinux/suite-settings.json`.
#[must_use]
pub fn suite_settings_file() -> PathBuf {
    big_os_kit::xdg_dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join(SUITE_SETTINGS_DIR_NAME)
        .join(SUITE_SETTINGS_FILE_NAME)
}

/// Open the suite-wide settings store (no defaults — suite keys default
/// in the reading app so an absent key keeps app-level fallbacks).
///
/// # Errors
///
/// Returns [`BigStorageError`] when the file exists but cannot be read or
/// parsed; callers typically log and fall back to
/// [`BigSettingsStore::blank`] with the same path.
pub fn open_suite_settings(app_version: &str) -> Result<BigSettingsStore, BigStorageError> {
    BigSettingsStore::open(&suite_settings_file(), app_version, &Map::new())
}

/// One-shot import: copy keys accepted by `key_filter` from a legacy
/// settings document (suite envelope or bare root) into `store`, skipping
/// keys the store already has, then save. The legacy file is never
/// modified. Returns the number of imported keys; `0` when the legacy
/// file is missing.
///
/// # Errors
///
/// Returns [`BigStorageError`] when the legacy file exists but cannot be
/// read/parsed, or when the flush after a non-empty import fails.
pub fn import_legacy_settings(
    store: &BigSettingsStore,
    legacy_file: &Path,
    key_filter: impl Fn(&str) -> bool,
) -> Result<usize, BigStorageError> {
    let (entries, _) = load_document(legacy_file)?;
    let mut imported = 0;
    for (key, value) in entries {
        if key_filter(&key) && store.value(&key).is_none() {
            store.set(&key, value);
            imported += 1;
        }
    }
    if imported > 0 {
        store.flush_now()?;
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn defaults() -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("font".to_owned(), json!("Monospace 12"));
        map.insert("opacity".to_owned(), json!(0.9));
        map
    }

    fn store_in(dir: &tempfile::TempDir) -> BigSettingsStore {
        BigSettingsStore::open(&dir.path().join("settings.json"), "1.0", &defaults())
            .expect("open blank store")
    }

    #[test]
    fn open_missing_file_yields_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(&dir);
        assert_eq!(store.string_or("font", "?"), "Monospace 12");
        assert_eq!(store.f64_or("opacity", 0.0), 0.9);
    }

    #[test]
    fn set_flush_reload_roundtrips_envelope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let store = store_in(&dir);
        store.set("font", json!("Fira Code 11"));
        store.flush_now().expect("flush");

        let raw: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        assert_eq!(raw["_version"], json!(BIG_SETTINGS_SCHEMA_VERSION));
        assert!(raw["metadata"]["version"] == json!("1.0"));
        assert_eq!(raw["settings"]["font"], json!("Fira Code 11"));

        let reloaded = BigSettingsStore::open(&path, "1.1", &defaults()).expect("reopen");
        assert_eq!(reloaded.string_or("font", "?"), "Fira Code 11");
    }

    #[test]
    fn legacy_bare_root_document_still_loads() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"font":"Legacy 10"}"#).expect("seed legacy");
        let store = BigSettingsStore::open(&path, "1.0", &defaults()).expect("open legacy");
        assert_eq!(store.string_or("font", "?"), "Legacy 10");
        assert_eq!(store.f64_or("opacity", 0.0), 0.9);
    }

    #[test]
    fn terminal_metadata_with_checksum_is_tolerated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"_version":1,"metadata":{"version":"2.0.0","created_at":1.0,"modified_at":2.0,"checksum":"abc"},"settings":{"font":"Kept 9"}}"#,
        )
        .expect("seed terminal envelope");
        let store = BigSettingsStore::open(&path, "2.1", &defaults()).expect("open");
        assert_eq!(store.string_or("font", "?"), "Kept 9");
    }

    #[test]
    fn set_notifies_and_prunes_stale_listeners() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(&dir);
        let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let seen_cb = seen.clone();
        store.subscribe(Rc::new(move |key| {
            seen_cb.borrow_mut().push(key.to_owned());
            true
        }));
        store.subscribe(Rc::new(|_| false));

        store.set("font", json!("New 1"));
        store.set("font", json!("New 1")); // unchanged: no notify
        store.set("font", json!("New 2"));
        assert_eq!(*seen.borrow(), vec!["font".to_owned(), "font".to_owned()]);
    }

    #[test]
    fn reset_to_defaults_saves_and_broadcasts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(&dir);
        let hits = Rc::new(RefCell::new(Vec::new()));
        let hits_cb = hits.clone();
        store.subscribe(Rc::new(move |key| {
            hits_cb.borrow_mut().push(key.to_owned());
            true
        }));
        store.set("font", json!("Custom 13"));
        store.reset_to_defaults(&defaults()).expect("reset");
        assert_eq!(store.string_or("font", "?"), "Monospace 12");
        assert!(hits.borrow().contains(&BIG_SETTINGS_ALL_KEYS.to_owned()));
    }

    #[test]
    fn suite_registry_owns_ai_keys_only() {
        assert!(is_suite_setting_key("ai_assistant_provider"));
        assert!(!is_suite_setting_key("ai_panel_side"));
        assert!(!is_suite_setting_key("ai_assistant_api_key"));
        assert!(suite_settings_file().ends_with("biglinux/suite-settings.json"));
    }

    #[test]
    fn legacy_import_copies_filtered_keys_without_overwrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let legacy = dir.path().join("legacy.json");
        std::fs::write(
            &legacy,
            r#"{"_version":1,"metadata":{"version":"2.0","created_at":1.0,"modified_at":2.0},
               "settings":{"ai_assistant_provider":"openrouter","ai_panel_side":"left","font":"Mono 12"}}"#,
        )
        .expect("seed legacy");
        let store = store_in(&dir);
        store.set("ai_assistant_provider", json!("local"));

        let imported =
            import_legacy_settings(&store, &legacy, is_suite_setting_key).expect("import");

        assert_eq!(imported, 0, "existing value never overwritten");
        assert_eq!(store.string_or("ai_assistant_provider", "?"), "local");

        let fresh = BigSettingsStore::blank(&dir.path().join("s2.json"), "1.0", &Map::new());
        let imported =
            import_legacy_settings(&fresh, &legacy, is_suite_setting_key).expect("import");
        assert_eq!(imported, 1);
        assert_eq!(fresh.string_or("ai_assistant_provider", "?"), "openrouter");
        assert_eq!(fresh.value("ai_panel_side"), None);
        assert_eq!(fresh.value("font"), None);
    }

    #[test]
    fn legacy_import_missing_file_is_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(&dir);
        let imported = import_legacy_settings(&store, &dir.path().join("absent.json"), |_| true)
            .expect("import");
        assert_eq!(imported, 0);
    }

    #[test]
    fn remove_deletes_and_notifies_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(&dir);
        let count = Rc::new(RefCell::new(0));
        let count_cb = count.clone();
        store.subscribe(Rc::new(move |_| {
            *count_cb.borrow_mut() += 1;
            true
        }));
        store.remove("font");
        store.remove("font");
        assert_eq!(*count.borrow(), 1);
        assert_eq!(store.value("font"), None);
    }

    #[test]
    fn typed_accessors_return_value_or_typed_fallback() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(&dir);
        store.set("count", json!(42));
        store.set("flag", json!(true));

        // Present and correctly typed.
        assert_eq!(store.i64_or("count", 7), 42);
        assert!(store.bool_or("flag", false));
        // Absent → fallback.
        assert_eq!(store.i64_or("absent", 7), 7);
        assert!(store.bool_or("absent", true));
        // Present but wrong type ("font" is a string) → fallback.
        assert_eq!(store.i64_or("font", 7), 7);
        assert!(!store.bool_or("font", false));
    }

    #[test]
    fn keys_lists_every_present_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(&dir); // defaults: font, opacity
        store.set("extra", json!(1));
        let mut keys = store.keys();
        keys.sort();
        assert_eq!(
            keys,
            vec!["extra".to_owned(), "font".to_owned(), "opacity".to_owned()]
        );
    }

    #[test]
    fn merge_defaults_fills_only_unset_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = BigSettingsStore::blank(&dir.path().join("s.json"), "1.0", &Map::new());
        store.set("font", json!("User Choice"));
        let mut more = Map::new();
        more.insert("font".to_owned(), json!("Default Font"));
        more.insert("newkey".to_owned(), json!("added"));
        store.merge_defaults(&more);
        assert_eq!(store.string_or("font", "?"), "User Choice", "existing kept");
        assert_eq!(store.string_or("newkey", "?"), "added", "unset filled");
    }

    #[test]
    fn notify_external_change_fires_listeners() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(&dir);
        let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let seen_cb = seen.clone();
        store.subscribe(Rc::new(move |key| {
            seen_cb.borrow_mut().push(key.to_owned());
            true
        }));
        store.notify_external_change("ai_assistant_provider");
        assert_eq!(*seen.borrow(), vec!["ai_assistant_provider".to_owned()]);
    }

    #[test]
    fn listener_count_reflects_live_subscriptions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(&dir);
        assert_eq!(store.listener_count(), 0);
        store.subscribe(Rc::new(|_| true));
        store.subscribe(Rc::new(|_| true));
        assert_eq!(store.listener_count(), 2);
    }

    #[test]
    fn open_non_notfound_read_error_propagates() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A directory where the settings file would be: it EXISTS (so the error is
        // not NotFound), but reading it fails — the error must surface, not be
        // swallowed as an empty document.
        let as_dir = dir.path().join("settings.json");
        std::fs::create_dir(&as_dir).expect("mkdir at the settings path");
        let result = BigSettingsStore::open(&as_dir, "1.0", &defaults());
        assert!(
            result.is_err(),
            "a non-NotFound read error must propagate, not yield a blank store"
        );
    }

    #[test]
    fn partial_envelope_keys_load_as_legacy_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        // Has "settings" but NOT "metadata": not a full envelope, so the whole
        // object is the legacy bare-root settings map.
        std::fs::write(&path, r#"{"settings":{"nested":1},"font":"Root 8"}"#).expect("seed");
        let store = BigSettingsStore::open(&path, "1.0", &Map::new()).expect("open");
        assert_eq!(
            store.string_or("font", "?"),
            "Root 8",
            "top-level key is an entry"
        );
        assert!(
            store.value("settings").is_some(),
            "the `settings` object is itself a stored value in legacy layout"
        );
    }

    #[test]
    fn saved_metadata_uses_a_real_timestamp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let store = BigSettingsStore::blank(&path, "9.9", &Map::new());
        store.set("k", json!(1));
        store.flush_now().expect("flush");
        let raw: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        let created = raw["metadata"]["created_at"].as_f64().expect("created_at");
        let modified = raw["metadata"]["modified_at"]
            .as_f64()
            .expect("modified_at");
        // A real Unix timestamp dwarfs the mutation constants (0.0 / 1.0 / -1.0).
        assert!(
            created > 1_000_000_000.0,
            "created_at looks real: {created}"
        );
        assert!(
            modified > 1_000_000_000.0,
            "modified_at looks real: {modified}"
        );
    }

    #[test]
    fn import_flushes_only_when_something_was_imported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let legacy = dir.path().join("legacy.json");
        std::fs::write(
            &legacy,
            r#"{"ai_assistant_provider":"openrouter","font":"x"}"#,
        )
        .expect("seed legacy");

        // (a) importing >=1 key flushes → the destination file is written.
        let dst = dir.path().join("imported.json");
        let store = BigSettingsStore::blank(&dst, "1.0", &Map::new());
        let imported =
            import_legacy_settings(&store, &legacy, is_suite_setting_key).expect("import");
        assert_eq!(imported, 1);
        assert!(dst.exists(), "a non-empty import must flush to disk");

        // (b) importing 0 keys must NOT flush, even when the store is already
        // dirty — guards `imported > 0` against `>= 0`.
        let dst_dirty = dir.path().join("dirty.json");
        let store_dirty = BigSettingsStore::blank(&dst_dirty, "1.0", &Map::new());
        store_dirty.set("pending", json!(1)); // dirty, but the debounce never fires here
        assert!(!dst_dirty.exists(), "no write before flush");
        let missing = dir.path().join("absent.json");
        let imported_zero =
            import_legacy_settings(&store_dirty, &missing, is_suite_setting_key).expect("import");
        assert_eq!(imported_zero, 0);
        assert!(
            !dst_dirty.exists(),
            "a zero-key import must not flush a dirty store to disk"
        );
    }
}
