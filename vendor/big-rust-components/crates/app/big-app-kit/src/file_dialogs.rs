// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! GTK file dialog wrapper for BigLinux desktop apps.

use std::path::{Path, PathBuf};

use gtk::gio;
use gtk::prelude::*;

use crate::files::{BigFilePickerSpec, BigPickerMode};

/// File picker value used across BigLinux apps.
#[must_use = "FilePicker is a builder; call open, save, or select_folder"]
pub struct FilePicker {
    spec: BigFilePickerSpec,
    accept_label: Option<String>,
    modal: bool,
    initial_folder: Option<PathBuf>,
    initial_gio_folder: Option<gio::File>,
    initial_name: Option<String>,
    filters: Vec<gtk::FileFilter>,
}

impl FilePicker {
    /// Construct a [`FilePicker`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    pub fn new(title: impl Into<String>) -> Self {
        Self::for_spec(BigFilePickerSpec::new(BigPickerMode::OpenFile, title))
    }

    /// Construct a [`FilePicker`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    pub fn open_files(title: impl Into<String>) -> Self {
        Self::for_spec(BigFilePickerSpec::open_file_button(title).multiple(true))
    }

    /// Construct a [`FilePicker`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    pub fn open_folder(title: impl Into<String>) -> Self {
        Self::for_spec(BigFilePickerSpec::open_folder_button(title))
    }

    /// Construct a [`FilePicker`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    pub fn destination_folder(title: impl Into<String>) -> Self {
        Self::for_spec(BigFilePickerSpec::destination_folder_row(title))
    }

    /// Construct a [`FilePicker`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    pub fn save_file(title: impl Into<String>) -> Self {
        Self::for_spec(BigFilePickerSpec::save_file_button(title))
    }

    /// Construct a [`FilePicker`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    pub fn for_spec(spec: BigFilePickerSpec) -> Self {
        Self {
            spec,
            accept_label: None,
            modal: true,
            initial_folder: None,
            initial_gio_folder: None,
            initial_name: None,
            filters: Vec::new(),
        }
    }

    /// Return a reference to the `spec` exposed by this [`FilePicker`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn spec(&self) -> &BigFilePickerSpec {
        &self.spec
    }

    /// Configure the `accept_label` setting and return the updated builder.
    ///
    /// The supplied `label` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`FilePicker`].
    pub fn accept_label(mut self, label: impl Into<String>) -> Self {
        self.accept_label = Some(label.into());
        self
    }

    /// Configure the `initial_folder` setting and return the updated builder.
    ///
    /// The supplied `path` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`FilePicker`].
    pub fn initial_folder(mut self, path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        if !path.as_os_str().is_empty() {
            self.initial_folder = Some(path.to_path_buf());
            self.initial_gio_folder = None;
        }
        self
    }

    /// Configure the initial folder from a `gio::File` so callers can preserve
    /// URI-backed folders such as `smb://`, `sftp://`, `ftp://`, or `ssh://`.
    pub fn initial_gio_folder(mut self, folder: gio::File) -> Self {
        self.initial_gio_folder = Some(folder);
        self.initial_folder = None;
        self
    }

    /// Configure the `initial_name` setting and return the updated builder.
    ///
    /// The supplied `name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`FilePicker`].
    pub fn initial_name(mut self, name: impl Into<String>) -> Self {
        self.initial_name = Some(name.into());
        self
    }

    /// Sets the initial folder to `$HOME/.ssh` when it exists, falling back
    /// to `$HOME`. Leaves the initial folder untouched if `$HOME` is not set.
    ///
    /// Convenience for SSH-aware dialogs (key pickers, known_hosts browsing,
    /// ssh-config selection) that should land users in the right place
    /// without each app re-implementing the same `~/.ssh` probe.
    pub fn initial_ssh_folder(self) -> Self {
        match resolve_ssh_initial_folder() {
            Some(path) => self.initial_folder(path),
            None => self,
        }
    }

    /// Configure the `filter` setting and return the updated builder.
    ///
    /// The supplied `name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`FilePicker`].
    pub fn filter(mut self, name: &str, suffixes: &[&str]) -> Self {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(name));
        for suffix in suffixes {
            filter.add_suffix(suffix);
        }
        self.filters.push(filter);
        self
    }

    /// Configure the `mime_filter` setting and return the updated builder.
    ///
    /// The supplied `name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`FilePicker`].
    pub fn mime_filter(mut self, name: &str, mime_types: &[&str]) -> Self {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(name));
        for mime in mime_types {
            filter.add_mime_type(mime);
        }
        self.filters.push(filter);
        self
    }

    /// Configure the `pattern_filter` setting and return the updated builder.
    ///
    /// The supplied `name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`FilePicker`].
    pub fn pattern_filter(mut self, name: &str, patterns: &[&str]) -> Self {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(name));
        for pattern in patterns {
            filter.add_pattern(pattern);
        }
        self.filters.push(filter);
        self
    }

    fn build_dialog(&self) -> gtk::FileDialog {
        let mut builder = gtk::FileDialog::builder()
            .title(&self.spec.title)
            .modal(self.modal);
        if let Some(ref label) = self.accept_label {
            builder = builder.accept_label(label);
        }
        if let Some(ref name) = self.initial_name {
            builder = builder.initial_name(name);
        }
        let dialog = builder.build();
        if let Some(ref folder) = self.initial_gio_folder {
            dialog.set_initial_folder(Some(folder));
        } else if let Some(ref dir) = self.initial_folder {
            dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        if !self.filters.is_empty() {
            let store = gio::ListStore::new::<gtk::FileFilter>();
            for filter in &self.filters {
                store.append(filter);
            }
            dialog.set_filters(Some(&store));
        }
        dialog
    }

    /// Open the picker as a modal child of `parent`; `callback` runs
    /// with the chosen path on success and is dropped on cancellation
    /// or error.
    pub fn open<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        self.open_optional(Some(parent), callback);
    }

    /// Same as [`Self::open`] but tolerates a missing parent window;
    /// in that case the portal picks the focus surface itself.
    pub fn open_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        let dialog = self.build_dialog();
        let mut callback = Some(callback);
        dialog.open(parent, None::<&gio::Cancellable>, move |result| {
            let Some(callback) = callback.take() else {
                return;
            };
            if let Ok(file) = result
                && let Some(path) = file.path()
            {
                callback(path);
            }
        });
    }

    /// Open the picker and return the selected `gio::File`.
    ///
    /// Use this variant when callers must preserve URI-backed selections such
    /// as `smb://`, `sftp://`, or `ftp://` instead of reducing the result to a
    /// local filesystem path.
    pub fn open_gio<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(gio::File) + 'static,
    {
        self.open_gio_optional(Some(parent), callback);
    }

    /// Parent-optional variant of [`Self::open_gio`].
    pub fn open_gio_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(gio::File) + 'static,
    {
        let dialog = self.build_dialog();
        let mut callback = Some(callback);
        dialog.open(parent, None::<&gio::Cancellable>, move |result| {
            let Some(callback) = callback.take() else {
                return;
            };
            if let Ok(file) = result {
                callback(file);
            }
        });
    }

    /// Multi-select variant of [`Self::open`]; `callback` runs with the
    /// non-empty selection or is dropped when the user cancels.
    pub fn open_multiple<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(Vec<PathBuf>) + 'static,
    {
        self.open_multiple_optional(Some(parent), callback);
    }

    /// Parent-optional multi-select variant of [`Self::open`].
    pub fn open_multiple_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(Vec<PathBuf>) + 'static,
    {
        let dialog = self.build_dialog();
        let mut callback = Some(callback);
        dialog.open_multiple(parent, None::<&gio::Cancellable>, move |result| {
            if let Ok(files) = result {
                let paths = collect_paths(&files);
                if !paths.is_empty() {
                    let Some(callback) = callback.take() else {
                        return;
                    };
                    callback(paths);
                }
            }
        });
    }

    /// Folder-select variant of [`Self::open`]; portal switches to its
    /// folder picker.
    pub fn select_folder<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        self.select_folder_optional(Some(parent), callback);
    }

    /// Parent-optional variant of [`Self::select_folder`].
    pub fn select_folder_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        let dialog = self.build_dialog();
        let mut callback = Some(callback);
        dialog.select_folder(parent, None::<&gio::Cancellable>, move |result| {
            let Some(callback) = callback.take() else {
                return;
            };
            if let Ok(folder) = result
                && let Some(path) = folder.path()
            {
                callback(path);
            }
        });
    }

    /// Save-as dialog; `callback` receives the chosen destination on
    /// success and is dropped when the user cancels.
    pub fn save<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        self.save_optional(Some(parent), callback);
    }

    /// Parent-optional variant of [`Self::save`].
    pub fn save_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        let dialog = self.build_dialog();
        let mut callback = Some(callback);
        dialog.save(parent, None::<&gio::Cancellable>, move |result| {
            let Some(callback) = callback.take() else {
                return;
            };
            if let Ok(file) = result
                && let Some(path) = file.path()
            {
                callback(path);
            }
        });
    }

    /// Save-as variant that forwards the full `Result` (including IO
    /// errors) so callers can surface diagnostics.
    pub fn save_result<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(Result<Option<PathBuf>, gtk::glib::Error>) + 'static,
    {
        self.save_result_optional(Some(parent), callback);
    }

    /// Parent-optional variant of [`Self::save_result`].
    pub fn save_result_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(Result<Option<PathBuf>, gtk::glib::Error>) + 'static,
    {
        let dialog = self.build_dialog();
        let mut callback = Some(callback);
        dialog.save(parent, None::<&gio::Cancellable>, move |result| {
            let Some(callback) = callback.take() else {
                return;
            };
            callback(result.map(|file| file.path()));
        });
    }

    /// Save-as variant that preserves URI-backed destinations. Use this when a
    /// caller can write through GIO and must not discard SMB/SFTP/etc. choices
    /// just because they do not have a local POSIX path.
    pub fn save_gio_result<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(Result<Option<gio::File>, gtk::glib::Error>) + 'static,
    {
        self.save_gio_result_optional(Some(parent), callback);
    }

    /// Parent-optional variant of [`Self::save_gio_result`].
    pub fn save_gio_result_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(Result<Option<gio::File>, gtk::glib::Error>) + 'static,
    {
        let dialog = self.build_dialog();
        let mut callback = Some(callback);
        dialog.save(parent, None::<&gio::Cancellable>, move |result| {
            let Some(callback) = callback.take() else {
                return;
            };
            callback(result.map(Some));
        });
    }
}

