// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! File picker, drag-and-drop, and file collection contracts.

use std::path::{Path, PathBuf};

/// Sort of thing referenced by a file picker, drop target, or
/// collection row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigFileKind {
    /// Regular file on the local filesystem.
    File,
    /// Directory on the local filesystem.
    Folder,
    /// Remote resource addressable by URI (HTTP, FTP, ...).
    Url,
}

/// One row in a file collection. Holds enough context to render the row
/// without re-statting the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFileEntry {
    /// Canonical URI (`file://...` for local entries).
    pub uri: String,
    /// Label to show in lists; usually the basename.
    pub display_name: String,
    /// What `uri` points at.
    pub kind: BigFileKind,
    /// Local filesystem path when applicable. `None` for remote URIs.
    pub path: Option<PathBuf>,
    /// File size in bytes when known.
    pub size_bytes: Option<u64>,
}

impl BigFileEntry {
    /// Build a local-file entry from a path. `display_name` is the
    /// basename when available, `"file"` otherwise; `uri` is derived
    /// as `file://<path>`.
    #[must_use]
    pub fn file(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let display_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_string();
        Self {
            uri: format!("file://{}", path.display()),
            display_name,
            kind: BigFileKind::File,
            path: Some(path),
            size_bytes: None,
        }
    }
}

/// Behaviour selected for a [`BigFilePickerSpec`]; drives which portal
/// method the adapter calls and which response fields it expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigPickerMode {
    /// Pick one or more existing files to open.
    OpenFile,
    /// Pick an existing folder to open.
    OpenFolder,
    /// Pick a filesystem location for a new file (save dialog).
    SaveFile,
    /// Pick an existing folder as the destination for output files.
    DestinationFolder,
}

/// Typed file picker spec. Drives portal- or libadwaita-backed file dialogs.
///
/// # Capabilities
///
/// `file-picker`, `file-picker+portal`
///
/// # Archetypes
///
/// `media-converter`, `editor`, `file-manager`, `screenshot`, `clipboard`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFilePickerSpec {
    /// Behaviour selected for this picker.
    pub mode: BigPickerMode,
    /// Localized dialog title shown by the portal.
    pub title: String,
    /// `true` to allow multi-selection (only honoured in
    /// `OpenFile`/`OpenFolder` modes).
    pub multiple: bool,
    /// Glob patterns (e.g. `"*.mp4"`) the dialog should filter by.
    pub patterns: Vec<String>,
    /// `true` to require the desktop portal backend; `false` lets the
    /// adapter fall back to a libadwaita-only dialog in test environments.
    pub portal_required: bool,
}

impl BigFilePickerSpec {
    /// Build a picker spec. Portal use is required by default; patterns
    /// and multi-select start empty.
    #[must_use]
    pub fn new(mode: BigPickerMode, title: impl Into<String>) -> Self {
        Self {
            mode,
            title: title.into(),
            multiple: false,
            patterns: Vec::new(),
            portal_required: true,
        }
    }

    /// Shortcut for an "Open file" toolbar button picker.
    #[must_use]
    pub fn open_file_button(title: impl Into<String>) -> Self {
        Self::new(BigPickerMode::OpenFile, title)
    }

    /// Shortcut for an "Open folder" picker.
    #[must_use]
    pub fn open_folder_button(title: impl Into<String>) -> Self {
        Self::new(BigPickerMode::OpenFolder, title)
    }

    /// Shortcut for a "Save as" picker.
    #[must_use]
    pub fn save_file_button(title: impl Into<String>) -> Self {
        Self::new(BigPickerMode::SaveFile, title)
    }

    /// Shortcut for a "Destination folder" row in conversion flows.
    #[must_use]
    pub fn destination_folder_row(title: impl Into<String>) -> Self {
        Self::new(BigPickerMode::DestinationFolder, title)
    }

    /// Enable or disable multi-selection. Only meaningful in
    /// `OpenFile`/`OpenFolder` modes.
    #[must_use]
    pub fn multiple(mut self, enabled: bool) -> Self {
        self.multiple = enabled;
        self
    }

    /// Append a glob pattern to the dialog's filter list.
    #[must_use]
    pub fn pattern(mut self, pattern: impl Into<String>) -> Self {
        self.patterns.push(pattern.into());
        self
    }

