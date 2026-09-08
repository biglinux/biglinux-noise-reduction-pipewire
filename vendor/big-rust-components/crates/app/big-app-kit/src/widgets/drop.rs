// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! File drag-and-drop helpers for app widgets.

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use relm4::gtk;
use std::path::PathBuf;

/// Parse a `text/uri-list` payload into one entry per line.
///
/// Strips blank lines and `#`-prefixed comments per RFC 2483. `max_items`
/// caps the resulting vector; `None` means unbounded. Callers typically
/// feed the result into [`drop_paths_from_text`] to resolve `file://` URIs.
#[must_use]
pub fn parse_uri_list(text: &str, max_items: Option<usize>) -> Vec<String> {
    let limit = max_items.unwrap_or(usize::MAX);
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .take(limit)
        .map(ToOwned::to_owned)
        .collect()
}

/// Resolve a `text/uri-list` drop payload to local filesystem paths.
///
/// Each entry is parsed by [`parse_uri_list`]; `file://` URIs are decoded
/// via [`gio::File::for_uri`] while plain entries are taken verbatim as
/// [`PathBuf`]. Entries that cannot be decoded are dropped silently.
#[must_use]
pub fn drop_paths_from_text(text: &str, max_items: Option<usize>) -> Vec<PathBuf> {
    parse_uri_list(text, max_items)
        .into_iter()
        .filter_map(|entry| {
            if entry.starts_with("file://") {
                gio::File::for_uri(&entry).path()
            } else {
                Some(PathBuf::from(entry))
            }
        })
        .collect()
}

/// Extract dropped paths from a `gtk::DropTarget` signal value.
///
/// Recognises [`gio::File`], [`gdk::FileList`], and plain `String`
/// (treated as a uri-list). Returns an empty vector when the value carries
/// no recognised type — typical receiver code aborts the drop on empty.
#[must_use]
pub fn drop_paths_from_value(value: &glib::Value) -> Vec<PathBuf> {
    if let Ok(file) = value.get::<gio::File>() {
        return file.path().into_iter().collect();
    }
    if let Ok(list) = value.get::<gdk::FileList>() {
        return list
            .files()
            .into_iter()
            .filter_map(|file| file.path())
            .collect();
    }
    if let Ok(text) = value.get::<String>() {
        return drop_paths_from_text(&text, None);
    }
    Vec::new()
}

/// Build a [`gtk::DropTarget`] that accepts file/path payloads.
///
/// The returned target reports the supplied [`gdk::DragAction`] and recognises
/// `gio::File`, `gdk::FileList`, and plain string drops. Pair with
/// [`drop_paths_from_value`] to extract paths from the drop signal.
pub fn path_drop_target(action: gdk::DragAction) -> gtk::DropTarget {
    let drop_target = gtk::DropTarget::builder().actions(action).build();
    drop_target.set_types(&[
        gio::File::static_type(),
        gdk::FileList::static_type(),
        String::static_type(),
    ]);
    drop_target
}

/// Attach a copy-action drop target to `widget` that forwards dropped paths.
///
/// The callback returns `true` to accept the drop, `false` to refuse it.
///
/// # Examples
///
/// ```no_run
/// use big_app_kit::widgets::add_file_drop_target;
/// use gtk::prelude::*;
///
/// gtk::init().unwrap();
/// let label = gtk::Label::new(Some("Drop files here"));
/// add_file_drop_target(&label, |paths| {
///     println!("got {} paths", paths.len());
///     true
/// });
/// ```
pub fn add_file_drop_target<W, F>(widget: &W, on_paths: F)
where
    W: IsA<gtk::Widget>,
    F: Fn(Vec<PathBuf>) -> bool + 'static,
{
    let drop_target = path_drop_target(gdk::DragAction::COPY);
    drop_target.connect_drop(move |_, value, _, _| {
        let paths = drop_paths_from_value(value);
        if paths.is_empty() {
            return false;
        }
        on_paths(paths)
    });
    widget.add_controller(drop_target);
}

/// Recursively expand directory drops via a user-supplied collector.
///
/// Files pass through unchanged; directory entries are replaced by the
/// `collect_dir` closure's output (typically a recursive readdir). Callers
/// pass this after [`drop_paths_from_value`] when a drop target should
/// accept folders.
#[must_use]
pub fn expand_drop_paths(
    paths: impl IntoIterator<Item = PathBuf>,
    collect_dir: impl Fn(&PathBuf) -> Vec<PathBuf>,
) -> Vec<PathBuf> {
    paths
        .into_iter()
        .flat_map(|path| {
            if path.is_dir() {
                collect_dir(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}
