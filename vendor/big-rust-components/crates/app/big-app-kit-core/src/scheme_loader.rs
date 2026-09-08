// SPDX-License-Identifier: MIT

//! Color-scheme loader contracts.
//!
//! Apps with theme support load schemes from three sources:
//!
//! - built-in (compiled-in palette, immutable);
//! - user (`~/.config/<app>/schemes/*.json`, mutable);
//! - system (`/usr/share/<app>/schemes/*.json`, immutable).
//!
//! `BigColorSchemeLoaderSpec` enumerates the policy; the trait
//! [`crate::scheme_loader::BigColorSchemeSource`] is implemented by both filesystem readers
//! and in-memory test doubles.

use std::collections::HashMap;
use std::path::PathBuf;

/// One scheme entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigColorSchemeEntry {
    /// Stable id (`"solarized-dark"`).
    pub id: String,
    /// Visible name.
    pub label: String,
    /// Origin tier.
    pub source: BigColorSchemeSourceKind,
    /// Optional filesystem path (for user/system tiers).
    pub path: Option<PathBuf>,
}

/// Enumeration of supported big color scheme source kind variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigColorSchemeSourceKind {
    /// Scheme baked into the binary.
    BuiltIn,
    /// Scheme loaded from the user's writable scheme directory.
    User,
    /// Scheme loaded from the read-only system scheme directory.
    System,
}

/// Display-free specification describing big color scheme loader behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigColorSchemeLoaderSpec {
    /// Compiled-in baseline.
    pub built_in_ids: Vec<String>,
    /// Filesystem path for user schemes (must be writable).
    pub user_dir: Option<PathBuf>,
    /// Filesystem path for system schemes (read-only).
    pub system_dir: Option<PathBuf>,
    /// `true` to watch directories for live updates.
    pub hot_reload: bool,
}

impl BigColorSchemeLoaderSpec {
    /// Build a loader spec with the supplied built-in id list and no
    /// filesystem tiers.
    #[must_use]
    pub fn new(built_in_ids: Vec<String>) -> Self {
        Self {
            built_in_ids,
            user_dir: None,
            system_dir: None,
            hot_reload: false,
        }
    }

    /// Builder method returning `Self` with user dir set.
    #[must_use]
    pub fn with_user_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.user_dir = Some(dir.into());
        self
    }

    /// Builder method returning `Self` with system dir set.
    #[must_use]
    pub fn with_system_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.system_dir = Some(dir.into());
        self
    }

    /// Builder method returning `Self` with hot reload set.
    #[must_use]
    pub fn with_hot_reload(mut self) -> Self {
        self.hot_reload = true;
        self
    }
}

/// Trait implemented by the actual loader (filesystem) and by test
/// doubles (in-memory).
pub trait BigColorSchemeSource {
    /// List all available scheme entries across every tier.
    fn list(&self) -> Vec<BigColorSchemeEntry>;

    /// Load the raw bytes for a scheme id.
    ///
    /// # Errors
    /// Implementation-defined; typically I/O failure or missing id.
    fn load(&self, id: &str) -> Result<Vec<u8>, String>;
}

/// In-memory source useful for tests.
#[derive(Debug, Default)]
pub struct BigInMemoryColorSchemeSource {
    entries: HashMap<String, (BigColorSchemeEntry, Vec<u8>)>,
}

impl BigInMemoryColorSchemeSource {
    /// Insert one in-memory scheme. The pair `(entry.id, body)` is
    /// the smallest unit the test source can serve.
    pub fn add(&mut self, entry: BigColorSchemeEntry, body: impl Into<Vec<u8>>) -> &mut Self {
        self.entries.insert(entry.id.clone(), (entry, body.into()));
        self
    }
}

impl BigColorSchemeSource for BigInMemoryColorSchemeSource {
    fn list(&self) -> Vec<BigColorSchemeEntry> {
        self.entries.values().map(|(e, _)| e.clone()).collect()
    }

    fn load(&self, id: &str) -> Result<Vec<u8>, String> {
        self.entries
            .get(id)
            .map(|(_, body)| body.clone())
            .ok_or_else(|| format!("unknown scheme id: {id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_carries_paths_and_hot_reload() {
        let spec = BigColorSchemeLoaderSpec::new(vec!["solarized-dark".into(), "nord".into()])
            .with_user_dir("/home/u/.config/my-app/schemes")
            .with_system_dir("/usr/share/my-app/schemes")
            .with_hot_reload();
        assert_eq!(spec.built_in_ids.len(), 2);
        assert!(spec.hot_reload);
        assert!(spec.user_dir.is_some());
        assert!(spec.system_dir.is_some());
    }

    #[test]
    fn in_memory_source_lists_added_entries() {
        let mut src = BigInMemoryColorSchemeSource::default();
        src.add(
            BigColorSchemeEntry {
                id: "solarized-dark".into(),
                label: "Solarized Dark".into(),
                source: BigColorSchemeSourceKind::BuiltIn,
                path: None,
            },
            b"{}".to_vec(),
        );
        let list = src.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "solarized-dark");
    }

    #[test]
    fn in_memory_source_load_returns_body() {
        let mut src = BigInMemoryColorSchemeSource::default();
        src.add(
            BigColorSchemeEntry {
                id: "nord".into(),
                label: "Nord".into(),
                source: BigColorSchemeSourceKind::BuiltIn,
                path: None,
            },
            b"fg=#fff",
        );
        assert_eq!(src.load("nord").unwrap(), b"fg=#fff");
        assert!(src.load("missing").is_err());
    }
}
