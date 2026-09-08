// SPDX-License-Identifier: MIT

//! File browser sub-specs.
//!
//! Data carriers for the file-manager class of apps. Each sub-spec is
//! display-free and serde-friendly; the widget builder layer assembles
//! them into a `GtkListView` / `GtkGridView` plus breadcrumb header.

use std::path::{Component, Path, PathBuf};

/// Layout flavour the file browser uses to render its current
/// directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigFileViewMode {
    /// One-line-per-entry list view.
    List,
    /// Thumbnail grid.
    Grid,
    /// Multi-pane column (NeXT/Finder-style) view.
    Columns,
}

/// Column the entries are ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigFileSortKey {
    /// Lexicographic name sort.
    Name,
    /// Numeric size sort; missing sizes treated as zero.
    Size,
    /// Modification timestamp sort.
    Modified,
    /// Group by file kind (directories first, then regular files,
    /// symlinks, other).
    Kind,
}

/// Direction the [`BigFileSortKey`] is applied in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigFileSortOrder {
    /// A→Z / 0→9.
    Ascending,
    /// Z→A / 9→0.
    Descending,
}

/// State of the breadcrumb navigation bar: a root label plus the
/// stack of components representing the current directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigBreadcrumbBarSpec {
    /// Label for the root row (`"Home"`, `"/"`, ...).
    pub root_label: String,
    /// Path components from root down to the current directory.
    pub segments: Vec<BigBreadcrumbSegment>,
}

/// One step of the breadcrumb stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigBreadcrumbSegment {
    /// Display label for this segment (usually the directory name).
    pub label: String,
    /// Fully-qualified path the user lands on when clicking this
    /// segment.
    pub absolute_path: PathBuf,
}

impl BigBreadcrumbBarSpec {
    /// Build a breadcrumb from an absolute path. `root_label` becomes
    /// the leftmost segment (e.g. `"Home"` or `"/"`).
    ///
    /// # Errors
    /// Returns an error description when `path` is empty or relative.
    pub fn from_absolute(root_label: impl Into<String>, path: &Path) -> Result<Self, String> {
        if path.as_os_str().is_empty() {
            return Err("empty path".into());
        }
        if !path.is_absolute() {
            return Err("path must be absolute".into());
        }
        let mut segments = Vec::new();
        let mut acc = PathBuf::new();
        for comp in path.components() {
            if let Component::Normal(name) = comp {
                acc.push(name);
                segments.push(BigBreadcrumbSegment {
                    label: name.to_string_lossy().into_owned(),
                    absolute_path: PathBuf::from("/").join(&acc),
                });
            }
        }
        Ok(Self {
            root_label: root_label.into(),
            segments,
        })
    }

    /// Return the deepest segment, i.e. the current directory. `None`
    /// when the breadcrumb still points at the root.
    #[must_use]
    pub fn deepest(&self) -> Option<&BigBreadcrumbSegment> {
        self.segments.last()
    }
}

/// One row of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFileEntry {
    /// Basename used as display label.
    pub name: String,
    /// Canonical absolute path.
    pub absolute_path: PathBuf,
    /// What this entry is (directory, regular file, symlink, other).
    pub kind: BigFileKind,
    /// Size in bytes when known.
    pub size_bytes: Option<u64>,
    /// Modification time as a Unix timestamp when known.
    pub modified_unix: Option<i64>,
}

/// Coarse file-system classification used for sorting and icon
/// selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigFileKind {
    /// Directory.
    Directory,
    /// Regular file.
    Regular,
    /// Symbolic link (regardless of target kind).
    Symlink,
    /// Device, FIFO, socket, or anything else that does not fit the
    /// other variants.
    Other,
}

/// Display-free state of the file list/grid view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFileListViewSpec {
    /// Layout flavour.
    pub mode: BigFileViewMode,
    /// Active sort column.
    pub sort_key: BigFileSortKey,
    /// Active sort direction.
    pub sort_order: BigFileSortOrder,
    /// `true` to keep dotfiles visible.
    pub show_hidden: bool,
    /// Paths the user has selected, in selection order.
    pub selected_paths: Vec<PathBuf>,
}

impl Default for BigFileListViewSpec {
    fn default() -> Self {
        Self {
            mode: BigFileViewMode::List,
            sort_key: BigFileSortKey::Name,
            sort_order: BigFileSortOrder::Ascending,
            show_hidden: false,
            selected_paths: Vec::new(),
        }
    }
}

