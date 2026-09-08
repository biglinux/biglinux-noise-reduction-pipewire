// SPDX-License-Identifier: MIT

//! Atomic JSON persistence primitives for BigLinux apps.
//!
//! `BigAtomicJsonStore` writes through a `<path>.tmp.<pid>.<ts>` sibling
//! file and then renames into place. `rename(2)` is atomic on the same
//! filesystem; partial writes from a crash are bound to the temp file
//! and visible only after a successful rename.
//!
//! `BigVersionedJsonStore` layers a `version` field on top so old payload
//! shapes can be migrated forward at load time.
//!
//! Both types are display-free, panic-free, and serde-friendly.
//!
//! # Examples
//!
//! ```
//! use big_os_kit::storage::BigAtomicJsonStore;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
//! struct Prefs { volume: u8 }
//!
//! let mut path = std::env::temp_dir();
//! path.push(format!("big-app-kit-storage-doctest-{}.json", std::process::id()));
//! let store: BigAtomicJsonStore<Prefs> = BigAtomicJsonStore::new(&path);
//! store.write(&Prefs { volume: 50 }).unwrap();
//! let loaded = store.read().unwrap().unwrap();
//! assert_eq!(loaded.volume, 50);
//! let _ = std::fs::remove_file(&path);
//! ```

use std::fs;
use std::io::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Serialize, de::DeserializeOwned};

/// Failure modes when reading or writing.
#[derive(Debug)]
pub enum BigStorageError {
    /// Underlying I/O failure (open, read, fsync, rename).
    Io(std::io::Error),
    /// JSON parse/serialize error from `serde_json`.
    Json(serde_json::Error),
    /// Versioned payload but the version is newer than what this binary
    /// understands. Caller should refuse to silently downgrade.
    UnknownVersion {
        /// Version stamp found in the payload.
        found: u32,
        /// Highest version this binary knows how to read.
        supported_up_to: u32,
    },
}

impl std::fmt::Display for BigStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "storage i/o error: {e}"),
            Self::Json(e) => write!(f, "storage json error: {e}"),
            Self::UnknownVersion {
                found,
                supported_up_to,
            } => write!(
                f,
                "stored schema version {found} is newer than supported {supported_up_to}"
            ),
        }
    }
}

impl std::error::Error for BigStorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::UnknownVersion { .. } => None,
        }
    }
}

impl From<std::io::Error> for BigStorageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for BigStorageError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Atomic JSON read/write keyed by file path.
pub struct BigAtomicJsonStore<T> {
    path: PathBuf,
    _marker: PhantomData<T>,
}

impl<T> BigAtomicJsonStore<T> {
    /// Construct a [`BigAtomicJsonStore`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            _marker: PhantomData,
        }
    }

    /// Return a reference to the `path` exposed by this [`BigAtomicJsonStore`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl<T: Serialize + DeserializeOwned> BigAtomicJsonStore<T> {
    /// Read and deserialize. Returns `Ok(None)` when the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`BigStorageError::Io`] for filesystem read failures (other than missing
    /// file, which yields `Ok(None)`), or [`BigStorageError::Json`] when parsing fails.
    pub fn read(&self) -> Result<Option<T>, BigStorageError> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let value: T = serde_json::from_slice(&bytes)?;
                Ok(Some(value))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BigStorageError::Io(e)),
        }
    }

    /// Serialize and atomically replace the target file.
    ///
    /// # Errors
    ///
    /// Returns [`BigStorageError::Io`] for filesystem failures (mkdir, write, fsync,
    /// or rename), or [`BigStorageError::Json`] when serialization fails.
    pub fn write(&self, value: &T) -> Result<(), BigStorageError> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let temporary_store_path = atomic_sibling_temp_path(&self.path);
        {
            let mut f = fs::File::create(&temporary_store_path)?;
            serde_json::to_writer_pretty(&mut f, value)?;
            f.write_all(b"\n")?;
            f.sync_all()?;
        }
        match fs::rename(&temporary_store_path, &self.path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&temporary_store_path);
                Err(BigStorageError::Io(e))
            }
        }
    }
}

