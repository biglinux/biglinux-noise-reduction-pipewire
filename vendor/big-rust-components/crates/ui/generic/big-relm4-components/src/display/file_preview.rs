// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Small file/path preview card used by hover popovers and file views.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk::prelude::*;
use gtk::{gio, glib};
use relm4::gtk;

const ICON_PX: i32 = 64;
const POPOVER_MAX_WIDTH: i32 = 540;
const TEXT_HEAD_HEIGHT: i32 = 240;

/// Preview feature limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigFilePreviewConfig {
    /// Show dir.
    pub show_dir: bool,
    /// Dir max entries.
    pub dir_max_entries: usize,
    /// Show image.
    pub show_image: bool,
    /// Image max px.
    pub image_max_px: i32,
    /// Image max mib.
    pub image_max_mib: u64,
    /// Show text.
    pub show_text: bool,
    /// Text max kib.
    pub text_max_kib: u64,
    /// Text max lines.
    pub text_max_lines: usize,
}

impl Default for BigFilePreviewConfig {
    fn default() -> Self {
        Self {
            show_dir: true,
            dir_max_entries: 12,
            show_image: true,
            image_max_px: 320,
            image_max_mib: 20,
            show_text: true,
            text_max_kib: 256,
            text_max_lines: 20,
        }
    }
}

/// Localizable words used by the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFilePreviewLabels {
    /// Empty.
    pub empty: String,
    /// Entry singular.
    pub entry_singular: String,
    /// Entry plural.
    pub entry_plural: String,
    /// Directory.
    pub directory: String,
    /// Symbolic link.
    pub symbolic_link: String,
    /// File.
    pub file: String,
}

impl BigFilePreviewLabels {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        empty: impl Into<String>,
        entry_singular: impl Into<String>,
        entry_plural: impl Into<String>,
        directory: impl Into<String>,
        symbolic_link: impl Into<String>,
        file: impl Into<String>,
    ) -> Self {
        Self {
            empty: empty.into(),
            entry_singular: entry_singular.into(),
            entry_plural: entry_plural.into(),
            directory: directory.into(),
            symbolic_link: symbolic_link.into(),
            file: file.into(),
        }
    }
}

/// One metadata line under the title in a preview-card header.
pub struct CardHeaderLine {
    /// Already-resolved, already-localized text (the caller owns formatting +
    /// i18n, so date formats and wording never drift between consumers).
    pub text: String,
    /// CSS classes, e.g. `&["body", "dim-label"]` or
    /// `&["caption", "dim-label", "monospace"]`.
    pub css: &'static [&'static str],
    /// Middle-ellipsize + cap at 48 chars (used for the name and symlink rows).
    pub ellipsize: bool,
}

impl CardHeaderLine {
    /// Creates a new instance.
    #[must_use]
    pub fn new(text: impl Into<String>, css: &'static [&'static str], ellipsize: bool) -> Self {
        Self {
            text: text.into(),
            css,
            ellipsize,
        }
    }
}

/// Build the shared preview-card header: a 64 px icon on the left and a column
/// with a `heading` title plus `lines` of dimmed metadata. The canonical layout
/// for every BigLinux file/media preview card — consumers resolve their own
/// text/i18n/date format and only share the widget construction + styling, so
/// the cards look identical without coupling their data sources.
#[must_use]
pub fn card_header_row(icon_name: &str, title: &str, lines: &[CardHeaderLine]) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(ICON_PX);
    icon.set_valign(gtk::Align::Start);
    icon.set_halign(gtk::Align::Center);
    row.append(&icon);

    let metadata_column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    metadata_column.append(&card_header_label(title, &["heading"], true));
    for line in lines {
        metadata_column.append(&card_header_label(&line.text, line.css, line.ellipsize));
    }
    row.append(&metadata_column);
    row
}

/// Whether `path` is an image extension the shared file preview can decode or
/// thumbnail.
#[must_use]
pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" | "heic"
            )
        })
}

/// Whether `path` is an MP4-family video container that commonly has desktop
/// thumbnails and ISO-BMFF metadata previews.
#[must_use]
pub fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4" | "mov" | "m4v" | "3gp" | "3g2"
            )
        })
}

