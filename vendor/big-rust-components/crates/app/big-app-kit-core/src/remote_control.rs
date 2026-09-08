//! HTTP server for token-scoped local web remotes.
//!
//! Serves a single-page HTML control surface plus a tiny JSON API:
//!
//! ```text
//! GET  /<token>/            -> remote_control.html
//! GET  /<token>/status      -> current playback snapshot (JSON)
//! POST /<token>/play-pause  -> toggle
//! POST /<token>/next
//! POST /<token>/prev
//! POST /<token>/volume?v=75 -> 0..100
//! ```
//!
//! Requests outside `/<token>/...` are answered with 404 — the token is
//! per-session and shared with the phone only through the QR code, so LAN
//! neighbors cannot guess the URL.

use std::io::Read;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::{info, warn};

/// Playback state returned by `GET /<token>/status`.
pub struct StatusSnapshot {
    /// `true` when the media is currently playing.
    pub playing: bool,
    /// Track title sent back to the remote.
    pub title: String,
    /// Track artist sent back to the remote.
    pub artist: String,
    /// Playback position in seconds.
    pub position_s: f64,
    /// Track duration in seconds.
    pub duration_s: f64,
    /// Current volume as a `0.0..=100.0` percent value.
    pub volume: f64,
}

/// Invoked on the HTTP worker thread — the window must marshal back to the
/// main thread (e.g. `glib::idle_add_once`) before touching GTK/mpv.
pub struct RemoteHandlers {
    /// Closure that toggles play/pause on the media engine.
    pub play_pause: Arc<dyn Fn() + Send + Sync>,
    /// Closure that advances to the next track.
    pub next: Arc<dyn Fn() + Send + Sync>,
    /// Closure that returns to the previous track.
    pub prev: Arc<dyn Fn() + Send + Sync>,
    /// Closure that sets the volume given a `0.0..=100.0` percent value.
    pub set_volume: Arc<dyn Fn(f64) + Send + Sync>,
    /// Closure that snapshots the current playback state for JSON
    /// serialisation.
    pub status: Arc<dyn Fn() -> StatusSnapshot + Send + Sync>,
}

/// Drop or call [`ServerHandle::stop`] to shut the server down.
pub struct ServerHandle {
    /// TCP port the listener bound to; surface to the user via QR
    /// code.
    pub port: u16,
    /// Per-session random token required in every URL path.
    pub token: String,
    stop: Arc<AtomicBool>,
}

