// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Private shared row contracts.

use crate::text::escape_markup_text;

#[must_use]
pub(crate) fn row_text(text: &str, allow_markup: bool) -> String {
    if allow_markup {
        text.to_string()
    } else {
        escape_markup_text(text)
    }
}

#[must_use]
pub(crate) fn escaped_text(text: &str) -> String {
    escape_markup_text(text)
}

#[must_use]
pub(crate) fn action_label_or_title(label: &Option<String>, title: &str) -> String {
    label.clone().unwrap_or_else(|| title.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_text_escapes_markup_by_default() {
        assert_eq!(row_text("<b>Audio</b>", false), "&lt;b&gt;Audio&lt;/b&gt;");
    }

    #[test]
    fn escaped_text_always_escapes_markup() {
        assert_eq!(escaped_text("A&B"), "A&amp;B");
    }

    #[test]
    fn row_text_allows_markup_explicitly() {
        assert_eq!(row_text("<b>Audio</b>", true), "<b>Audio</b>");
    }

    #[test]
    fn action_label_falls_back_to_raw_title() {
        assert_eq!(action_label_or_title(&None, "<b>Audio</b>"), "<b>Audio</b>");
        assert_eq!(
            action_label_or_title(&Some("Open audio".to_owned()), "Audio"),
            "Open audio"
        );
    }
}