impl BigFileListViewSpec {
    /// Sort entries according to the current key + order. Hidden files
    /// (leading `.`) are filtered out when `show_hidden` is `false`.
    #[must_use]
    pub fn arrange(&self, entries: &[BigFileEntry]) -> Vec<BigFileEntry> {
        let mut filtered: Vec<BigFileEntry> = entries
            .iter()
            .filter(|e| self.show_hidden || !e.name.starts_with('.'))
            .cloned()
            .collect();
        match self.sort_key {
            BigFileSortKey::Name => filtered.sort_by(|a, b| a.name.cmp(&b.name)),
            BigFileSortKey::Size => filtered.sort_by_key(|e| e.size_bytes.unwrap_or(0)),
            BigFileSortKey::Modified => filtered.sort_by_key(|e| e.modified_unix.unwrap_or(0)),
            BigFileSortKey::Kind => filtered.sort_by_key(|e| file_kind_priority(e.kind)),
        }
        if matches!(self.sort_order, BigFileSortOrder::Descending) {
            filtered.reverse();
        }
        filtered
    }
}

fn file_kind_priority(kind: BigFileKind) -> u8 {
    match kind {
        BigFileKind::Directory => 0,
        BigFileKind::Regular => 1,
        BigFileKind::Symlink => 2,
        BigFileKind::Other => 3,
    }
}

/// Metadata pane data backing a "Properties" dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFilePropertiesSpec {
    /// Subject entry the dialog describes.
    pub entry: BigFileEntry,
    /// Detected MIME type when known (`"text/plain"`, ...).
    pub mime_type: Option<String>,
    /// Owning user as a readable name when resolved.
    pub owner: Option<String>,
    /// Owning group as a readable name when resolved.
    pub group: Option<String>,
    /// Permission bits formatted as a four-digit octal string.
    pub permissions_octal: Option<String>,
}

impl BigFilePropertiesSpec {
    /// Build a properties spec with only the entry filled in; the
    /// host fills the optional metadata fields asynchronously.
    #[must_use]
    pub fn new(entry: BigFileEntry) -> Self {
        Self {
            entry,
            mime_type: None,
            owner: None,
            group: None,
            permissions_octal: None,
        }
    }
}

/// What the file transfer is supposed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigFileTransferKind {
    /// Copy sources to destination, leaving sources in place.
    Copy,
    /// Move sources to destination, removing them from the origin.
    Move,
    /// Send sources to the trash (recoverable).
    Trash,
    /// Permanently delete sources.
    Delete,
}

/// Display-free description of one file-manager transfer operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFileTransferSpec {
    /// What kind of transfer this is.
    pub kind: BigFileTransferKind,
    /// Source paths involved in the operation.
    pub source_paths: Vec<PathBuf>,
    /// Destination directory for copy/move; must be `None` for
    /// `Trash` and `Delete`.
    pub destination: Option<PathBuf>,
    /// Behaviour when a destination entry with the same name already
    /// exists.
    pub overwrite: BigOverwritePolicy,
}

/// Resolution strategy for destination collisions during a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigOverwritePolicy {
    /// Ask the user; the transfer pauses until they answer.
    Prompt,
    /// Skip colliding entries silently.
    Skip,
    /// Overwrite destination entries.
    Overwrite,
    /// Append a numeric suffix to keep both copies.
    KeepBoth,
}

