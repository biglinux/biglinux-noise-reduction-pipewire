// SPDX-License-Identifier: MIT

//! Text-vs-binary detection and editor-mode classification for arbitrary
//! files, independent of any widget toolkit.
//!
//! Given a path, a small sniff window of leading bytes, and an optional MIME
//! hint, these helpers decide whether a file is editable text and which
//! `EditorMode` a plain-text editor should open it in (Markdown vs. a syntax
//! language). Extension and stem tables resolve most cases without reading the
//! file; `sniff_mime` falls back to `tree_magic_mini` only when the
//! extension is inconclusive.

use std::path::Path;

/// How a plain-text editor should treat a file's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorMode {
    /// Render as Markdown (live preview / prose highlighting).
    Markdown,
    /// Plain text, optionally with a syntax-highlighting language token.
    PlainText {
        /// Syntax language token (e.g. `"rust"`), or `None` for no highlighting.
        syntax: Option<String>,
    },
}

/// Upper bound on a file's size, in bytes, for it to be treated as editable
/// text (10 MiB).
pub const MAX_TEXT_BYTES: u64 = 10 * 1024 * 1024;

/// Number of leading bytes inspected when sniffing a file's content.
pub const SNIFF_WINDOW: usize = 4096;

/// File extensions (lowercase, without dot) that open in [`EditorMode::Markdown`].
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd"];