/// Resolves the directory `initial_ssh_folder` lands in.
///
/// Returns `$HOME/.ssh` when that directory exists, otherwise `$HOME`,
/// otherwise `None`.
#[must_use]
pub fn resolve_ssh_initial_folder() -> Option<PathBuf> {
    resolve_ssh_initial_folder_from_home(std::env::var_os("HOME").map(PathBuf::from))
}

fn resolve_ssh_initial_folder_from_home(home: Option<PathBuf>) -> Option<PathBuf> {
    let home = home?;
    let ssh_dir = home.join(".ssh");
    if ssh_dir.is_dir() {
        Some(ssh_dir)
    } else {
        Some(home)
    }
}

fn collect_paths(files: &gio::ListModel) -> Vec<PathBuf> {
    (0..files.n_items())
        .filter_map(|i| files.item(i))
        .filter_map(|obj| obj.downcast::<gio::File>().ok())
        .filter_map(|file| file.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn empty_initial_folder_does_not_update_builder_state() {
        let file_picker = FilePicker::new("Open").initial_folder("");

        assert!(file_picker.initial_folder.is_none());
    }

    #[test]
    fn non_empty_initial_folder_updates_builder_state() {
        let file_picker = FilePicker::new("Open").initial_folder("/tmp");

        assert_eq!(file_picker.initial_folder, Some(PathBuf::from("/tmp")));
        assert!(file_picker.initial_gio_folder.is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn initial_gio_folder_replaces_local_initial_folder() {
        let folder = gio::File::for_uri("smb://server/share/");
        let file_picker = FilePicker::new("Open")
            .initial_folder("/tmp")
            .initial_gio_folder(folder);

        assert!(file_picker.initial_folder.is_none());
        assert_eq!(
            file_picker
                .initial_gio_folder
                .as_ref()
                .map(|file| file.uri()),
            Some("smb://server/share/".into())
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn local_initial_folder_replaces_gio_initial_folder() {
        let folder = gio::File::for_uri("sftp://server/srv/share/");
        let file_picker = FilePicker::new("Open")
            .initial_gio_folder(folder)
            .initial_folder("/tmp");

        assert_eq!(file_picker.initial_folder, Some(PathBuf::from("/tmp")));
        assert!(file_picker.initial_gio_folder.is_none());
    }

    #[test]
    fn public_ssh_initial_folder_uses_current_home_environment() {
        let current_home_directory = std::env::var_os("HOME")
            .map(PathBuf::from)
            .expect("HOME is available for tests");
        let expected_folder = resolve_ssh_initial_folder_from_home(Some(current_home_directory));

        assert_eq!(resolve_ssh_initial_folder(), expected_folder);
    }

    #[test]
    fn ssh_initial_folder_prefers_existing_dot_ssh_directory() {
        let home_directory = tempdir().expect("temporary home directory");
        let ssh_directory = home_directory.path().join(".ssh");
        std::fs::create_dir(&ssh_directory).expect("temporary ssh directory");

        let resolved_folder =
            resolve_ssh_initial_folder_from_home(Some(home_directory.path().to_path_buf()));

        assert_eq!(resolved_folder, Some(ssh_directory));
    }

    #[test]
    fn ssh_initial_folder_falls_back_to_home_directory() {
        let home_directory = tempdir().expect("temporary home directory");

        let resolved_folder =
            resolve_ssh_initial_folder_from_home(Some(home_directory.path().to_path_buf()));

        assert_eq!(resolved_folder, Some(home_directory.path().to_path_buf()));
    }

    #[test]
    fn ssh_initial_folder_absent_when_home_is_missing() {
        assert_eq!(resolve_ssh_initial_folder_from_home(None), None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn collect_paths_keeps_local_file_order() {
        // GIO ListStore<File> calls GLib type registration through FFI, which
        // Miri cannot execute. Normal tests still cover ordered path projection.
        let temporary_directory = tempdir().expect("temporary path collection directory");
        let first_path = temporary_directory.path().join("first.txt");
        let second_path = temporary_directory.path().join("second.txt");
        let file_store = gio::ListStore::new::<gio::File>();
        file_store.append(&gio::File::for_path(&first_path));
        file_store.append(&gio::File::for_path(&second_path));

        let collected_paths = collect_paths(file_store.upcast_ref());

        assert_eq!(collected_paths, vec![first_path, second_path]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn collect_paths_ignores_non_file_objects() {
        // GIO ListStore<Menu> exercises the non-file branch in normal tests,
        // but GLib object construction is outside Miri's Rust UB model.
        let menu_store = gio::ListStore::new::<gio::Menu>();
        menu_store.append(&gio::Menu::new());

        let collected_paths = collect_paths(menu_store.upcast_ref());

        assert!(collected_paths.is_empty());
    }
}