fn atomic_sibling_temp_path(target: &Path) -> PathBuf {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "store".to_string());
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(".{name}.tmp.{pid}.{seq}"))
}

/// Atomically write `bytes` to `path`: write a uniquely-named sibling temp file,
/// `fsync` it, then rename it over the target. A crash mid-write leaves the
/// previous contents intact (a plain `fs::write` truncates first). The temp file
/// is created in the target's parent directory so the rename stays within one
/// filesystem (a cross-filesystem rename fails). Creates the parent dir if
/// missing.
///
/// GTK-free and blocking — offload it (e.g. via
/// `big_relm4_components::file_io::write_bytes_async`) so a slow/network
/// filesystem can't stall the UI thread.
///
/// NOTE: the rename installs a *new* inode, so the result carries the temp
/// file's permissions/owner (default umask), not the previous file's, and any
/// hard links to the old inode are left pointing at the old contents. Right for
/// config/state and fresh files; for editing a user document whose mode must be
/// preserved, write in place instead.
///
/// # Errors
/// Returns the underlying [`std::io::Error`] from the dir create, temp write,
/// `fsync`, or rename.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let temporary_write_path = atomic_sibling_temp_path(path);
    {
        let mut file = fs::File::create(&temporary_write_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    match fs::rename(&temporary_write_path, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temporary_write_path);
            Err(error)
        }
    }
}

/// Migration callback contract used by [`BigVersionedJsonStore`].
///
/// Receives the parsed payload as a generic JSON `Value` plus the
/// declared version. Returns the upgraded payload **and** the new
/// version that it now represents. The store loops until the value is
/// fully migrated or [`BigStorageError::UnknownVersion`] is raised.
pub type BigMigrationFn = fn(
    version: u32,
    payload: serde_json::Value,
) -> Result<(u32, serde_json::Value), BigStorageError>;

/// Versioned wrapper. Stores `{ "version": N, "data": ... }`.
pub struct BigVersionedJsonStore<T> {
    inner: BigAtomicJsonStore<serde_json::Value>,
    current_version: u32,
    migrate: BigMigrationFn,
    _marker: PhantomData<T>,
}

#[derive(Serialize, Deserialize, Debug)]
struct VersionedEnvelope {
    version: u32,
    data: serde_json::Value,
}

use serde::Deserialize;