    /// Project to a [`BigFilePickerResolved`] that exposes the boolean
    /// affordances the adapter cares about (accepts files, accepts
    /// folders, produces an output path, multi-select, patterns).
    #[must_use]
    pub fn resolved(&self) -> BigFilePickerResolved {
        BigFilePickerResolved {
            accepts_files: matches!(self.mode, BigPickerMode::OpenFile | BigPickerMode::SaveFile),
            accepts_folders: matches!(
                self.mode,
                BigPickerMode::OpenFolder | BigPickerMode::DestinationFolder
            ),
            produces_output_path: matches!(
                self.mode,
                BigPickerMode::SaveFile | BigPickerMode::DestinationFolder
            ),
            multiple: self.multiple,
            patterns: self.patterns.clone(),
        }
    }
}

/// Adapter-facing flags computed from a [`BigFilePickerSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFilePickerResolved {
    /// `true` when the dialog should let the user select files.
    pub accepts_files: bool,
    /// `true` when the dialog should let the user select folders.
    pub accepts_folders: bool,
    /// `true` for save-style modes that produce a new path the app
    /// must write to.
    pub produces_output_path: bool,
    /// Copy of [`BigFilePickerSpec::multiple`].
    pub multiple: bool,
    /// Copy of [`BigFilePickerSpec::patterns`].
    pub patterns: Vec<String>,
}

/// Describes what a drop target on the window will accept.
///
/// Apps construct one [`BigDropTargetSpec`] per region (main canvas,
/// sidebar, queue) and feed it into the GTK drop adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDropTargetSpec {
    /// `true` when local files dropped on the region are accepted.
    pub accepts_files: bool,
    /// `true` when dropped folders are accepted.
    pub accepts_folders: bool,
    /// `true` when text URIs (drag from a browser) are accepted.
    pub accepts_urls: bool,
    /// Maximum number of items in a single drop; `None` means
    /// unlimited.
    pub max_items: Option<usize>,
}

impl Default for BigDropTargetSpec {
    fn default() -> Self {
        Self {
            accepts_files: true,
            accepts_folders: false,
            accepts_urls: false,
            max_items: None,
        }
    }
}

impl BigDropTargetSpec {
    /// Toggle folder acceptance.
    #[must_use]
    pub fn folders(mut self, enabled: bool) -> Self {
        self.accepts_folders = enabled;
        self
    }

    /// Toggle URL acceptance.
    #[must_use]
    pub fn urls(mut self, enabled: bool) -> Self {
        self.accepts_urls = enabled;
        self
    }

    /// Cap the number of items per drop.
    #[must_use]
    pub fn max_items(mut self, value: usize) -> Self {
        self.max_items = Some(value);
        self
    }

    /// `true` when the spec accepts items of the requested kind. Used
    /// by the drop adapter on each enter/motion event.
    #[must_use]
    pub fn accepts(&self, kind: BigFileKind) -> bool {
        match kind {
            BigFileKind::File => self.accepts_files,
            BigFileKind::Folder => self.accepts_folders,
            BigFileKind::Url => self.accepts_urls,
        }
    }
}

/// File pipeline spec — source picker → optional transform → destination + safety policy.
///
/// # Capabilities
///
/// `file-workflow`, `file-workflow+overwrite-policy`
///
/// # Archetypes
///
/// `media-converter`, `file-manager`, `screenshot`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFileWorkflowSpec {
    /// File picker used as the entry point of the workflow.
    pub picker: BigFilePickerSpec,
    /// Drop target accepted as an alternate input source.
    pub drop_target: BigDropTargetSpec,
    /// Maximum number of recent-file entries to remember.
    pub recent_limit: usize,
}

impl BigFileWorkflowSpec {
    /// Shortcut for the common "open multiple files" workflow:
    /// multi-select picker, default drop target, 10 recent entries.
    #[must_use]
    pub fn open_files(title: impl Into<String>) -> Self {
        Self {
            picker: BigFilePickerSpec::new(BigPickerMode::OpenFile, title).multiple(true),
            drop_target: BigDropTargetSpec::default(),
            recent_limit: 10,
        }
    }
}

/// Turn arbitrary display text or an URL into a bounded filename stem.
#[must_use]
pub fn safe_filename_stem(input: &str, max_len: usize, fallback: &str) -> String {
    let mut out = String::with_capacity(input.len().min(max_len));
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if "-_.".contains(ch) {
            out.push(ch);
        } else {
            out.push('_');
        }
        if out.len() >= max_len {
            out.truncate(max_len);
            break;
        }
    }
    if out.is_empty() {
        out.push_str(fallback);
        out.truncate(max_len);
    }
    out
}

