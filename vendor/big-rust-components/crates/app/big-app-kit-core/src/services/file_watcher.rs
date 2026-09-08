// SPDX-License-Identifier: MIT

//! File-watcher spec with debounce.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What happened to a watched filesystem entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BigFileEventKind {
    /// Entry appeared.
    Created,
    /// Entry contents or metadata changed.
    Modified,
    /// Entry was removed.
    Deleted,
    /// Entry was renamed; the new path is on the carrying
    /// [`BigFileEvent`], the previous path is captured here.
    Renamed {
        /// Previous path before the rename.
        from: PathBuf,
    },
}

/// One delivered file-watcher event, ready for the host's consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFileEvent {
    /// Path the event applies to (post-rename for `Renamed`).
    pub path: PathBuf,
    /// What happened.
    pub kind: BigFileEventKind,
}

/// Display-free specification describing big file watch behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFileWatchSpec {
    /// Directories or files to monitor.
    pub roots: Vec<PathBuf>,
    /// `true` to recurse into subdirectories.
    pub recursive: bool,
    /// Debounce window. Events fired within this window are coalesced
    /// per `(path, kind)` pair before delivery.
    pub debounce: Duration,
    /// Glob-style patterns to ignore (e.g. `*.tmp`, `target/`).
    pub ignore_patterns: Vec<String>,
}

impl BigFileWatchSpec {
    /// Build a [`BigFileWatchSpec`] populated from the supplied root paths.
    #[must_use]
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            recursive: true,
            debounce: Duration::from_millis(250),
            ignore_patterns: Vec::new(),
        }
    }

    /// Builder method returning `Self` with debounce set.
    #[must_use]
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    /// Disable recursion into subdirectories.
    #[must_use]
    pub fn non_recursive(mut self) -> Self {
        self.recursive = false;
        self
    }

    /// Append `pattern` (simple glob) to the ignore list.
    #[must_use]
    pub fn ignore(mut self, pattern: impl Into<String>) -> Self {
        self.ignore_patterns.push(pattern.into());
        self
    }

    /// Returns `true` when `path` matches any ignore pattern. Uses
    /// simple substring matching; production code may swap in `globset`.
    #[must_use]
    pub fn is_ignored(&self, path: &Path) -> bool {
        let s = path.to_string_lossy();
        self.ignore_patterns
            .iter()
            .any(|pat| matches_simple_glob(pat, &s))
    }
}

fn matches_simple_glob(pattern: &str, text: &str) -> bool {
    if let Some(stripped) = pattern.strip_prefix('*') {
        return text.ends_with(stripped);
    }
    if let Some(stripped) = pattern.strip_suffix('*') {
        return text.starts_with(stripped);
    }
    text.contains(pattern)
}

/// In-memory event coalescer used by tests and the production worker.
#[derive(Debug)]
pub struct BigFileEventCoalescer {
    debounce: Duration,
    last_emit: HashMap<(PathBuf, &'static str), Instant>,
}

impl BigFileEventCoalescer {
    /// Build a [`BigFileEventCoalescer`] with the supplied debounce window.
    #[must_use]
    pub fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            last_emit: HashMap::new(),
        }
    }

    /// `true` when the event should reach the consumer; `false` when
    /// it was coalesced by an earlier identical event.
    pub fn admit_at(&mut self, event: &BigFileEvent, now: Instant) -> bool {
        let key = (event.path.clone(), kind_token(&event.kind));
        if let Some(last) = self.last_emit.get(&key)
            && now.duration_since(*last) < self.debounce
        {
            return false;
        }
        self.last_emit.insert(key, now);
        true
    }

    /// Report whether the `admit` condition currently holds.
    pub fn admit(&mut self, event: &BigFileEvent) -> bool {
        self.admit_at(event, Instant::now())
    }
}

fn kind_token(kind: &BigFileEventKind) -> &'static str {
    match kind {
        BigFileEventKind::Created => "created",
        BigFileEventKind::Modified => "modified",
        BigFileEventKind::Deleted => "deleted",
        BigFileEventKind::Renamed { .. } => "renamed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(path: &str, kind: BigFileEventKind) -> BigFileEvent {
        BigFileEvent {
            path: PathBuf::from(path),
            kind,
        }
    }

    #[test]
    fn default_spec_recurses_with_250ms_debounce() {
        let s = BigFileWatchSpec::new(vec!["/home/u/Music".into()]);
        assert!(s.recursive);
        assert_eq!(s.debounce, Duration::from_millis(250));
    }

    #[test]
    fn ignore_pattern_prefix_glob() {
        let s = BigFileWatchSpec::new(vec!["/x".into()]).ignore("target");
        assert!(s.is_ignored(Path::new("/x/target/build")));
    }

    #[test]
    fn ignore_pattern_suffix_glob() {
        let s = BigFileWatchSpec::new(vec!["/x".into()]).ignore("*.tmp");
        assert!(s.is_ignored(Path::new("/x/file.tmp")));
        assert!(!s.is_ignored(Path::new("/x/file.txt")));
    }

    #[test]
    fn coalescer_drops_repeat_event_inside_debounce() {
        let mut c = BigFileEventCoalescer::new(Duration::from_millis(250));
        let t = Instant::now();
        assert!(c.admit_at(&event("/x", BigFileEventKind::Modified), t));
        assert!(!c.admit_at(&event("/x", BigFileEventKind::Modified), t));
    }

    #[test]
    fn coalescer_recovers_after_window() {
        let mut c = BigFileEventCoalescer::new(Duration::from_millis(10));
        let t = Instant::now();
        assert!(c.admit_at(&event("/x", BigFileEventKind::Modified), t));
        let t2 = t + Duration::from_millis(100);
        assert!(c.admit_at(&event("/x", BigFileEventKind::Modified), t2));
    }

    #[test]
    fn coalescer_recovers_at_exact_debounce_boundary() {
        let mut c = BigFileEventCoalescer::new(Duration::from_millis(250));
        let t = Instant::now();
        assert!(c.admit_at(&event("/x", BigFileEventKind::Modified), t));
        let t2 = t + Duration::from_millis(250);
        assert!(c.admit_at(&event("/x", BigFileEventKind::Modified), t2));
    }

    #[test]
    fn coalescer_admit_uses_current_time_and_stored_state() {
        let mut c = BigFileEventCoalescer::new(Duration::from_secs(60));
        assert!(c.admit(&event("/x", BigFileEventKind::Modified)));
        assert!(!c.admit(&event("/x", BigFileEventKind::Modified)));
    }

    #[test]
    fn coalescer_treats_different_kinds_separately() {
        let mut c = BigFileEventCoalescer::new(Duration::from_millis(250));
        let t = Instant::now();
        assert!(c.admit_at(&event("/x", BigFileEventKind::Modified), t));
        assert!(c.admit_at(&event("/x", BigFileEventKind::Deleted), t));
    }
}
