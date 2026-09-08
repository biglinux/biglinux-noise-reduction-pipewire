//! Shared HTTP client policy backed by the system `curl` command.
//!
//! The crate keeps the HTTP policy centralized without linking Rust TLS/crypto
//! stacks into every consumer. `curl` is executed with argv-only arguments,
//! explicit timeouts, bounded reads, and validated header values.

use std::io::{Cursor, Read, Write};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Duration;

use crate::url::RedactedUrl;

const CURL: &str = "curl";
const USER_AGENT: &str = "BigLinuxRustApp/0.1 (https://github.com/biglinux/big-rust-components)";
/// Browser-like user agent for favicon/page fetches that reject generic clients.
pub const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120.0.0.0";
const TIMEOUT_SECS: u64 = 15;
const _: () = assert!(TIMEOUT_SECS >= 5);
const _: () = assert!(TIMEOUT_SECS <= 60);
const FAST_TIMEOUT_SECS: u64 = 5;
const FAST_GLOBAL_TIMEOUT_SECS: u64 = 15;
/// 5 MB cap on text bodies; binary helpers use their own caps.
const MAX_TEXT_BODY_BYTES: u64 = 5 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 128 * 1024;
const DEFAULT_MAX_REDIRECTS: u8 = 10;

/// Response body plus the metadata consumers need without depending on a Rust
/// HTTP crate.
#[derive(Debug, Clone)]
pub struct HttpResponseBytes {
    /// HTTP status code returned by the server after redirects.
    pub status: u16,
    /// Requested URI. The curl backend follows redirects but does not expose
    /// the post-redirect URI yet.
    pub final_url: String,
    /// Response Content-Type header, if present.
    pub content_type: Option<String>,
    /// Response bytes, bounded by the caller-provided cap.
    pub bytes: Vec<u8>,
}

/// Streaming response plus metadata.
pub struct HttpStreamResponse {
    /// HTTP status code returned by the server after redirects.
    pub status: u16,
    /// Requested URI. The curl backend follows redirects but does not expose
    /// the post-redirect URI yet.
    pub final_url: String,
    /// Response Content-Type header, if present.
    pub content_type: Option<String>,
    /// Content-Length header, if present and parseable.
    pub content_length: Option<u64>,
    /// Streaming body reader. Callers must impose their own semantic cap.
    pub reader: Box<dyn Read>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpMethod {
    Get,
    Post,
}

struct CurlHeaders {
    status: u16,
    content_type: Option<String>,
    content_length: Option<u64>,
}

struct CurlBodyReader {
    child: Child,
    buffered: Cursor<Vec<u8>>,
    stdout: ChildStdout,
}

impl Read for CurlBodyReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.buffered.position() < self.buffered.get_ref().len() as u64 {
            return self.buffered.read(buf);
        }
        self.stdout.read(buf)
    }
}

impl Drop for CurlBodyReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// DNS/lookup failures are permanent per invocation — never retry.
fn is_dns_error(err: &str) -> bool {
    err.contains("code 6")
        || err.contains("Could not resolve host")
        || err.contains("Name or service not known")
        || err.contains("No address associated")
        || err.contains("failed to lookup address")
        || err.contains("dns error")
}

/// One retry after 500 ms for transient errors (timeout, 5xx); skip on DNS.
pub fn http_get_with_retry(url: &str) -> Option<String> {
    http_get_with_retry_timeout(url, Duration::from_secs(TIMEOUT_SECS))
}

/// Single attempt; caller (radio_api) handles multi-server fallback.
pub fn http_get_fast(url: &str) -> Option<String> {
    http_get_single(url, Duration::from_secs(FAST_GLOBAL_TIMEOUT_SECS))
}

fn http_get_single(url: &str, timeout: Duration) -> Option<String> {
    let url_log = RedactedUrl(url);
    match curl_bytes(
        HttpMethod::Get,
        url,
        &RequestHeaders::default(),
        &[],
        None,
        MAX_TEXT_BODY_BYTES,
        timeout,
        true,
    ) {
        Ok(resp) if resp.status == 200 => String::from_utf8(resp.bytes).ok(),
        Ok(resp) => {
            log::debug!("[http] {url_log}: status={}", resp.status);
            None
        }
        Err(error) => {
            log::debug!("[http] {url_log}: error={error}");
            None
        }
    }
}