/// Sanitize a user-facing filename stem while preserving Unicode letters/numbers.
#[must_use]
pub fn sanitize_filename_stem_preserve_unicode(input: &str, fallback: &str) -> String {
    let safe = input
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character == ' ' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .replace(' ', "_");
    if safe.is_empty() {
        fallback.to_string()
    } else {
        safe
    }
}

/// Return the file stem of `path` (basename without final extension);
/// falls back to the raw input when no stem is recoverable.
#[must_use]
pub fn file_stem_display_name(path: &str) -> String {
    Path::new(path).file_stem().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().to_string(),
    )
}

/// Return the basename of `path` for use as a folder label; falls back
/// to the raw input for roots like `"/"`.
#[must_use]
pub fn folder_display_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().to_string(),
    )
}

/// Policy for the "Recent files" submenu/sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigRecentFilesSpec {
    /// Maximum number of entries to keep.
    pub limit: usize,
    /// `true` to keep listing files whose path no longer exists; the
    /// UI typically dims them.
    pub include_missing: bool,
}

impl Default for BigRecentFilesSpec {
    fn default() -> Self {
        Self {
            limit: 10,
            include_missing: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_picker_produces_output_path() {
        let resolved = BigFilePickerSpec::new(BigPickerMode::SaveFile, "Save").resolved();
        assert!(resolved.accepts_files);
        assert!(resolved.produces_output_path);
        assert!(!resolved.accepts_folders);
    }

    #[test]
    fn folder_picker_accepts_only_folders() {
        let resolved = BigFilePickerSpec::open_folder_button("Open folder").resolved();
        assert!(!resolved.accepts_files);
        assert!(resolved.accepts_folders);
    }

    #[test]
    fn drop_target_contract_separates_files_folders_and_urls() {
        let spec = BigDropTargetSpec::default().folders(true).urls(false);
        assert!(spec.accepts(BigFileKind::File));
        assert!(spec.accepts(BigFileKind::Folder));
        assert!(!spec.accepts(BigFileKind::Url));
    }

    #[test]
    fn drop_target_limit_records_maximum_dropped_paths() {
        let spec = BigDropTargetSpec::default().max_items(3);

        assert_eq!(spec.max_items, Some(3));
        assert!(spec.accepts(BigFileKind::File));
    }

    #[test]
    fn safe_filename_stem_is_lowercase_bounded_and_nonempty() {
        assert_eq!(safe_filename_stem("My IPTV", 80, "unnamed"), "my_iptv");
        assert_eq!(
            safe_filename_stem("http://a.example/x.m3u", 80, "unnamed"),
            "http___a.example_x.m3u"
        );
        assert_eq!(safe_filename_stem("", 80, "unnamed"), "unnamed");
        assert_eq!(
            safe_filename_stem(&"a".repeat(200), 80, "unnamed").len(),
            80
        );
    }

    #[test]
    fn unicode_filename_stem_preserves_letters_and_replaces_punctuation() {
        assert_eq!(
            sanitize_filename_stem_preserve_unicode("Rádio Brasil", "radio"),
            "Rádio_Brasil"
        );
        assert_eq!(
            sanitize_filename_stem_preserve_unicode("Rock & Roll!", "radio"),
            "Rock___Roll_"
        );
        assert_eq!(
            sanitize_filename_stem_preserve_unicode("  Radio  ", "radio"),
            "Radio"
        );
        assert_eq!(
            sanitize_filename_stem_preserve_unicode("", "radio"),
            "radio"
        );
    }

    #[test]
    fn file_stem_display_name_strips_path_and_extension() {
        assert_eq!(file_stem_display_name("/home/user/video.mp4"), "video");
        assert_eq!(file_stem_display_name("video.mp4"), "video");
        assert_eq!(file_stem_display_name("/home/user/folder"), "folder");
    }

    #[test]
    fn folder_display_name_uses_basename_or_original_path() {
        assert_eq!(folder_display_name("/home/user/Videos"), "Videos");
        assert_eq!(folder_display_name("/"), "/");
        assert_eq!(folder_display_name("myfolder"), "myfolder");
    }
}
