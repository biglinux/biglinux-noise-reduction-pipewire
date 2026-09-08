// SPDX-License-Identifier: MIT

//! Filesystem-safe filename sanitization.
//!
//! Turns an arbitrary user string into a filename that is safe to write on
//! common filesystems: forbidden and control characters are replaced or
//! stripped, surrounding whitespace/dots are trimmed, and the result is capped
//! in length. Empty or fully-stripped input collapses to `"unnamed"`.

/// Characters never allowed in a sanitized filename.
const FILENAME_FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*', '\0'];
/// Placeholder returned when sanitization leaves nothing usable.
const UNNAMED: &str = "unnamed";
/// Maximum length (in characters) of a sanitized filename.
pub const MAX_SANITIZED_FILENAME_LENGTH: usize = 128;

/// Sanitize `input` into a filesystem-safe filename, mapping forbidden
/// characters to `replacement`, dropping control characters, trimming leading
/// and trailing spaces and dots, and truncating to
/// [`MAX_SANITIZED_FILENAME_LENGTH`] characters. Returns `"unnamed"` when the
/// result would be empty.
#[must_use]
pub fn sanitize_filename(input: &str, replacement: char) -> String {
    if input.is_empty() {
        return UNNAMED.to_owned();
    }
    let replaced: String = input
        .chars()
        .map(|c| {
            if FILENAME_FORBIDDEN.contains(&c) {
                replacement
            } else {
                c
            }
        })
        .filter(|c| (*c as u32) >= 32)
        .collect();
    let trimmed = replaced.trim_matches(|c: char| c == ' ' || c == '.');
    if trimmed.is_empty() {
        return UNNAMED.to_owned();
    }
    trimmed
        .chars()
        .take(MAX_SANITIZED_FILENAME_LENGTH)
        .collect()
}
/// [`sanitize_filename`] with `'_'` as the replacement character.
#[must_use]
pub fn sanitize_filename_default(input: &str) -> String {
    sanitize_filename(input, '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_table() {
        let long_in = "a".repeat(200);
        let long_out = "a".repeat(MAX_SANITIZED_FILENAME_LENGTH);
        let cases: &[(&str, &str)] = &[
            ("simple", "simple"),
            ("with space", "with space"),
            ("unicode-名前", "unicode-名前"),
            ("", UNNAMED),
            ("   ", UNNAMED),
            ("...", UNNAMED),
            ("safe.txt", "safe.txt"),
            ("has/slash", "has_slash"),
            ("has\\backslash", "has_backslash"),
            ("has\"quote", "has_quote"),
            ("has<lt", "has_lt"),
            ("has>gt", "has_gt"),
            ("has|pipe", "has_pipe"),
            ("has?question", "has_question"),
            ("has*star", "has_star"),
            ("has\x00null", "has_null"),
            ("has\x01control", "hascontrol"),
            ("\x7fdelete", "\x7fdelete"),
            ("../../etc/passwd", "_.._etc_passwd"),
            ("  leading-trailing  ", "leading-trailing"),
            (".hidden", "hidden"),
            ("trailing.", "trailing"),
            (long_in.as_str(), long_out.as_str()),
        ];
        for (raw, expected) in cases {
            assert_eq!(
                sanitize_filename_default(raw),
                *expected,
                "sanitize_filename({raw:?})"
            );
        }
    }

    #[test]
    fn sanitize_filename_custom_replacement() {
        assert_eq!(sanitize_filename("a/b", '-'), "a-b");
    }

    #[test]
    fn sanitize_filename_never_exceeds_max_length() {
        for raw in ["x".repeat(5000), "é".repeat(500), "😀".repeat(100)] {
            let out = sanitize_filename_default(&raw);
            assert!(out.chars().count() <= MAX_SANITIZED_FILENAME_LENGTH);
        }
    }
}