impl BigFileTransferSpec {
    /// Validate the transfer: non-empty sources, destination required
    /// for copy/move, and `Delete`/`Trash` reject a destination.
    ///
    /// # Errors
    /// Returns a description string when the spec is internally
    /// inconsistent.
    pub fn validate(&self) -> Result<(), String> {
        if self.source_paths.is_empty() {
            return Err("transfer has no source paths".into());
        }
        match self.kind {
            BigFileTransferKind::Copy | BigFileTransferKind::Move => {
                if self.destination.is_none() {
                    return Err(format!("{:?} requires a destination", self.kind));
                }
            }
            BigFileTransferKind::Trash | BigFileTransferKind::Delete => {
                if self.destination.is_some() {
                    return Err(format!("{:?} must not have a destination", self.kind));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn breadcrumb_builds_from_absolute_path() {
        let bc =
            BigBreadcrumbBarSpec::from_absolute("Home", Path::new("/home/user/Documents/Reports"))
                .unwrap();
        assert_eq!(bc.segments.len(), 4);
        assert_eq!(bc.deepest().unwrap().label, "Reports");
    }

    #[test]
    fn breadcrumb_rejects_relative_path() {
        assert!(BigBreadcrumbBarSpec::from_absolute("H", Path::new("relative/x")).is_err());
    }

    #[test]
    fn listview_filters_hidden_by_default() {
        let entries = vec![
            BigFileEntry {
                name: "visible.txt".into(),
                absolute_path: "/v".into(),
                kind: BigFileKind::Regular,
                size_bytes: Some(100),
                modified_unix: Some(1),
            },
            BigFileEntry {
                name: ".hidden".into(),
                absolute_path: "/.h".into(),
                kind: BigFileKind::Regular,
                size_bytes: Some(10),
                modified_unix: Some(2),
            },
        ];
        let view = BigFileListViewSpec::default();
        let arranged = view.arrange(&entries);
        assert_eq!(arranged.len(), 1);
        assert_eq!(arranged[0].name, "visible.txt");
    }

    #[test]
    fn listview_show_hidden_includes_dotfiles() {
        let entries = vec![BigFileEntry {
            name: ".hidden".into(),
            absolute_path: "/.h".into(),
            kind: BigFileKind::Regular,
            size_bytes: None,
            modified_unix: None,
        }];
        let view = BigFileListViewSpec {
            show_hidden: true,
            ..BigFileListViewSpec::default()
        };
        assert_eq!(view.arrange(&entries).len(), 1);
    }

    #[test]
    fn listview_sort_size_descending() {
        let entries = vec![
            BigFileEntry {
                name: "small".into(),
                absolute_path: "/s".into(),
                kind: BigFileKind::Regular,
                size_bytes: Some(10),
                modified_unix: None,
            },
            BigFileEntry {
                name: "big".into(),
                absolute_path: "/b".into(),
                kind: BigFileKind::Regular,
                size_bytes: Some(1000),
                modified_unix: None,
            },
        ];
        let view = BigFileListViewSpec {
            sort_key: BigFileSortKey::Size,
            sort_order: BigFileSortOrder::Descending,
            ..BigFileListViewSpec::default()
        };
        let arranged = view.arrange(&entries);
        assert_eq!(arranged[0].name, "big");
    }

    #[test]
    fn listview_sort_kind_orders_directories_first() {
        let entries = vec![
            BigFileEntry {
                name: "z-file".into(),
                absolute_path: "/zf".into(),
                kind: BigFileKind::Regular,
                size_bytes: None,
                modified_unix: None,
            },
            BigFileEntry {
                name: "a-dir".into(),
                absolute_path: "/ad".into(),
                kind: BigFileKind::Directory,
                size_bytes: None,
                modified_unix: None,
            },
        ];
        let view = BigFileListViewSpec {
            sort_key: BigFileSortKey::Kind,
            ..BigFileListViewSpec::default()
        };
        let arranged = view.arrange(&entries);
        assert_eq!(arranged[0].kind, BigFileKind::Directory);
    }

    #[test]
    fn transfer_copy_requires_destination() {
        let spec = BigFileTransferSpec {
            kind: BigFileTransferKind::Copy,
            source_paths: vec!["/a".into()],
            destination: None,
            overwrite: BigOverwritePolicy::Prompt,
        };
        assert!(spec.validate().is_err());
    }

    #[test]
    fn transfer_delete_rejects_destination() {
        let spec = BigFileTransferSpec {
            kind: BigFileTransferKind::Delete,
            source_paths: vec!["/a".into()],
            destination: Some("/b".into()),
            overwrite: BigOverwritePolicy::Prompt,
        };
        assert!(spec.validate().is_err());
    }

    #[test]
    fn transfer_empty_sources_rejected() {
        let spec = BigFileTransferSpec {
            kind: BigFileTransferKind::Copy,
            source_paths: vec![],
            destination: Some("/b".into()),
            overwrite: BigOverwritePolicy::Prompt,
        };
        assert!(spec.validate().is_err());
    }

    #[test]
    fn transfer_valid_copy_passes() {
        let spec = BigFileTransferSpec {
            kind: BigFileTransferKind::Copy,
            source_paths: vec!["/a".into()],
            destination: Some("/b".into()),
            overwrite: BigOverwritePolicy::Skip,
        };
        spec.validate().unwrap();
    }
}