impl<T: Serialize + DeserializeOwned> BigVersionedJsonStore<T> {
    /// Construct a [`BigVersionedJsonStore`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, current_version: u32, migrate: BigMigrationFn) -> Self {
        Self {
            inner: BigAtomicJsonStore::new(path),
            current_version,
            migrate,
            _marker: PhantomData,
        }
    }

    /// Read, migrating older payloads forward. Returns `Ok(None)` when
    /// the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`BigStorageError::Io`] on read failures, [`BigStorageError::Json`] on
    /// parse failures, or [`BigStorageError::UnknownVersion`] when the on-disk version
    /// exceeds `current_version`.
    pub fn read(&self) -> Result<Option<T>, BigStorageError> {
        let Some(raw) = self.inner.read()? else {
            return Ok(None);
        };
        let envelope: VersionedEnvelope = serde_json::from_value(raw)?;
        let mut version = envelope.version;
        let mut payload_json = envelope.data;
        if version > self.current_version {
            return Err(BigStorageError::UnknownVersion {
                found: version,
                supported_up_to: self.current_version,
            });
        }
        while version < self.current_version {
            let prev = version;
            let (next_version, next_payload_json) = (self.migrate)(version, payload_json)?;
            if next_version <= prev {
                return Err(BigStorageError::UnknownVersion {
                    found: next_version,
                    supported_up_to: self.current_version,
                });
            }
            version = next_version;
            payload_json = next_payload_json;
        }
        let value: T = serde_json::from_value(payload_json)?;
        Ok(Some(value))
    }

    /// Serialize with the current version label.
    ///
    /// # Errors
    ///
    /// Returns [`BigStorageError::Json`] on serialization failure or
    /// [`BigStorageError::Io`] on filesystem failure.
    pub fn write(&self, value: &T) -> Result<(), BigStorageError> {
        let envelope = VersionedEnvelope {
            version: self.current_version,
            data: serde_json::to_value(value)?,
        };
        let raw = serde_json::to_value(&envelope)?;
        self.inner.write(&raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
    struct Prefs {
        volume: u8,
        theme: String,
    }

    fn storage_test_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        p.push(format!("big-app-kit-storage-test-{pid}-{seq}-{name}.json"));
        p
    }

    #[test]
    fn missing_file_returns_none() {
        let store: BigAtomicJsonStore<Prefs> =
            BigAtomicJsonStore::new(storage_test_path("missing"));
        assert!(store.read().unwrap().is_none());
    }

    #[test]
    fn round_trip_preserves_payload() {
        let path = storage_test_path("round-trip");
        let store: BigAtomicJsonStore<Prefs> = BigAtomicJsonStore::new(&path);
        let prefs = Prefs {
            volume: 33,
            theme: "dark".to_string(),
        };
        store.write(&prefs).unwrap();
        let loaded = store.read().unwrap().unwrap();
        assert_eq!(loaded, prefs);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn atomic_write_creates_file_with_exact_bytes() {
        let path = storage_test_path("atomic-create");
        atomic_write(&path, b"hello\0world").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello\0world");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn atomic_write_overwrites_and_leaves_no_temp() {
        let path = storage_test_path("atomic-overwrite");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second-longer").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second-longer");
        // The sibling temp file (`.<name>.tmp.<pid>.<seq>`) must be renamed away,
        // not left behind.
        let parent = path.parent().unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let stray: Vec<_> = fs::read_dir(parent)
            .unwrap()
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.starts_with(&format!(".{name}.tmp."))
            })
            .collect();
        assert!(stray.is_empty(), "temp file left behind: {stray:?}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn write_replaces_existing_content() {
        let path = storage_test_path("replace");
        let store: BigAtomicJsonStore<Prefs> = BigAtomicJsonStore::new(&path);
        store
            .write(&Prefs {
                volume: 1,
                theme: "a".into(),
            })
            .unwrap();
        store
            .write(&Prefs {
                volume: 99,
                theme: "b".into(),
            })
            .unwrap();
        let loaded = store.read().unwrap().unwrap();
        assert_eq!(loaded.volume, 99);
        assert_eq!(loaded.theme, "b");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn temp_file_does_not_remain_after_write() {
        let path = storage_test_path("no-leak");
        let store: BigAtomicJsonStore<Prefs> = BigAtomicJsonStore::new(&path);
        store
            .write(&Prefs {
                volume: 5,
                theme: "x".into(),
            })
            .unwrap();
        let parent = path.parent().unwrap();
        let stem = format!(".{}", path.file_name().unwrap().to_string_lossy());
        let leftover = fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().starts_with(&stem));
        assert!(!leftover, "no .tmp sibling should remain after rename");
        let _ = fs::remove_file(&path);
    }

    fn migrate(
        version: u32,
        value: serde_json::Value,
    ) -> Result<(u32, serde_json::Value), BigStorageError> {
        match version {
            1 => {
                // v1 had {"vol":..}. v2 renames vol -> volume and adds theme.
                let vol = value.get("vol").and_then(|v| v.as_u64()).unwrap_or(0);
                Ok((
                    2,
                    serde_json::json!({ "volume": vol as u8, "theme": "dark" }),
                ))
            }
            _ => Err(BigStorageError::UnknownVersion {
                found: version,
                supported_up_to: 2,
            }),
        }
    }

    #[test]
    fn versioned_migrates_v1_to_v2() {
        let path = storage_test_path("migrate");
        // Write a v1 envelope by hand.
        let envelope = serde_json::json!({
            "version": 1,
            "data": { "vol": 42 }
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&envelope).unwrap()).unwrap();
        let store: BigVersionedJsonStore<Prefs> = BigVersionedJsonStore::new(&path, 2, migrate);
        let loaded = store.read().unwrap().unwrap();
        assert_eq!(loaded.volume, 42);
        assert_eq!(loaded.theme, "dark");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn versioned_rejects_future_version() {
        let path = storage_test_path("future");
        let envelope = serde_json::json!({
            "version": 9999,
            "data": { "volume": 0, "theme": "x" }
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&envelope).unwrap()).unwrap();
        let store: BigVersionedJsonStore<Prefs> = BigVersionedJsonStore::new(&path, 2, migrate);
        let err = store.read().unwrap_err();
        assert!(matches!(err, BigStorageError::UnknownVersion { .. }));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn error_display_and_source() {
        use std::error::Error as _;
        // Display renders the wrapped message; source() exposes the cause for
        // wrapping variants and None for the version mismatch.
        let io = BigStorageError::Io(std::io::Error::other("boom"));
        assert!(
            io.to_string().contains("boom"),
            "Display must render the i/o cause"
        );
        assert!(io.source().is_some(), "Io must expose its source");

        let unknown = BigStorageError::UnknownVersion {
            found: 5,
            supported_up_to: 2,
        };
        let display_message = unknown.to_string();
        assert!(
            display_message.contains('5') && display_message.contains('2'),
            "got {display_message:?}"
        );
        assert!(
            unknown.source().is_none(),
            "version mismatch has no inner source"
        );
    }

    #[test]
    fn read_surfaces_non_missing_errors() {
        // A read failure that is NOT "file not found" (here the path is a
        // directory) must propagate as an error, never be swallowed as Ok(None).
        let dir = std::env::temp_dir().join(format!(
            "bigosk-isdir-{}-{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let store: BigAtomicJsonStore<Prefs> = BigAtomicJsonStore::new(&dir);
        assert!(
            store.read().is_err(),
            "reading a directory must error, not return None",
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_creates_missing_parent_dirs() {
        // Both the store and the free atomic_write must `mkdir -p` the parent
        // before writing the temp file.
        let base = std::env::temp_dir().join(format!(
            "bigosk-mkdir-{}-{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let nested = base.join("a").join("b").join("prefs.json");
        let store: BigAtomicJsonStore<Prefs> = BigAtomicJsonStore::new(&nested);
        store
            .write(&Prefs {
                volume: 7,
                theme: "z".into(),
            })
            .unwrap();
        assert_eq!(store.read().unwrap().unwrap().volume, 7);

        let raw = base.join("c").join("d").join("blob.bin");
        atomic_write(&raw, b"xyz").unwrap();
        assert_eq!(fs::read(&raw).unwrap(), b"xyz");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn versioned_reads_current_version_without_migrating() {
        // version == current must read straight through (no migration, no
        // reject) — pins the `>` boundary in read against a `>=`.
        let path = storage_test_path("current-version");
        let envelope = serde_json::json!({
            "version": 2,
            "data": { "volume": 8, "theme": "ok" }
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&envelope).unwrap()).unwrap();
        let store: BigVersionedJsonStore<Prefs> = BigVersionedJsonStore::new(&path, 2, migrate);
        assert_eq!(store.read().unwrap().unwrap().volume, 8);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn versioned_write_then_read_round_trips() {
        // store.write must actually persist — the by-hand-envelope tests never
        // call it. A no-op write would make the subsequent read return None.
        let path = storage_test_path("versioned-write");
        let store: BigVersionedJsonStore<Prefs> = BigVersionedJsonStore::new(&path, 2, migrate);
        store
            .write(&Prefs {
                volume: 71,
                theme: "wr".into(),
            })
            .unwrap();
        let loaded = store
            .read()
            .unwrap()
            .expect("written value must be readable");
        assert_eq!(loaded.volume, 71);
        assert_eq!(loaded.theme, "wr");
        let _ = fs::remove_file(&path);
    }
}
