//! RFC 3986 percent-encoding helpers for URL query values.

/// Query-parameter keys (matched case-insensitively) whose values [`redact_url`]
/// masks before a URL is logged. Covers Xtream Codes credentials
/// (`username`/`password`) and common token/secret parameter names.
const SENSITIVE_QUERY_KEYS: &[&str] = &[
    "password",
    "pass",
    "pwd",
    "username",
    "user",
    "token",
    "access_token",
    "secret",
    "api_key",
    "apikey",
    "key",
    "auth",
    "session",
    "sid",
];

fn is_sensitive_query_key(key: &str) -> bool {
    SENSITIVE_QUERY_KEYS
        .iter()
        .any(|sensitive| key.eq_ignore_ascii_case(sensitive))
}

/// Mask the `userinfo` (`user:pass@`) component of a URL authority, if present.
fn redact_userinfo(base: &str) -> String {
    let Some(scheme_end) = base.find("://") else {
        return base.to_string();
    };
    let authority_start = scheme_end + 3;
    let rest = &base[authority_start..];
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    match authority.find('@') {
        Some(at) => format!(
            "{}***@{}{}",
            &base[..authority_start],
            &authority[at + 1..],
            &rest[authority_end..]
        ),
        None => base.to_string(),
    }
}

/// Mask credentials in a URL so it is safe to log: the authority `userinfo`
/// (`user:pass@`) and the values of sensitive query parameters (see
/// `SENSITIVE_QUERY_KEYS` — e.g. Xtream `username`/`password`) become `***`.
/// Best-effort string rewrite that never fails; a non-URL string is returned
/// unchanged apart from any `key=value` query pairs it recognizes.
#[must_use]
pub fn redact_url(url: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };

    let mut out = redact_userinfo(base);

    if let Some(query) = query {
        out.push('?');
        for (index, pair) in query.split('&').enumerate() {
            if index > 0 {
                out.push('&');
            }
            match pair.split_once('=') {
                Some((key, value)) if !value.is_empty() && is_sensitive_query_key(key) => {
                    out.push_str(key);
                    out.push_str("=***");
                }
                _ => out.push_str(pair),
            }
        }
    }
    out
}

/// Lazy [`Display`](std::fmt::Display) wrapper that redacts a URL only when it is
/// actually formatted — so passing `RedactedUrl(url)` to a `log::debug!` costs
/// nothing when the log level is disabled, yet never leaks credentials when it
/// fires. See [`redact_url`].
pub struct RedactedUrl<'a>(pub &'a str);

impl std::fmt::Display for RedactedUrl<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&redact_url(self.0))
    }
}

/// Percent-encode a query value while preserving the RFC 3986 unreserved set.
#[must_use]
pub fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Return the final URL path segment when it looks like a filename.
#[must_use]
pub fn filename_from_url_path(url: &str, fallback_filename: &str) -> String {
    url.rsplit('/')
        .next()
        .and_then(|segment| {
            let name = segment.split('?').next().unwrap_or(segment);
            if name.is_empty() || !name.contains('.') {
                None
            } else {
                Some(name.to_string())
            }
        })
        .unwrap_or_else(|| fallback_filename.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_value_keeps_unreserved_ascii() {
        assert_eq!(encode_query_value("A-Z_0.9~"), "A-Z_0.9~");
    }

    #[test]
    fn query_value_encodes_spaces_and_reserved_chars() {
        assert_eq!(encode_query_value("a b&c=d?/"), "a%20b%26c%3Dd%3F%2F");
    }

    #[test]
    fn query_value_encodes_utf8_bytes() {
        assert_eq!(encode_query_value("café"), "caf%C3%A9");
        assert_eq!(encode_query_value("東京"), "%E6%9D%B1%E4%BA%AC");
    }

    #[test]
    fn filename_from_url_path_keeps_last_segment_without_query() {
        assert_eq!(
            filename_from_url_path("https://example.test/a/b/song.flac?token=1", "fallback.mp3"),
            "song.flac"
        );
    }

    #[test]
    fn redact_masks_xtream_username_and_password() {
        assert_eq!(
            redact_url("http://host:8080/get.php?username=joe&password=hunter2&type=m3u_plus"),
            "http://host:8080/get.php?username=***&password=***&type=m3u_plus"
        );
    }

    #[test]
    fn redact_masks_userinfo() {
        assert_eq!(
            redact_url("http://joe:hunter2@host:8080/live/stream.ts"),
            "http://***@host:8080/live/stream.ts"
        );
    }

    #[test]
    fn redact_masks_token_secret_and_key_case_insensitively() {
        assert_eq!(
            redact_url("https://api.test/v1?API_KEY=abc&Token=xyz&page=2"),
            "https://api.test/v1?API_KEY=***&Token=***&page=2"
        );
    }

    #[test]
    fn redact_leaves_clean_urls_untouched() {
        let clean = "https://example.test/a/b/song.flac?format=json&page=3";
        assert_eq!(redact_url(clean), clean);
    }

    #[test]
    fn redact_keeps_empty_sensitive_values_empty() {
        assert_eq!(
            redact_url("http://host/get.php?password=&type=m3u"),
            "http://host/get.php?password=&type=m3u"
        );
    }

    #[test]
    fn redacted_url_display_matches_redact_url() {
        let url = "http://host/get.php?username=joe&password=secret";
        assert_eq!(format!("{}", RedactedUrl(url)), redact_url(url));
    }

    #[test]
    fn filename_from_url_path_requires_extension() {
        assert_eq!(
            filename_from_url_path("https://example.test/a/stream", "fallback.mp3"),
            "fallback.mp3"
        );
        assert_eq!(
            filename_from_url_path("https://example.test/a/", "fallback.mp3"),
            "fallback.mp3"
        );
    }
}
