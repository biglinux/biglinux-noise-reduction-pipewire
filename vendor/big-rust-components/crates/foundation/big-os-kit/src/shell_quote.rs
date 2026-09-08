// SPDX-License-Identifier: MIT

//! POSIX shell single-quoting for values interpolated into a shell command
//! string.
//!
//! Prefer argv-form subprocess spawning (see [`crate::subprocess`]) over
//! building shell strings. Use `shlex_quote` only when a value must be
//! embedded in a shell command line that is genuinely handed to a shell (e.g.
//! a user-facing command preview or a `find -exec` fragment).

/// Quote `s` so it survives POSIX shell word-splitting and expansion as a
/// single literal argument. Safe characters pass through unquoted; anything
/// else is wrapped in single quotes with embedded `'` escaped Python-style
/// (`'"'"'`). An empty string becomes `''`.
#[must_use]
pub fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_owned();
    }
    if s.chars().all(is_shlex_safe) {
        return s.to_owned();
    }
    let escaped = s.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

fn is_shlex_safe(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '_' | '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shlex_quote_empty_yields_two_single_quotes() {
        assert_eq!(shlex_quote(""), "''");
    }

    #[test]
    fn shlex_quote_safe_chars_round_trip_unchanged() {
        for safe in [
            "hello",
            "path/to/file.txt",
            "ABC_123",
            "a@b.c:42",
            "a+b=c",
            "v1.2.3-beta",
            "100%",
        ] {
            assert_eq!(shlex_quote(safe), safe, "safe input changed: {safe}");
        }
    }

    #[test]
    fn shlex_quote_unsafe_wraps_in_single_quotes() {
        assert_eq!(shlex_quote("hello world"), "'hello world'");
        assert_eq!(shlex_quote("a;b"), "'a;b'");
        assert_eq!(shlex_quote("a\"b"), "'a\"b'");
        assert_eq!(shlex_quote("$(whoami)"), "'$(whoami)'");
        assert_eq!(shlex_quote("rm -rf /"), "'rm -rf /'");
    }

    #[test]
    fn shlex_quote_inner_single_quote_escapes_python_style() {
        assert_eq!(shlex_quote("it's"), r#"'it'"'"'s'"#);
        assert_eq!(shlex_quote("'"), r#"''"'"''"#);
    }
}