impl ServerHandle {
    /// Signal the worker thread to exit on its next poll. Idempotent;
    /// also invoked by [`Drop`].
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Returns once the listener is bound; serves on a worker thread thereafter.
pub fn start(handlers: RemoteHandlers) -> Result<ServerHandle, String> {
    let listener = TcpListener::bind("0.0.0.0:0").map_err(|e| format!("bind: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    let server =
        tiny_http::Server::from_listener(listener, None).map_err(|e| format!("http: {e}"))?;

    let token = make_token();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_thread = Arc::clone(&stop);
    let token_for_thread = token.clone();

    thread::spawn(move || {
        loop {
            if stop_for_thread.load(Ordering::Relaxed) {
                break;
            }
            match server.recv_timeout(Duration::from_millis(500)) {
                Ok(Some(request)) => handle(request, &token_for_thread, &handlers),
                Ok(None) => {}
                Err(e) => {
                    warn!("web-remote recv error: {e}");
                    break;
                }
            }
        }
        info!("web-remote stopped on port {port}");
    });

    info!("web-remote listening on port {port}");
    Ok(ServerHandle { port, token, stop })
}

fn handle(request: tiny_http::Request, token: &str, handlers: &RemoteHandlers) {
    let expected_prefix = format!("/{token}");
    if !request.url().starts_with(&expected_prefix) {
        let _ =
            request.respond(tiny_http::Response::from_string("Not Found").with_status_code(404));
        return;
    }
    let tail = &request.url()[expected_prefix.len()..];
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    let path = path.trim_start_matches('/');

    match path {
        "" | "index.html" => respond_html(request, INDEX_HTML),
        "status" => respond_json(request, &status_json(&(handlers.status)())),
        "play-pause" => {
            (handlers.play_pause)();
            respond_ok(request);
        }
        "next" => {
            (handlers.next)();
            respond_ok(request);
        }
        "prev" => {
            (handlers.prev)();
            respond_ok(request);
        }
        "volume" => {
            if let Some(volume) = parse_volume(query) {
                (handlers.set_volume)(volume);
            }
            respond_ok(request);
        }
        _ => {
            let _ = request
                .respond(tiny_http::Response::from_string("Not Found").with_status_code(404));
        }
    }
}

fn respond_html(request: tiny_http::Request, body: &'static str) {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], b"text/html; charset=utf-8")
        .expect("static header");
    let response = tiny_http::Response::from_string(body).with_header(header);
    let _ = request.respond(response);
}

fn respond_json(request: tiny_http::Request, body: &str) {
    let header =
        tiny_http::Header::from_bytes(&b"Content-Type"[..], b"application/json; charset=utf-8")
            .expect("static header");
    let response = tiny_http::Response::from_string(body).with_header(header);
    let _ = request.respond(response);
}

fn respond_ok(request: tiny_http::Request) {
    let _ = request.respond(tiny_http::Response::from_string("ok"));
}

fn status_json(status_snapshot: &StatusSnapshot) -> String {
    // Hand-rolled to dodge serde codegen on a hot path.
    format!(
        "{{\"playing\":{},\"title\":\"{}\",\"artist\":\"{}\",\"position\":{:.3},\"duration\":{:.3},\"volume\":{:.1}}}",
        status_snapshot.playing,
        escape_json(&status_snapshot.title),
        escape_json(&status_snapshot.artist),
        status_snapshot.position_s,
        status_snapshot.duration_s,
        status_snapshot.volume,
    )
}

fn escape_json(raw_text: &str) -> String {
    let mut escaped_text = String::with_capacity(raw_text.len());
    for character in raw_text.chars() {
        match character {
            '"' => escaped_text.push_str("\\\""),
            '\\' => escaped_text.push_str("\\\\"),
            '\n' => escaped_text.push_str("\\n"),
            '\r' => escaped_text.push_str("\\r"),
            '\t' => escaped_text.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                escaped_text.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped_text.push(character),
        }
    }
    escaped_text
}

fn parse_volume(query: &str) -> Option<f64> {
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix("v=") {
            return v.parse::<f64>().ok().map(|v| v.clamp(0.0, 100.0));
        }
    }
    None
}

fn make_token() -> String {
    let mut bytes = [0_u8; 16];
    if let Ok(mut urandom_file) = std::fs::File::open("/dev/urandom")
        && urandom_file.read_exact(&mut bytes).is_ok()
    {
        return hex(&bytes);
    }
    warn!("web-remote: /dev/urandom unavailable, using degraded token source");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let pid = u128::from(std::process::id());
    degraded_token_from_seed(now, pid)
}

fn degraded_token_from_seed(now: u128, pid: u128) -> String {
    let mut mix = now ^ (pid << 64);
    let mut bytes = [0_u8; 16];
    for byte in &mut bytes {
        *byte = (mix & 0xff) as u8;
        mix = mix.rotate_left(7).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    hex(&bytes)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex_text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex_text.push(DIGITS[(byte >> 4) as usize] as char);
        hex_text.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    hex_text
}

const INDEX_HTML: &str = include_str!("remote_control.html");

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpStream;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn volume_parse_clamps() {
        assert_eq!(parse_volume("v=50"), Some(50.0));
        assert_eq!(parse_volume("v=150"), Some(100.0));
        assert_eq!(parse_volume("v=-5"), Some(0.0));
        assert_eq!(parse_volume("foo=bar&v=42.5"), Some(42.5));
        assert_eq!(parse_volume("nope"), None);
    }

    #[test]
    fn escape_json_specials() {
        assert_eq!(escape_json("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        assert_eq!(escape_json("a\rb\t\u{1f}"), "a\\rb\\t\\u001f");
        assert_eq!(escape_json("a b"), "a b");
    }

    #[test]
    fn status_json_is_valid_and_escaped() {
        let status_snapshot = StatusSnapshot {
            playing: true,
            title: "A \"quoted\" title\n".into(),
            artist: "Artist\\Name".into(),
            position_s: 12.34567,
            duration_s: 98.76543,
            volume: 42.26,
        };

        let value: serde_json::Value =
            serde_json::from_str(&status_json(&status_snapshot)).unwrap();

        assert_eq!(value["playing"], true);
        assert_eq!(value["title"], "A \"quoted\" title\n");
        assert_eq!(value["artist"], "Artist\\Name");
        assert_eq!(value["position"], 12.346);
        assert_eq!(value["duration"], 98.765);
        assert_eq!(value["volume"], 42.3);
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa2, 0xff]), "000fa2ff");
    }

    #[test]
    fn token_is_32_hex_chars() {
        let t = make_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(t.chars().all(|c| !c.is_ascii_uppercase()));
    }

    #[test]
    fn degraded_token_fallback_mixes_timestamp_and_runtime_identifier() {
        assert_eq!(
            degraded_token_from_seed(
                0x0123_4567_89ab_cdef_0011_2233_4455_6677,
                0x0102_0304_0506_0708,
            ),
            "7780dfc160b1cce8934b4af8caf19557"
        );
        assert_ne!(
            degraded_token_from_seed(
                0x0123_4567_89ab_cdef_0011_2233_4455_6677,
                0x0102_0304_0506_0709,
            ),
            "7780dfc160b1cce8934b4af8caf19557"
        );
    }

    #[test]
    fn server_handle_stop_sets_shutdown_flag() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let handle = ServerHandle {
            port: 12345,
            token: "token".into(),
            stop: Arc::clone(&stop_flag),
        };

        handle.stop();

        assert!(stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn server_handle_drop_sets_shutdown_flag() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        {
            let _handle = ServerHandle {
                port: 12345,
                token: "token".into(),
                stop: Arc::clone(&stop_flag),
            };
        }

        assert!(stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn local_http_routes_dispatch_handlers_and_render_responses() {
        // This is a live loopback TCP integration test. Miri can execute the
        // pure route helpers above, but socket readiness is outside its UB model.
        let play_pause_count = Arc::new(AtomicUsize::new(0));
        let next_count = Arc::new(AtomicUsize::new(0));
        let previous_count = Arc::new(AtomicUsize::new(0));
        let status_count = Arc::new(AtomicUsize::new(0));
        let observed_volumes = Arc::new(Mutex::new(Vec::new()));

        let handlers = RemoteHandlers {
            play_pause: {
                let play_pause_count = Arc::clone(&play_pause_count);
                Arc::new(move || {
                    play_pause_count.fetch_add(1, Ordering::Relaxed);
                })
            },
            next: {
                let next_count = Arc::clone(&next_count);
                Arc::new(move || {
                    next_count.fetch_add(1, Ordering::Relaxed);
                })
            },
            prev: {
                let previous_count = Arc::clone(&previous_count);
                Arc::new(move || {
                    previous_count.fetch_add(1, Ordering::Relaxed);
                })
            },
            set_volume: {
                let observed_volumes = Arc::clone(&observed_volumes);
                Arc::new(move |volume| {
                    observed_volumes.lock().unwrap().push(volume);
                })
            },
            status: {
                let status_count = Arc::clone(&status_count);
                Arc::new(move || {
                    status_count.fetch_add(1, Ordering::Relaxed);
                    StatusSnapshot {
                        playing: true,
                        title: "Remote \"Track\"".into(),
                        artist: "BigLinux".into(),
                        position_s: 1.25,
                        duration_s: 5.0,
                        volume: 75.0,
                    }
                })
            },
        };

        let server_handle = start(handlers).unwrap();
        let token_path = format!("/{}", server_handle.token);

        let index_response =
            send_http_request(server_handle.port, "GET", &format!("{token_path}/"));
        assert!(index_response.starts_with("HTTP/1.1 200"));
        assert!(index_response.contains("Content-Type: text/html"));

        let status_response =
            send_http_request(server_handle.port, "GET", &format!("{token_path}/status"));
        assert!(status_response.starts_with("HTTP/1.1 200"));
        assert!(status_response.contains("Content-Type: application/json"));
        let status_value: serde_json::Value =
            serde_json::from_str(response_body(&status_response)).unwrap();
        assert_eq!(status_value["playing"], true);
        assert_eq!(status_value["title"], "Remote \"Track\"");
        assert_eq!(status_count.load(Ordering::Relaxed), 1);

        assert_ok_response(&send_http_request(
            server_handle.port,
            "POST",
            &format!("{token_path}/play-pause"),
        ));
        assert_ok_response(&send_http_request(
            server_handle.port,
            "POST",
            &format!("{token_path}/next"),
        ));
        assert_ok_response(&send_http_request(
            server_handle.port,
            "POST",
            &format!("{token_path}/prev"),
        ));
        assert_ok_response(&send_http_request(
            server_handle.port,
            "POST",
            &format!("{token_path}/volume?v=150"),
        ));

        assert_eq!(play_pause_count.load(Ordering::Relaxed), 1);
        assert_eq!(next_count.load(Ordering::Relaxed), 1);
        assert_eq!(previous_count.load(Ordering::Relaxed), 1);
        assert_eq!(*observed_volumes.lock().unwrap(), vec![100.0]);

        let unauthorized_response =
            send_http_request(server_handle.port, "GET", "/wrong-token/status");
        assert!(unauthorized_response.starts_with("HTTP/1.1 404"));
        assert!(unauthorized_response.contains("Not Found"));
        assert_eq!(status_count.load(Ordering::Relaxed), 1);

        server_handle.stop();
    }

    fn send_http_request(port: u16, method: &str, request_target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let request = format!(
            "{method} {request_target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).unwrap();

        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn response_body(response: &str) -> &str {
        response.split_once("\r\n\r\n").unwrap().1
    }

    fn assert_ok_response(response: &str) {
        assert!(response.starts_with("HTTP/1.1 200"));
        assert_eq!(response_body(response), "ok");
    }
}
