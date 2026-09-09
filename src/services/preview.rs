//! A reversible 15-second graph-quantum preview.
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
// Three attempts have at most three two-second commands each, plus backoff.
// The parent must not kill the child before that restoration budget expires.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(25);
const RESTORE_ATTEMPTS: usize = 3;
const VALUES: &[u32] = &[0, 256, 512, 1024, 2048, 4096];

pub struct QuantumPreview {
    child: Option<BigSubprocessChild>,
    cancellation: Option<ChildStdin>,
}

impl QuantumPreview {
    /// Blocking startup; invoke on a worker, never from a GTK signal handler.
    pub fn start(frames: u32) -> io::Result<Self> {
        if !VALUES.contains(&frames) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported preview quantum",
            ));
        }
        let cli = std::env::current_exe()?.with_file_name("biglinux-microphone-cli");
        let program = cli
            .to_str()
            .ok_or_else(|| io::Error::other("invalid executable path"))?;
        let mut child = BigSubprocessSpec::builder()
            .program(program)
            .args(["preview-quantum", &frames.to_string()])
            .stdin(Vec::new())
            .stdout(BigSubprocessOutputMode::Capture)
            .stderr(BigSubprocessOutputMode::Null)
            .allow_list([program])
            .build()
            .spawn()
            .map_err(io::Error::other)?;
        let stdout = child
            .take_stdout()
            .ok_or_else(|| io::Error::other("preview stdout missing"))?;
        let mut preview = Self {
            cancellation: child.take_stdin(),
            child: Some(child),
        };
        if !poll_readable(stdout.as_raw_fd(), Duration::from_secs(6))? {
            preview.stop()?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "preview did not start",
            ));
        }
        let mut ready = String::new();
        io::BufReader::new(stdout).read_line(&mut ready)?;
        if ready.trim() != "READY" {
            preview.stop()?;
            return Err(io::Error::other(
                "the audio system could not start a preview",
            ));
        }
        Ok(preview)
    }

    /// Nonblocking completion with the restoration status intact.
    pub fn completion(&mut self) -> Option<io::Result<()>> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => {
                self.child.take();
                self.cancellation.take();
                Some(completion_status(status.success()))
            }
            Err(error) => Some(Err(io::Error::other(error))),
        }
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
            drop(gio::spawn_blocking(move || {
                if let Err(error) = finish_child(child) {
                    log::warn!("preview cleanup: {error}");
                }
            }));
        }
    }
}

fn finish_child(mut child: BigSubprocessChild) -> io::Result<()> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(io::Error::other)? {
            return completion_status(status.success());
        }
        if started.elapsed() >= CLEANUP_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "preview cleanup timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Entry point used only by the GUI-owned child.
pub fn run_child(argument: Option<String>) -> ExitCode {
    let result = (|| -> io::Result<()> {
        let frames = argument
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| VALUES.contains(value))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "unsupported preview quantum")
            })?;
        let lock_path = crate::config::settings_file().with_file_name("quantum-preview.lock");
        let _guard = crate::config::storage::SettingsLock::at(&lock_path, Duration::ZERO)?;
        let previous = current_quantum()?;
        if let Err(error) = set_quantum(frames) {
            // A timed-out command may already have changed metadata.
            // Restore only after confirming the value is still ours.
            let restored = restore_override(previous, frames);
            return restored.and(Err(error));
        }
        let waiting = (|| -> io::Result<()> {
            println!("READY");
            io::stdout().flush()?;
            poll_readable(io::stdin().as_raw_fd(), LIFETIME)?;
            Ok(())
        })();
        restore_override(previous, frames).and(waiting)
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("preview: {error}");
            ExitCode::FAILURE
        }
    }
}

fn completion_status(success: bool) -> io::Result<()> {
    if success {
        Ok(())
    } else {
        Err(io::Error::other("preview restoration failed"))
    }
}

fn restore_override(previous: u32, temporary: u32) -> io::Result<()> {
    restore_with(
        previous,
        temporary,
        current_quantum,
        set_quantum,
        std::thread::sleep,
    )
}

/// Retrying a read or write does not grant ownership over another app's
/// override. Every write is preceded by a fresh comparison, and verified.
fn restore_with(
    previous: u32,
    temporary: u32,
    mut read: impl FnMut() -> io::Result<u32>,
    mut write: impl FnMut(u32) -> io::Result<()>,
    mut pause: impl FnMut(Duration),
) -> io::Result<()> {
    let mut failure = io::Error::other("preview restoration was not confirmed");
    for attempt in 0..RESTORE_ATTEMPTS {
        let result = (|| {
            if read()? != temporary {
                return Ok(());
            }
            write(previous)?;
            if read()? == temporary && previous != temporary {
                return Err(io::Error::other("temporary audio buffer is still active"));
            }
            Ok(())
        })();
        match result {
            Ok(()) => return Ok(()),
            Err(error) => failure = error,
        }
        if attempt + 1 < RESTORE_ATTEMPTS {
            pause(Duration::from_millis(150));
        }
    }
    Err(failure)
}