/// Return a bounded UTF-8-lossy text head for preview widgets.
#[must_use]
pub fn preview_text_from_bytes(bytes: &[u8], max_lines: usize, truncated: bool) -> Option<String> {
    if max_lines == 0 || looks_binary(bytes) {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let head: Vec<&str> = lines.by_ref().take(max_lines).collect();
    if head.is_empty() {
        return None;
    }
    let has_more_lines = lines.next().is_some();
    let mut joined = head.join("\n");
    if has_more_lines || truncated {
        joined.push_str("\n…");
    }
    Some(joined)
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8 * 1024).any(|byte| *byte == 0)
}

/// Format Unix permission bits as a nine-character `rwxrwxrwx` string.
#[must_use]
pub fn unix_mode_string(mode: u32) -> String {
    let mut out = String::with_capacity(9);
    let bits = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    for (bit, ch) in bits {
        out.push(if mode & bit == bit { ch } else { '-' });
    }
    out
}

fn card_header_label(text: &str, css: &[&str], ellipsize: bool) -> gtk::Label {
    let builder = gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .css_classes(css.to_vec());
    let builder = if ellipsize {
        builder
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .max_width_chars(48)
    } else {
        builder
    };
    builder.build()
}

/// Build a compact preview widget for a filesystem path.
#[must_use]
pub fn build_file_preview(
    path: &Path,
    cfg: BigFilePreviewConfig,
    labels: &BigFilePreviewLabels,
) -> gtk::Box {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(10)
        .margin_end(10)
        .margin_top(10)
        .margin_bottom(10)
        .build();
    let meta = std::fs::symlink_metadata(path).ok();
    root.append(&header_row(path, meta.as_ref(), labels));
    if let Some(body) = body_widget(path, meta.as_ref(), cfg, labels) {
        root.append(&body);
    }
    root
}

