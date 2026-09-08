// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Recent-file JSONL store shared by desktop apps.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// Single entry inside a recent collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentEntry {
    /// File path (UTF-8 with lossy conversion).
    pub path: String,
    /// Last-access time as a Unix timestamp.
    pub timestamp: u64,
}

/// Recent store value used across BigLinux apps.
#[derive(Debug, Clone)]
pub struct RecentStore {
    state_dir: PathBuf,
    max_entries: usize,
}

impl RecentStore {
    /// Construct a [`RecentStore`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn new(state_dir: impl Into<PathBuf>, max_entries: usize) -> Self {
        Self {
            state_dir: state_dir.into(),
            max_entries: max_entries.max(1),
        }
    }

    /// Return the current `path for kind` value held by this [`RecentStore`].
    #[must_use]
    pub fn path_for_kind(&self, kind: &str) -> PathBuf {
        let filename = if kind.is_empty() {
            "recent.jsonl".to_string()
        } else {
            format!("recent-{kind}.jsonl")
        };
        self.state_dir.join(filename)
    }

    /// Return the current `load` value held by this [`RecentStore`].
    #[must_use]
    pub fn load(&self, kind: &str) -> Vec<RecentEntry> {
        let Ok(content) = fs::read_to_string(self.path_for_kind(kind)) else {
            return Vec::new();
        };
        let mut entries: Vec<RecentEntry> = content.lines().filter_map(parse_recent_line).collect();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.timestamp));
        entries.truncate(self.max_entries);
        entries
    }

    /// Return the current `add` value held by this [`RecentStore`].
    pub fn add(&self, file_path: &str, kind: &str, timestamp: u64) -> Result<(), String> {
        if is_remote_media_url(file_path) {
            return Ok(());
        }

        let canonical = fs::canonicalize(file_path)
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string());

        let mut entries = self.load(kind);
        entries.retain(|entry| entry.path != canonical);
        entries.insert(
            0,
            RecentEntry {
                path: canonical,
                timestamp,
            },
        );
        entries.truncate(self.max_entries);
        self.save(&entries, kind)
    }

    /// Return the current `clear` value held by this [`RecentStore`].
    pub fn clear(&self, kind: &str) -> Result<(), String> {
        let path = self.path_for_kind(kind);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }

    fn save(&self, entries: &[RecentEntry], kind: &str) -> Result<(), String> {
        let path = self.path_for_kind(kind);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let content = entries
            .iter()
            .map(|entry| json!({"path": entry.path, "ts": entry.timestamp}).to_string())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path, content).map_err(|err| err.to_string())
    }
}

/// Return the basename of `path` for use as a recent-file row label,
/// falling back to the raw input when there is no basename.
#[must_use]
pub fn display_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().to_string(),
    )
}

/// Return `true` when is remote media url.
#[must_use]
pub fn is_remote_media_url(path: &str) -> bool {
    path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("rtsp://")
        || path.starts_with("rtmp://")
}

fn parse_recent_line(line: &str) -> Option<RecentEntry> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    Some(RecentEntry {
        path: value.get("path")?.as_str()?.to_string(),
        timestamp: value.get("ts")?.as_u64()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("big-app-kit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn display_name_uses_basename_or_raw_path() {
        assert_eq!(display_name("/home/user/video.mp4"), "video.mp4");
        assert_eq!(display_name("/"), "/");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn store_dedupes_sorts_and_caps_entries() {
        let dir = temp_state_dir("recent-store");
        let store = RecentStore::new(&dir, 2);
        let first = dir.join("first.mp3");
        let second = dir.join("second.mp3");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();

        store.add(first.to_str().unwrap(), "audio", 1).unwrap();
        store.add(second.to_str().unwrap(), "audio", 2).unwrap();
        store.add(first.to_str().unwrap(), "audio", 3).unwrap();

        let entries = store.load("audio");
        assert_eq!(entries.len(), 2);
        assert!(entries[0].path.ends_with("first.mp3"));
        assert_eq!(entries[0].timestamp, 3);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn clear_removes_existing_recent_file() {
        let state_directory = temp_state_dir("recent-clear-existing");
        let store = RecentStore::new(&state_directory, 5);
        let media_file_path = state_directory.join("track.mp3");
        fs::write(&media_file_path, "audio").unwrap();

        store
            .add(media_file_path.to_str().unwrap(), "audio", 10)
            .unwrap();
        assert!(!store.load("audio").is_empty());

        store.clear("audio").unwrap();

        assert!(store.load("audio").is_empty());
        assert!(!store.path_for_kind("audio").exists());

        let _ = fs::remove_dir_all(state_directory);
    }

    #[test]
    fn clear_missing_recent_file_is_idempotent() {
        let state_directory = temp_state_dir("recent-clear-missing");
        let store = RecentStore::new(&state_directory, 5);

        store.clear("audio").unwrap();

        let _ = fs::remove_dir_all(state_directory);
    }

    #[test]
    fn clear_reports_real_remove_errors() {
        let state_directory = temp_state_dir("recent-clear-error");
        let store = RecentStore::new(&state_directory, 5);
        let recent_file_path = store.path_for_kind("audio");
        fs::create_dir_all(&recent_file_path).unwrap();

        assert!(store.clear("audio").is_err());

        let _ = fs::remove_dir_all(state_directory);
    }

    #[test]
    fn remote_urls_are_not_saved() {
        let dir = temp_state_dir("recent-remote");
        let store = RecentStore::new(&dir, 5);
        store
            .add("https://example.com/video.mp4", "video", 1)
            .unwrap();
        assert!(store.load("video").is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn remote_media_url_detection_covers_streaming_schemes() {
        assert!(is_remote_media_url("http://example.com/track.mp3"));
        assert!(is_remote_media_url("https://example.com/video.mp4"));
        assert!(is_remote_media_url("rtsp://camera.local/live"));
        assert!(is_remote_media_url("rtmp://stream.example/live"));
        assert!(!is_remote_media_url("/home/user/video.mp4"));
    }
}