fn poll_readable(fd: std::os::fd::RawFd, timeout: Duration) -> io::Result<bool> {
    let started = Instant::now();
    loop {
        let left = timeout.saturating_sub(started.elapsed());
        let millis = i32::try_from(left.as_millis()).unwrap_or(i32::MAX);
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        // SAFETY: descriptor is one initialized pollfd and remains valid for
        // the call; fd is borrowed from a live stream, never closed here.
        let result = unsafe { libc::poll(&raw mut descriptor, 1, millis) };
        if result >= 0 {
            return Ok(result > 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        if started.elapsed() >= timeout {
            return Ok(false);
        }
    }
}

fn current_quantum() -> io::Result<u32> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-dump")
        .allow_list(["/usr/bin/pw-dump"])
        .timeout(Duration::from_secs(2))
        .stderr(BigSubprocessOutputMode::Null)
        .build()
        .run()
        .map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other("PipeWire is unavailable"));
    }
    let graph: Vec<Value> = serde_json::from_slice(&output.stdout).map_err(io::Error::other)?;
    quantum_from_graph(&graph)
        .ok_or_else(|| io::Error::other("PipeWire settings metadata is unavailable"))
}

fn quantum_from_graph(graph: &[Value]) -> Option<u32> {
    let metadata = graph.iter().find(|object| {
        object["type"] == "PipeWire:Interface:Metadata"
            && (object["props"]["metadata.name"] == "settings"
                || object["info"]["props"]["metadata.name"] == "settings")
    })?;
    let value = metadata["metadata"]
        .as_array()?
        .iter()
        .find(|entry| entry["key"] == "clock.force-quantum");
    match value {
        None => Some(0),
        Some(entry) => entry["value"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .or_else(|| entry["value"].as_str()?.parse().ok()),
    }
}

fn set_quantum(frames: u32) -> io::Result<()> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-metadata")
        .args([
            "-n",
            "settings",
            "0",
            "clock.force-quantum",
            &frames.to_string(),
        ])
        .allow_list(["/usr/bin/pw-metadata"])
        .timeout(Duration::from_secs(2))
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .build()
        .run()
        .map_err(io::Error::other)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other("the audio buffer could not be changed"))
    }
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

#[cfg(test)]
mod restoration_tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn transient_read_failure_does_not_abandon_the_override() {
        let value = Cell::new(256);
        let reads = Cell::new(0);
        restore_with(
            1024,
            256,
            || {
                reads.set(reads.get() + 1);
                if reads.get() == 1 {
                    Err(io::Error::other("temporarily unavailable"))
                } else {
                    Ok(value.get())
                }
            },
            |v| {
                value.set(v);
                Ok(())
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(value.get(), 1024);
    }

    #[test]
    fn another_app_override_is_never_replaced_during_retry() {
        let reads = Cell::new(0);
        restore_with(
            1024,
            256,
            || {
                reads.set(reads.get() + 1);
                if reads.get() == 1 {
                    Err(io::Error::other("busy"))
                } else {
                    Ok(512)
                }
            },
            |_| panic!("external override must be preserved"),
            |_| {},
        )
        .unwrap();
    }

    #[test]
    fn failed_write_is_retried_and_read_back() {
        let value = Cell::new(256);
        let writes = Cell::new(0);
        restore_with(
            1024,
            256,
            || Ok(value.get()),
            |v| {
                writes.set(writes.get() + 1);
                if writes.get() == 1 {
                    Err(io::Error::other("temporary write failure"))
                } else {
                    value.set(v);
                    Ok(())
                }
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(writes.get(), 2);
        assert_eq!(value.get(), 1024);
    }

    #[test]
    fn persistent_failure_is_bounded_and_remains_an_error() {
        let reads = Cell::new(0);
        assert!(
            restore_with(
                1024,
                256,
                || {
                    reads.set(reads.get() + 1);
                    Err(io::Error::other("unavailable"))
                },
                |_| Ok(()),
                |_| {}
            )
            .is_err()
        );
        assert_eq!(reads.get(), RESTORE_ATTEMPTS);
        assert!(completion_status(false).is_err());
        assert!(completion_status(true).is_ok());
    }
}
