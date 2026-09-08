from pathlib import Path
from _common import done, write, replace, commit

TITLE = 'feat(audio): make buffer previews reversible and automatically expire'
if not done(TITLE):
    write('src/services/preview.rs', r'''//! A reversible 15-second graph-quantum preview.
//!
//! A small CLI child owns the override, not the window. Closing its stdin
//! cancels the preview, including when the GUI exits unexpectedly. A deadline
//! restores the previous value even if the window stops responding. Restoration
//! checks that another application has not replaced our temporary value.

use std::io::{self, BufRead, Write};
use std::os::fd::AsRawFd;
use std::process::{ChildStdin, ExitCode};
use std::time::{Duration, Instant};

use big_os_kit::subprocess::{BigSubprocessChild, BigSubprocessOutputMode, BigSubprocessSpec};
use serde_json::Value;

const LIFETIME: Duration = Duration::from_secs(15);
const VALUES: &[u32] = &[0, 256, 512, 1024, 2048, 4096];

pub struct QuantumPreview {
    child: Option<BigSubprocessChild>,
    cancellation: Option<ChildStdin>,
}

impl QuantumPreview {
    /// Blocking startup; invoke on a worker, never from a GTK signal handler.
    pub fn start(frames: u32) -> io::Result<Self> {
        if !VALUES.contains(&frames) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "unsupported preview quantum"));
        }
        let cli = std::env::current_exe()?.with_file_name("biglinux-microphone-cli");
        let program = cli.to_str().ok_or_else(|| io::Error::other("invalid executable path"))?;
        let mut child = BigSubprocessSpec::builder().program(program)
            .args(["preview-quantum", &frames.to_string()])
            .stdin(Vec::new())
            .stdout(BigSubprocessOutputMode::Capture)
            .stderr(BigSubprocessOutputMode::Null)
            .allow_list([program]).build().spawn().map_err(io::Error::other)?;
        let stdout = child.take_stdout().ok_or_else(|| io::Error::other("preview stdout missing"))?;
        let mut preview = Self { cancellation: child.take_stdin(), child: Some(child) };
        if !poll_readable(stdout.as_raw_fd(), Duration::from_secs(6))? {
            preview.stop()?;
            return Err(io::Error::new(io::ErrorKind::TimedOut, "preview did not start"));
        }
        let mut ready = String::new();
        io::BufReader::new(stdout).read_line(&mut ready)?;
        if ready.trim() != "READY" {
            preview.stop()?;
            return Err(io::Error::other("the audio system could not start a preview"));
        }
        Ok(preview)
    }

    pub fn is_alive(&mut self) -> bool {
        self.child.as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    /// Cancel and reap on a worker. The child restores the previous override.
    pub fn stop(&mut self) -> io::Result<()> {
        self.cancellation.take();
        self.child.take().map_or(Ok(()), finish_child)
    }
}

impl Drop for QuantumPreview {
    fn drop(&mut self) {
        self.cancellation.take();
        if let Some(child) = self.child.take() {
            // GIO's shared pool owns the blocking reap; widget destruction
            // must not wait for a backend command to finish.
            let _ = gio::spawn_blocking(move || {
                if let Err(error) = finish_child(child) {
                    log::warn!("preview cleanup: {error}");
                }
            });
        }
    }
}

fn finish_child(mut child: BigSubprocessChild) -> io::Result<()> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(io::Error::other)? {
            return if status.success() { Ok(()) } else { Err(io::Error::other("preview restoration failed")) };
        }
        if started.elapsed() >= Duration::from_secs(6) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "preview cleanup timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Entry point used only by the GUI-owned child.
pub fn run_child(argument: Option<String>) -> ExitCode {
    let result = (|| -> io::Result<()> {
        let frames = argument.and_then(|value| value.parse::<u32>().ok())
            .filter(|value| VALUES.contains(value))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unsupported preview quantum"))?;
        let lock_path = crate::config::settings_file().with_file_name("quantum-preview.lock");
        let _guard = crate::config::storage::SettingsLock::at(&lock_path, Duration::ZERO)?;
        let previous = current_quantum()?;
        set_quantum(frames)?;
        let waiting = (|| -> io::Result<()> {
            println!("READY");
            io::stdout().flush()?;
            poll_readable(io::stdin().as_raw_fd(), LIFETIME)?;
            Ok(())
        })();
        // Do not erase an explicit override set by another application while
        // the user was listening. There is no compare-and-set in this API.
        let restored = if current_quantum()? == frames {
            set_quantum(previous)
        } else { Ok(()) };
        restored.and(waiting)
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => { eprintln!("preview: {error}"); ExitCode::FAILURE }
    }
}

fn poll_readable(fd: std::os::fd::RawFd, timeout: Duration) -> io::Result<bool> {
    let started = Instant::now();
    loop {
        let left = timeout.saturating_sub(started.elapsed());
        let millis = i32::try_from(left.as_millis()).unwrap_or(i32::MAX);
        let mut descriptor = libc::pollfd { fd, events: libc::POLLIN | libc::POLLHUP, revents: 0 };
        // SAFETY: descriptor is one initialized pollfd and remains valid for
        // the call; fd is borrowed from a live stream, never closed here.
        let result = unsafe { libc::poll(&raw mut descriptor, 1, millis) };
        if result >= 0 { return Ok(result > 0); }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted { return Err(error); }
        if started.elapsed() >= timeout { return Ok(false); }
    }
}

fn current_quantum() -> io::Result<u32> {
    let output = BigSubprocessSpec::builder().program("/usr/bin/pw-dump")
        .allow_list(["/usr/bin/pw-dump"]).timeout(Duration::from_secs(2))
        .stderr(BigSubprocessOutputMode::Null).build().run().map_err(io::Error::other)?;
    if !output.status.success() { return Err(io::Error::other("PipeWire is unavailable")); }
    let graph: Vec<Value> = serde_json::from_slice(&output.stdout).map_err(io::Error::other)?;
    quantum_from_graph(&graph).ok_or_else(|| io::Error::other("PipeWire settings metadata is unavailable"))
}

fn quantum_from_graph(graph: &[Value]) -> Option<u32> {
    let metadata = graph.iter().find(|object| object["type"] == "PipeWire:Interface:Metadata"
        && (object["props"]["metadata.name"] == "settings" || object["info"]["props"]["metadata.name"] == "settings"))?;
    let value = metadata["metadata"].as_array()?.iter()
        .find(|entry| entry["key"] == "clock.force-quantum");
    match value {
        None => Some(0),
        Some(entry) => entry["value"].as_u64().and_then(|value| u32::try_from(value).ok())
            .or_else(|| entry["value"].as_str()?.parse().ok()),
    }
}

fn set_quantum(frames: u32) -> io::Result<()> {
    let output = BigSubprocessSpec::builder().program("/usr/bin/pw-metadata")
        .args(["-n", "settings", "0", "clock.force-quantum", &frames.to_string()])
        .allow_list(["/usr/bin/pw-metadata"]).timeout(Duration::from_secs(2))
        .stdout(BigSubprocessOutputMode::Null).stderr(BigSubprocessOutputMode::Null)
        .build().run().map_err(io::Error::other)?;
    if output.status.success() { Ok(()) } else { Err(io::Error::other("the audio buffer could not be changed")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_override_means_release_not_a_guessed_distribution_default() {
        let graph = serde_json::json!([{"type":"PipeWire:Interface:Metadata", "props":{"metadata.name":"settings"}, "metadata":[]}]);
        assert_eq!(quantum_from_graph(graph.as_array().unwrap()), Some(0));
        assert_eq!(quantum_from_graph(&[]), None);
    }
    #[test]
    fn previous_explicit_override_is_preserved() {
        let graph = serde_json::json!([{"type":"PipeWire:Interface:Metadata", "props":{"metadata.name":"settings"}, "metadata":[{"key":"clock.force-quantum","value":"512"}]}]);
        assert_eq!(quantum_from_graph(graph.as_array().unwrap()), Some(512));
    }
}
''')
    file = Path('src/services.rs'); file.write_text(file.read_text() + '\npub mod preview;\n')
    replace('src/bin/cli.rs', '    MeasureModel,', '    MeasureModel,\n    PreviewQuantum,')
    replace('src/bin/cli.rs', '            "measure-model" => Self::MeasureModel,', '            "measure-model" => Self::MeasureModel,\n            "preview-quantum" => Self::PreviewQuantum,')
    replace('src/bin/cli.rs', '            Self::MeasureModel => {', '            Self::PreviewQuantum => biglinux_microphone::services::preview::run_child(args.next()),\n            Self::MeasureModel => {')
    commit(TITLE, ['src/services.rs', 'src/services/preview.rs', 'src/bin/cli.rs'])