fn http_get_with_retry_timeout(url: &str, timeout: Duration) -> Option<String> {
    let url_log = RedactedUrl(url);
    for attempt in 0..2 {
        match curl_bytes(
            HttpMethod::Get,
            url,
            &RequestHeaders::default(),
            &[],
            None,
            MAX_TEXT_BODY_BYTES,
            timeout,
            true,
        ) {
            Ok(resp) if resp.status < 500 => {
                if resp.status != 200 {
                    log::debug!("[http] {url_log}: non-200 status={}", resp.status);
                    return None;
                }
                return String::from_utf8(resp.bytes)
                    .inspect_err(|e| log::debug!("[http] {url_log}: utf8 body error={e}"))
                    .ok();
            }
            Ok(resp) => {
                log::debug!(
                    "[http] {url_log}: 5xx status={} attempt={attempt}",
                    resp.status
                );
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            Err(error) if is_dns_error(&error) => {
                log::debug!("[http] {url_log}: DNS error={error}");
                return None;
            }
            Err(error) => {
                log::debug!("[http] {url_log}: error={error} attempt={attempt}");
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    }
    None
}

/// `Accept: application/json` — MusicBrainz/CoverArtArchive return XML
/// otherwise. Body parsing left to callers; same retry as `http_get_with_retry`.
pub fn http_get_json(url: &str) -> Option<String> {
    http_get_text_with_headers(
        url,
        &RequestHeaders::default(),
        &[("Accept", "application/json")],
        Duration::from_secs(TIMEOUT_SECS),
        true,
    )
}

/// Form-urlencoded POST. Returns the HTTP status on connection success so
/// callers can branch on 200 vs 4xx/5xx. **No retry** — scrobbles must not
/// double-post; signature is the caller's responsibility.
pub fn http_post_form(url: &str, form_body: &str) -> Result<u16, String> {
    curl_bytes(
        HttpMethod::Post,
        url,
        &RequestHeaders::default(),
        &[("Content-Type", "application/x-www-form-urlencoded")],
        Some(form_body.as_bytes()),
        1024 * 1024,
        Duration::from_secs(TIMEOUT_SECS),
        false,
    )
    .map(|resp| resp.status)
}

/// JSON POST + optional auth header (e.g. `("Authorization", "Token xyz")`
/// for ListenBrainz, `"Bearer ..."` for OAuth). No retry.
pub fn http_post_json(
    url: &str,
    json_body: &str,
    auth: Option<(&str, &str)>,
) -> Result<u16, String> {
    let mut extra_headers = vec![("Content-Type", "application/json")];
    if let Some((name, value)) = auth {
        extra_headers.push((name, value));
    }
    curl_bytes(
        HttpMethod::Post,
        url,
        &RequestHeaders::default(),
        &extra_headers,
        Some(json_body.as_bytes()),
        1024 * 1024,
        Duration::from_secs(TIMEOUT_SECS),
        false,
    )
    .map(|resp| resp.status)
}

/// Binary GET (cover images etc.); 1 MB cap.
pub fn http_get_bytes(url: &str) -> Option<Vec<u8>> {
    let http_response = curl_bytes(
        HttpMethod::Get,
        url,
        &RequestHeaders::default(),
        &[],
        None,
        1024 * 1024,
        Duration::from_secs(TIMEOUT_SECS),
        true,
    )
    .ok()?;
    (http_response.status == 200).then_some(http_response.bytes)
}

/// IPTV providers often require specific `User-Agent`/`Referer`; empty fields
/// fall back to the command default. Use [`RequestHeaders::default`] otherwise.
#[derive(Debug, Clone, Default)]
pub struct RequestHeaders<'a> {
    /// Override for the `User-Agent` header; empty falls back to the default.
    pub user_agent: &'a str,
    /// Override for the `Referer` header; empty omits the header.
    pub referer: &'a str,
}

impl<'a> RequestHeaders<'a> {
    /// Browser-like headers for sites that reject generic HTTP clients.
    pub fn browser() -> Self {
        Self {
            user_agent: BROWSER_USER_AGENT,
            referer: "",
        }
    }

    /// Return a copy with a User-Agent override.
    pub fn user_agent(mut self, value: &'a str) -> Self {
        self.user_agent = value;
        self
    }

    /// Return a copy with a Referer header.
    pub fn referer(mut self, value: &'a str) -> Self {
        self.referer = value;
        self
    }

    fn is_empty(&self) -> bool {
        self.user_agent.is_empty() && self.referer.is_empty()
    }

    fn pairs(&self) -> Vec<(&str, &str)> {
        let mut pairs = Vec::with_capacity(2);
        if !self.user_agent.is_empty() {
            pairs.push(("User-Agent", self.user_agent));
        }
        if !self.referer.is_empty() {
            pairs.push(("Referer", self.referer));
        }
        pairs
    }
}

/// Reuses the shared command policy; retries once on transient errors.
pub fn http_get_with_headers(url: &str, headers: &RequestHeaders<'_>) -> Option<String> {
    if headers.is_empty() {
        return http_get_with_retry(url);
    }
    http_get_text_with_headers(url, headers, &[], Duration::from_secs(TIMEOUT_SECS), true)
}

/// Binary GET + per-request headers; 1 MB cap.
pub fn http_get_bytes_with_headers(url: &str, headers: &RequestHeaders<'_>) -> Option<Vec<u8>> {
    let http_response = curl_bytes(
        HttpMethod::Get,
        url,
        headers,
        &[],
        None,
        1024 * 1024,
        Duration::from_secs(TIMEOUT_SECS),
        true,
    )
    .ok()?;
    (http_response.status == 200).then_some(http_response.bytes)
}

/// GET a response into memory with caller-owned body cap and response metadata.
pub fn http_get_bytes_capped(
    url: &str,
    headers: &RequestHeaders<'_>,
    max_bytes: u64,
    timeout: Duration,
) -> Result<HttpResponseBytes, String> {
    http_get_bytes_capped_with_extra_headers(url, headers, &[], max_bytes, timeout)
}

/// GET a response into memory with caller-owned body cap, response metadata,
/// and additional per-call headers.
pub fn http_get_bytes_capped_with_extra_headers(
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    max_bytes: u64,
    timeout: Duration,
) -> Result<HttpResponseBytes, String> {
    curl_bytes(
        HttpMethod::Get,
        url,
        headers,
        extra_headers,
        None,
        max_bytes,
        timeout,
        true,
    )
}

/// GET a streaming response with metadata and caller-owned body policy.
pub fn http_get_stream(
    url: &str,
    headers: &RequestHeaders<'_>,
    timeout: Duration,
) -> Result<HttpStreamResponse, String> {
    http_get_stream_with_extra_headers(url, headers, &[], timeout)
}

/// GET a streaming response with metadata, additional per-call headers, and
/// caller-owned body policy.
pub fn http_get_stream_with_extra_headers(
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<HttpStreamResponse, String> {
    curl_stream(
        HttpMethod::Get,
        url,
        headers,
        extra_headers,
        None,
        timeout,
        true,
    )
}

/// POST JSON and return a streaming response. Redirects are disabled because
/// callers commonly attach API keys in headers; never forward secrets to a
/// redirected host.
pub fn http_post_json_stream(
    url: &str,
    json_body: &str,
    headers: &RequestHeaders<'_>,
    timeout: Duration,
) -> Result<HttpStreamResponse, String> {
    http_post_json_stream_with_extra_headers(url, json_body, headers, &[], timeout)
}

/// POST JSON and return a streaming response with additional per-call headers.
/// Redirects are disabled because callers commonly attach API keys in headers;
/// never forward secrets to a redirected host.
pub fn http_post_json_stream_with_extra_headers(
    url: &str,
    json_body: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<HttpStreamResponse, String> {
    let mut all_headers = vec![("Content-Type", "application/json")];
    all_headers.extend_from_slice(extra_headers);
    curl_stream(
        HttpMethod::Post,
        url,
        headers,
        &all_headers,
        Some(json_body.as_bytes()),
        timeout,
        false,
    )
}

/// Read a stream into memory, refusing to exceed `max_bytes`.
pub fn read_stream_capped(reader: &mut dyn Read, max_bytes: u64) -> Result<Vec<u8>, String> {
    read_capped(reader, max_bytes)
}

/// Copy a response stream to a writer with cancellation and progress callbacks.
pub fn copy_stream_with_progress<W, C, P>(
    reader: &mut dyn Read,
    writer: &mut W,
    mut is_cancelled: C,
    mut on_chunk: P,
) -> Result<(), String>
where
    W: Write,
    C: FnMut() -> bool,
    P: FnMut(u64),
{
    let mut buf = [0_u8; 65_536];
    loop {
        if is_cancelled() {
            return Err("cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        writer.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        on_chunk(n as u64);
    }
}

fn http_get_text_with_headers(
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    timeout: Duration,
    follow_redirects: bool,
) -> Option<String> {
    let url_log = RedactedUrl(url);
    for attempt in 0..2 {
        match curl_bytes(
            HttpMethod::Get,
            url,
            headers,
            extra_headers,
            None,
            MAX_TEXT_BODY_BYTES,
            timeout,
            follow_redirects,
        ) {
            Ok(resp) if resp.status < 500 => {
                if resp.status != 200 {
                    log::debug!("[http] {url_log}: non-200 status={}", resp.status);
                    return None;
                }
                return String::from_utf8(resp.bytes)
                    .inspect_err(|e| log::debug!("[http] {url_log}: utf8 body error={e}"))
                    .ok();
            }
            Ok(resp) => {
                log::debug!(
                    "[http] {url_log}: 5xx status={} attempt={attempt}",
                    resp.status
                );
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            Err(error) if is_dns_error(&error) => {
                log::debug!("[http] {url_log}: DNS error={error}");
                return None;
            }
            Err(error) => {
                log::debug!("[http] {url_log}: error={error} attempt={attempt}");
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    }
    None
}

fn curl_bytes(
    method: HttpMethod,
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
    max_bytes: u64,
    timeout: Duration,
    follow_redirects: bool,
) -> Result<HttpResponseBytes, String> {
    let mut child = spawn_curl(
        method,
        url,
        headers,
        extra_headers,
        body,
        timeout,
        follow_redirects,
    )?;
    if let Some(body) = body {
        write_request_body(&mut child, body)?;
    }
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "curl stdout unavailable".to_owned())?;
    let parsed_headers = read_final_headers(&mut stdout, follow_redirects)?;
    let bytes = match read_capped(&mut stdout, max_bytes) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    wait_for_success(child)?;
    Ok(HttpResponseBytes {
        status: parsed_headers.status,
        final_url: url.to_owned(),
        content_type: parsed_headers.content_type,
        bytes,
    })
}

fn curl_stream(
    method: HttpMethod,
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
    timeout: Duration,
    follow_redirects: bool,
) -> Result<HttpStreamResponse, String> {
    let mut child = spawn_curl(
        method,
        url,
        headers,
        extra_headers,
        body,
        timeout,
        follow_redirects,
    )?;
    if let Some(body) = body {
        write_request_body(&mut child, body)?;
    }
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "curl stdout unavailable".to_owned())?;
    let parsed_headers = match read_final_headers(&mut stdout, follow_redirects) {
        Ok(headers) => headers,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    Ok(HttpStreamResponse {
        status: parsed_headers.status,
        final_url: url.to_owned(),
        content_type: parsed_headers.content_type,
        content_length: parsed_headers.content_length,
        reader: Box::new(CurlBodyReader {
            child,
            buffered: Cursor::new(Vec::new()),
            stdout,
        }),
    })
}

fn spawn_curl(
    method: HttpMethod,
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
    timeout: Duration,
    follow_redirects: bool,
) -> Result<Child, String> {
    validate_url(url)?;
    let mut command = base_curl(timeout, headers.user_agent);
    command
        .arg("--request")
        .arg(match method {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
        })
        .arg("--include");
    if follow_redirects {
        command
            .arg("--location")
            .arg("--max-redirs")
            .arg(DEFAULT_MAX_REDIRECTS.to_string());
    }
    for (name, value) in headers.pairs() {
        push_header_arg(&mut command, name, value)?;
    }
    for (name, value) in extra_headers.iter().copied() {
        if !value.is_empty() {
            push_header_arg(&mut command, name, value)?;
        }
    }
    if body.is_some() {
        command.arg("--data-binary").arg("@-").stdin(Stdio::piped());
    }
    command
        .arg("--url")
        .arg(url)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn curl: {e}"))
}

fn base_curl(timeout: Duration, user_agent: &str) -> Command {
    let mut command = Command::new(CURL);
    command
        .arg("--silent")
        .arg("--show-error")
        .arg("--connect-timeout")
        .arg(FAST_TIMEOUT_SECS.to_string())
        .arg("--max-time")
        .arg(timeout.as_secs_f64().to_string())
        .arg("--user-agent")
        .arg(if user_agent.is_empty() {
            USER_AGENT
        } else {
            user_agent
        })
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn validate_url(url: &str) -> Result<(), String> {
    if url.contains('\0') {
        return Err("URL contains a NUL byte".to_owned());
    }
    Ok(())
}

fn push_header_arg(command: &mut Command, name: &str, value: &str) -> Result<(), String> {
    validate_header_name(name)?;
    validate_header_value(value)?;
    command.arg("--header").arg(format!("{name}: {value}"));
    Ok(())
}

fn validate_header_name(value: &str) -> Result<(), String> {
    if value.is_empty() || value.contains(':') || value.contains('\r') || value.contains('\n') {
        return Err("HTTP header name is invalid".to_owned());
    }
    Ok(())
}

fn validate_header_value(value: &str) -> Result<(), String> {
    if value.contains('\r') || value.contains('\n') {
        return Err("HTTP header contains a newline".to_owned());
    }
    Ok(())
}

fn write_request_body(child: &mut Child, body: &[u8]) -> Result<(), String> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "curl stdin unavailable".to_owned())?;
    stdin
        .write_all(body)
        .map_err(|e| format!("write curl stdin: {e}"))
}

fn read_final_headers(
    stdout: &mut ChildStdout,
    follow_redirects: bool,
) -> Result<CurlHeaders, String> {
    loop {
        let headers = read_one_header_block(stdout)?;
        let status = parse_status(&headers)?;
        let location = parse_header_value(&headers, "location");
        if (100..200).contains(&status)
            || (follow_redirects && (300..400).contains(&status) && location.is_some())
        {
            continue;
        }
        let content_type = parse_header_value(&headers, "content-type");
        let content_length = parse_header_value(&headers, "content-length")
            .and_then(|value| value.parse::<u64>().ok());
        return Ok(CurlHeaders {
            status,
            content_type,
            content_length,
        });
    }
}

fn read_one_header_block(stdout: &mut ChildStdout) -> Result<Vec<u8>, String> {
    let mut headers = Vec::new();
    let mut byte = [0_u8; 1];
    while headers.len() <= MAX_HEADER_BYTES {
        let n = stdout
            .read(&mut byte)
            .map_err(|e| format!("read curl headers: {e}"))?;
        if n == 0 {
            return Err("curl ended before HTTP headers".to_owned());
        }
        headers.push(byte[0]);
        if headers.ends_with(b"\r\n\r\n") || headers.ends_with(b"\n\n") {
            return Ok(headers);
        }
    }
    Err(format!(
        "HTTP headers exceeded {MAX_HEADER_BYTES} bytes cap"
    ))
}

fn parse_status(headers: &[u8]) -> Result<u16, String> {
    let text = String::from_utf8_lossy(headers);
    text.lines()
        .filter_map(|line| line.strip_prefix("HTTP/"))
        .filter_map(|rest| rest.split_whitespace().nth(1))
        .filter_map(|code| code.parse::<u16>().ok())
        .next_back()
        .ok_or_else(|| "missing HTTP status from curl headers".to_owned())
}

fn parse_header_value(headers: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(headers);
    text.lines().find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        header_name
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn read_capped(reader: &mut dyn Read, max_bytes: u64) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    let mut limited = reader.take(max_bytes + 1);
    limited
        .read_to_end(&mut body)
        .map_err(|e| format!("read curl body: {e}"))?;
    if body.len() as u64 > max_bytes {
        return Err(format!("response body exceeded {max_bytes} bytes cap"));
    }
    Ok(body)
}

fn wait_for_success(mut child: Child) -> Result<(), String> {
    let status = child.wait().map_err(|e| format!("wait curl: {e}"))?;
    if status.success() {
        Ok(())
    } else if let Some(code) = status.code() {
        Err(format!("curl exited with code {code}"))
    } else {
        Err(format!("curl exited with status {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_identifies_biglinux_rust_components() {
        assert!(USER_AGENT.contains("BigLinuxRustApp"));
    }

    #[test]
    // Miri does not implement `posix_spawn`; normal `cargo test` still checks
    // that the system curl backend fails closed for invalid URLs.
    #[cfg_attr(miri, ignore)]
    fn retry_returns_none_for_invalid_url() {
        assert!(http_get_with_retry("not-a-url").is_none());
    }

    #[test]
    // Miri does not implement `posix_spawn`; normal `cargo test` still checks
    // that the fast system curl path fails closed for invalid URLs.
    #[cfg_attr(miri, ignore)]
    fn fast_returns_none_for_invalid_url() {
        assert!(http_get_fast("not-a-url").is_none());
    }

    #[test]
    fn fast_timeout_is_shorter() {
        const { assert!(FAST_TIMEOUT_SECS < TIMEOUT_SECS) };
        const { assert!(FAST_TIMEOUT_SECS >= 3) };
        const { assert!(FAST_TIMEOUT_SECS <= 10) };
    }

    #[test]
    fn fast_global_timeout_reasonable() {
        const { assert!(FAST_GLOBAL_TIMEOUT_SECS >= TIMEOUT_SECS) };
        const { assert!(FAST_GLOBAL_TIMEOUT_SECS <= 60) };
    }

    #[test]
    fn dns_error_detected() {
        assert!(is_dns_error("curl exited with code 6"));
    }

    #[test]
    fn timeout_error_not_dns() {
        assert!(!is_dns_error("curl exited with code 28"));
    }

    #[test]
    // Miri does not implement `posix_spawn`; normal `cargo test` still checks
    // that the binary curl path fails closed for invalid URLs.
    #[cfg_attr(miri, ignore)]
    fn bytes_returns_none_for_invalid_url() {
        assert!(http_get_bytes("not-a-url").is_none());
    }

    #[test]
    // Empty headers intentionally delegate into the system curl retry path.
    #[cfg_attr(miri, ignore)]
    fn empty_headers_delegate_to_retry() {
        let headers = RequestHeaders::default();
        assert!(headers.is_empty());
        assert!(http_get_with_headers("not-a-url", &headers).is_none());
    }

    #[test]
    fn request_headers_default_is_empty() {
        let h = RequestHeaders::default();
        assert!(h.user_agent.is_empty());
        assert!(h.referer.is_empty());
    }

    #[test]
    // Miri does not implement `posix_spawn`; normal `cargo test` still checks
    // that the header-aware binary curl path fails closed for invalid URLs.
    #[cfg_attr(miri, ignore)]
    fn bytes_with_headers_returns_none_for_invalid_url() {
        assert!(http_get_bytes_with_headers("not-a-url", &RequestHeaders::default()).is_none());
    }

    #[test]
    fn rejects_header_newlines() {
        let mut command = Command::new(CURL);
        assert!(
            push_header_arg(&mut command, "Authorization", "Bearer ok\nInjected: yes").is_err()
        );
    }

    #[test]
    fn rejects_invalid_header_name() {
        let mut command = Command::new(CURL);
        assert!(push_header_arg(&mut command, "Bad:Header", "value").is_err());
    }

    #[test]
    fn parses_status_from_last_http_header_block() {
        let headers =
            b"HTTP/1.1 100 Continue\r\n\r\nHTTP/2 200\r\ncontent-type: text/plain\r\n\r\n";
        assert_eq!(parse_status(headers).unwrap(), 200);
    }

    #[test]
    fn parses_header_values_case_insensitively() {
        let headers = b"HTTP/2 200\r\nContent-Type: text/plain\r\nContent-Length: 42\r\n\r\n";
        assert_eq!(
            parse_header_value(headers, "content-type"),
            Some("text/plain".to_owned())
        );
        assert_eq!(
            parse_header_value(headers, "content-length"),
            Some("42".to_owned())
        );
    }
}