/// Map of file extension (lowercase, no dot) to syntax-highlighting language
/// token. An empty token means "plain text, no highlighting".
pub const EXTENSION_LANG: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("py", "python3"),
    ("sh", "sh"),
    ("bash", "sh"),
    ("zsh", "sh"),
    ("fish", "sh"),
    ("c", "c"),
    ("h", "c"),
    ("cc", "cpp"),
    ("cpp", "cpp"),
    ("cxx", "cpp"),
    ("hpp", "cpp"),
    ("hh", "cpp"),
    ("js", "js"),
    ("mjs", "js"),
    ("cjs", "js"),
    ("ts", "typescript"),
    ("tsx", "typescript"),
    ("jsx", "js"),
    ("json", "json"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("toml", "toml"),
    ("ini", "ini"),
    ("conf", "ini"),
    ("html", "html"),
    ("htm", "html"),
    ("css", "css"),
    ("scss", "scss"),
    ("xml", "xml"),
    ("svg", "xml"),
    ("go", "go"),
    ("rb", "ruby"),
    ("php", "php"),
    ("lua", "lua"),
    ("sql", "sql"),
    ("dockerfile", "dockerfile"),
    ("desktop", "desktop"),
    ("service", "ini"),
    ("nix", "nix"),
    ("pkgbuild", "sh"),
    ("install", "sh"),
    ("patch", "diff"),
    ("diff", "diff"),
    ("log", ""),
    ("txt", ""),
    ("text", ""),
    ("env", "sh"),
];
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}
fn stem_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}
/// True if `path` has a Markdown extension (see [`MARKDOWN_EXTENSIONS`]).
#[must_use]
pub fn is_markdown_extension(path: &Path) -> bool {
    let ext = extension(path);
    MARKDOWN_EXTENSIONS.contains(&ext.as_str())
}
/// Resolve a syntax-highlighting language token for `path` from its extension,
/// falling back to well-known stems (`Dockerfile`, `Makefile`, dotfiles).
/// Returns `None` when no highlighting applies.
#[must_use]
pub fn syntax_for_path(path: &Path) -> Option<String> {
    let ext = extension(path);
    if !ext.is_empty()
        && let Some((_, lang)) = EXTENSION_LANG.iter().find(|(e, _)| *e == ext)
    {
        if lang.is_empty() {
            return None;
        }
        return Some((*lang).to_string());
    }
    let stem = stem_name(path);
    match stem.as_str() {
        "dockerfile" | "containerfile" => Some("dockerfile".to_string()),
        "makefile" | "gnumakefile" => Some("makefile".to_string()),
        "cmakelists.txt" => Some("cmake".to_string()),
        "pkgbuild" | ".bashrc" | ".bash_profile" | ".zshrc" | ".profile" => Some("sh".to_string()),
        ".gitconfig" | ".gitignore" | ".gitmodules" => Some("ini".to_string()),
        _ => None,
    }
}
/// Infer a syntax language token from a file's `#!` shebang line, if present.
#[must_use]
pub fn shebang_lang(text: &str) -> Option<&'static str> {
    let first = text.lines().find(|l| !l.trim().is_empty())?;
    let rest = first.strip_prefix("#!")?;
    let rest = rest.trim_start();
    let interp = rest
        .split_whitespace()
        .next_back()
        .or_else(|| rest.split_whitespace().next())?;
    let name = interp.rsplit('/').next()?;
    let key = name.trim_end_matches(char::is_numeric).to_ascii_lowercase();
    match key.as_str() {
        "bash" | "sh" | "zsh" | "ksh" | "dash" | "fish" => Some("sh"),
        "python" => Some("python3"),
        "node" | "nodejs" => Some("js"),
        "ruby" => Some("ruby"),
        "perl" => Some("perl"),
        "lua" => Some("lua"),
        "awk" | "gawk" => Some("awk"),
        "tcl" | "wish" => Some("tcl"),
        "php" => Some("php"),
        _ => None,
    }
}
/// Classify a file's [`EditorMode`] from its path alone (extension/stem),
/// without inspecting content. Returns `None` when the extension is unknown.
#[must_use]
pub fn detect_mode_by_extension(path: &Path) -> Option<EditorMode> {
    if is_markdown_extension(path) {
        return Some(EditorMode::Markdown);
    }
    let ext = extension(path);
    if ext.is_empty() {
        // Stem-based hits (Dockerfile, Makefile, …) still resolve.
        if let Some(syntax) = syntax_for_path(path) {
            return Some(EditorMode::PlainText {
                syntax: Some(syntax),
            });
        }
        return None;
    }
    if let Some((_, lang)) = EXTENSION_LANG.iter().find(|(e, _)| *e == ext) {
        let syntax = if lang.is_empty() {
            None
        } else {
            Some((*lang).to_string())
        };
        return Some(EditorMode::PlainText { syntax });
    }
    None
}
/// True if a MIME type names a textual format (`text/*` plus a curated set of
/// text-bearing `application/*` types).
#[must_use]
pub fn is_text_mime(mime: &str) -> bool {
    let mime = mime.trim().to_ascii_lowercase();
    if mime.starts_with("text/") {
        return true;
    }
    matches!(
        mime.as_str(),
        "application/json"
            | "application/xml"
            | "application/xhtml+xml"
            | "application/javascript"
            | "application/x-shellscript"
            | "application/x-sh"
            | "application/x-yaml"
            | "application/x-toml"
            | "application/toml"
            | "application/x-perl"
            | "application/x-ruby"
            | "application/x-python"
            | "application/x-php"
            | "application/sql"
    )
}
/// Heuristic binary check: true if any of the first [`SNIFF_WINDOW`] bytes is a
/// NUL byte.
#[must_use]
pub fn looks_binary(sniff: &[u8]) -> bool {
    sniff.iter().take(SNIFF_WINDOW).any(|b| *b == 0)
}
/// Guess a MIME type from a byte sample via `tree_magic_mini`, defaulting to
/// `text/plain` for an empty sample.
#[must_use]
pub fn sniff_mime(sniff: &[u8]) -> String {
    if sniff.is_empty() {
        return "text/plain".to_string();
    }
    tree_magic_mini::from_u8(sniff).to_string()
}
/// Decide the [`EditorMode`] for a file, combining extension classification, a
/// caller-supplied MIME hint, and content sniffing. Always resolves to some
/// mode (defaults to plain text).
#[must_use]
pub fn detect_mode(path: &Path, sniff: &[u8], mime_hint: Option<&str>) -> EditorMode {
    if let Some(mode) = detect_mode_by_extension(path) {
        return mode;
    }
    if let Some(hint) = mime_hint
        && is_text_mime(hint)
    {
        return EditorMode::PlainText { syntax: None };
    }
    let sniffed = sniff_mime(sniff);
    if is_text_mime(&sniffed) {
        return EditorMode::PlainText { syntax: None };
    }
    EditorMode::PlainText { syntax: None }
}
/// True if a file should be treated as editable text: a known text extension,
/// or a non-binary sniff/MIME match, and within the [`MAX_TEXT_BYTES`] size cap.
#[must_use]
pub fn is_text_file(path: &Path, sniff: &[u8], mime_hint: Option<&str>, size: u64) -> bool {
    if is_markdown_extension(path) || detect_mode_by_extension(path).is_some() {
        return size <= MAX_TEXT_BYTES;
    }
    if looks_binary(sniff) {
        return false;
    }
    if let Some(hint) = mime_hint
        && is_text_mime(hint)
    {
        return size <= MAX_TEXT_BYTES;
    }
    if is_text_mime(&sniff_mime(sniff)) {
        return size <= MAX_TEXT_BYTES;
    }
    false
}