fn header_row(
    path: &Path,
    meta: Option<&std::fs::Metadata>,
    labels: &BigFilePreviewLabels,
) -> gtk::Box {
    let icon_name = resolve_icon_name(path, meta);
    let title = path.file_name().map_or_else(
        || path.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    let mut lines = Vec::new();
    if let Some(text) = kind_size_text(path, meta, labels) {
        lines.push(CardHeaderLine::new(text, &["body", "dim-label"], false));
    }
    if let Some(text) = meta.and_then(|m| m.modified().ok()).and_then(format_mtime) {
        lines.push(CardHeaderLine::new(text, &["caption", "dim-label"], false));
    }
    if let Some(meta) = meta {
        lines.push(CardHeaderLine::new(
            unix_mode_string(meta.permissions().mode()),
            &["caption", "dim-label", "monospace"],
            false,
        ));
    }
    if let Some(text) = symlink_target_text(path, meta) {
        lines.push(CardHeaderLine::new(text, &["caption", "dim-label"], true));
    }
    card_header_row(&icon_name, &title, &lines)
}

fn kind_size_text(
    path: &Path,
    meta: Option<&std::fs::Metadata>,
    labels: &BigFilePreviewLabels,
) -> Option<String> {
    let meta = meta?;
    let kind = describe_kind(path, meta, labels);
    Some(if meta.file_type().is_file() {
        format!("{kind} · {}", format_size(meta.len()))
    } else if meta.file_type().is_dir() {
        match directory_entry_count(path) {
            Some(n) => format!("{kind} · {n} {}", entries_word(n, labels)),
            None => kind,
        }
    } else {
        kind
    })
}

fn symlink_target_text(path: &Path, meta: Option<&std::fs::Metadata>) -> Option<String> {
    if !meta?.file_type().is_symlink() {
        return None;
    }
    let target = std::fs::read_link(path).ok()?;
    Some(format!("→ {}", target.display()))
}

fn body_widget(
    path: &Path,
    meta: Option<&std::fs::Metadata>,
    cfg: BigFilePreviewConfig,
    labels: &BigFilePreviewLabels,
) -> Option<gtk::Widget> {
    let meta = meta?;
    if meta.file_type().is_dir() {
        return cfg
            .show_dir
            .then(|| directory_listing(path, cfg.dir_max_entries, labels).upcast());
    }
    if cfg.show_image
        && is_image_path(path)
        && meta.len() <= cfg.image_max_mib.saturating_mul(1024 * 1024)
        && let Some(picture) = image_preview(path, cfg.image_max_px)
    {
        return Some(picture.upcast());
    }
    if cfg.show_text
        && meta.len() <= cfg.text_max_kib.saturating_mul(1024)
        && is_text_like(path)
        && let Some(head) = text_head(path, cfg.text_max_lines)
    {
        return Some(head.upcast());
    }
    None
}

fn image_preview(path: &Path, max_px: i32) -> Option<gtk::Box> {
    let texture = crate::display::texture::texture_from_file_at_scale(path, max_px)?;
    let w = texture.width();
    let h = texture.height();
    let pic = gtk::Picture::for_paintable(&texture);
    pic.set_can_shrink(false);
    pic.set_content_fit(gtk::ContentFit::Contain);
    pic.set_size_request(w, h);
    pic.set_hexpand(false);
    pic.set_vexpand(false);
    pic.set_halign(gtk::Align::Center);
    pic.set_valign(gtk::Align::Center);
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    frame.set_size_request(w, h);
    frame.append(&pic);
    Some(frame)
}

fn directory_listing(
    path: &Path,
    max_entries: usize,
    labels: &BigFilePreviewLabels,
) -> gtk::ScrolledWindow {
    let entries = collect_directory(path, max_entries);
    let label_text = if entries.is_empty() {
        labels.empty.clone()
    } else {
        entries.join("\n")
    };
    let label = gtk::Label::builder()
        .label(label_text)
        .halign(gtk::Align::Start)
        .selectable(false)
        .css_classes(["monospace"])
        .build();
    gtk::ScrolledWindow::builder()
        .child(&label)
        .max_content_height(TEXT_HEAD_HEIGHT)
        .max_content_width(POPOVER_MAX_WIDTH)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build()
}

fn collect_directory(path: &Path, max_entries: usize) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut rows: Vec<(bool, String)> = read
        .flatten()
        .map(|e| {
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            let name = e.file_name().to_string_lossy().into_owned();
            (is_dir, name)
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    rows.into_iter()
        .take(max_entries)
        .map(|(is_dir, name)| if is_dir { format!("{name}/") } else { name })
        .collect()
}

fn directory_entry_count(path: &Path) -> Option<usize> {
    Some(std::fs::read_dir(path).ok()?.flatten().count())
}

fn entries_word(n: usize, labels: &BigFilePreviewLabels) -> &str {
    if n == 1 {
        &labels.entry_singular
    } else {
        &labels.entry_plural
    }
}

fn text_head(path: &Path, max_lines: usize) -> Option<gtk::ScrolledWindow> {
    use std::io::Read;
    // A head preview never needs the whole file. This widget is built on the GTK
    // main thread, so reading a multi-GB file here (the old `std::fs::read`)
    // would stall the UI and spike memory. Read only a bounded prefix — ample
    // for `max_lines` of normal text.
    const PREVIEW_CAP_BYTES: u64 = 128 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(PREVIEW_CAP_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    // We read exactly the cap → the file is larger; the line count is a lower
    // bound, so show an open-ended "…" rather than a precise (+N).
    let capped = bytes.len() as u64 >= PREVIEW_CAP_BYTES;
    if looks_binary(&bytes) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let head: Vec<&str> = text.lines().take(max_lines).collect();
    if head.is_empty() {
        return None;
    }
    let total_lines = text.lines().count();
    let suffix = if capped {
        "\n…".to_owned()
    } else if total_lines > max_lines {
        format!("\n… (+{})", total_lines.saturating_sub(max_lines))
    } else {
        String::new()
    };
    let mut joined = head.join("\n");
    joined.push_str(&suffix);
    let label = gtk::Label::builder()
        .label(joined)
        .halign(gtk::Align::Start)
        .selectable(false)
        .css_classes(["monospace"])
        .build();
    Some(
        gtk::ScrolledWindow::builder()
            .child(&label)
            .max_content_height(TEXT_HEAD_HEIGHT)
            .max_content_width(POPOVER_MAX_WIDTH)
            .propagate_natural_height(true)
            .propagate_natural_width(true)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .build(),
    )
}

fn is_text_like(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|s| s.to_string_lossy().into_owned()) else {
        return false;
    };
    let (mime, _) = gio::content_type_guess(Some(&name), None);
    mime.starts_with("text/") || gio::content_type_is_a(&mime, "text/plain")
}

fn resolve_icon_name(path: &Path, meta: Option<&std::fs::Metadata>) -> String {
    if meta.is_some_and(|m| m.file_type().is_dir()) {
        return "folder".to_owned();
    }
    let Some(name) = path.file_name().map(|s| s.to_string_lossy().into_owned()) else {
        return "text-x-generic".to_owned();
    };
    let (mime, _) = gio::content_type_guess(Some(&name), None);
    let gicon = gio::functions::content_type_get_icon(&mime);
    gicon
        .downcast_ref::<gio::ThemedIcon>()
        .and_then(|t| t.names().first().map(glib::GString::to_string))
        .unwrap_or_else(|| "text-x-generic".to_owned())
}

fn describe_kind(path: &Path, meta: &std::fs::Metadata, labels: &BigFilePreviewLabels) -> String {
    if meta.file_type().is_dir() {
        return labels.directory.clone();
    }
    if meta.file_type().is_symlink() {
        return labels.symbolic_link.clone();
    }
    let Some(name) = path.file_name().map(|s| s.to_string_lossy().into_owned()) else {
        return labels.file.clone();
    };
    let (mime, _) = gio::content_type_guess(Some(&name), None);
    let description = gio::content_type_get_description(&mime);
    let s = description.to_string();
    if s.is_empty() { labels.file.clone() } else { s }
}

use crate::text::format_size;

fn format_mtime(t: SystemTime) -> Option<String> {
    let secs = t.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let secs = i64::try_from(secs).ok()?;
    let dt = glib::DateTime::from_unix_local(secs).ok()?;
    dt.format("%Y-%m-%d %H:%M").ok().map(|gs| gs.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_word_uses_singular_only_for_one() {
        let labels = BigFilePreviewLabels::new(
            "(empty)",
            "entry",
            "entries",
            "Directory",
            "Symbolic link",
            "File",
        );
        assert_eq!(entries_word(1, &labels), "entry");
        assert_eq!(entries_word(2, &labels), "entries");
    }

    #[test]
    fn mode_string_formats_unix_permissions() {
        assert_eq!(unix_mode_string(0o100644), "rw-r--r--");
        assert_eq!(unix_mode_string(0o040755), "rwxr-xr-x");
    }

    #[test]
    fn image_path_matches_supported_preview_extensions() {
        assert!(is_image_path(Path::new("photo.JPG")));
        assert!(is_image_path(Path::new("icon.avif")));
        assert!(!is_image_path(Path::new("movie.mp4")));
    }

    #[test]
    fn video_path_matches_mp4_family_extensions() {
        assert!(is_video_path(Path::new("clip.MOV")));
        assert!(is_video_path(Path::new("phone.3gp")));
        assert!(!is_video_path(Path::new("clip.webm")));
    }

    #[test]
    fn preview_text_from_bytes_limits_lines() {
        let text = preview_text_from_bytes(b"one\ntwo\nthree\n", 2, false).unwrap();

        assert_eq!(text, "one\ntwo\n…");
    }

    #[test]
    fn preview_text_from_bytes_marks_truncated_input() {
        let text = preview_text_from_bytes(b"one line", 4, true).unwrap();

        assert_eq!(text, "one line\n…");
    }

    #[test]
    fn preview_text_from_bytes_rejects_binary_input() {
        assert!(preview_text_from_bytes(b"abc\0def", 4, false).is_none());
    }
}
