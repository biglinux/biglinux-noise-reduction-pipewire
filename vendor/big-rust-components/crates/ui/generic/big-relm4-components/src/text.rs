// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Text / number formatting helpers shared across BigLinux GTK apps.

/// Format a byte count with a binary (1024) divisor and decimal unit labels
/// (`B`/`KB`/`MB`/`GB`/`TB`), e.g. `1536` → `"1.5 KB"`, `512` → `"512 B"`.
///
/// This is the workspace's file-size style for galleries, file-preview cards
/// and properties dialogs. It is deliberately distinct from
/// `big_media_components::bytes::decimal_detailed` (1000-based, used for
/// media-file metadata) and from the file-manager's own `KiB` style.
#[must_use]
pub fn format_size(size: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = size as f64;
    let mut idx = 0;
    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{size} {}", UNITS[idx])
    } else {
        format!("{value:.1} {}", UNITS[idx])
    }
}

pub(crate) fn escape_markup_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\'' => escaped.push_str("&apos;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_thresholds() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn escape_markup_text_matches_glib_markup_entities() {
        assert_eq!(
            escape_markup_text("<b>\"A&B's\"</b>"),
            "&lt;b&gt;&quot;A&amp;B&apos;s&quot;&lt;/b&gt;"
        );
    }
}
