from pathlib import Path
from _common import done, replace, write, commit

TITLE = 'fix(subprocess): enforce allow lists and bound the complete pipe lifecycle'
if not done(TITLE):
    root = 'vendor/big-rust-components/crates/foundation/big-os-kit/src/'
    path = root + 'subprocess.rs'
    replace(path, 'self.allowed.is_empty() || self.allowed.contains(program)', 'self.allowed.contains(program)')
    text = Path(path).read_text()
    text = text.replace('fn run_resolved(\n', '#[cfg(not(unix))]\nfn run_resolved(\n')
    text = text.replace('enum WaitOutcome {', '#[cfg(not(unix))]\nenum WaitOutcome {')
    text = text.replace('fn drain_capped<R:', '#[cfg(any(test, not(unix)))]\nfn drain_capped<R:')
    pos = text.index('// Reject NUL, LF, CR, DEL')
    text = text[:pos] + '''#[cfg(unix)]
mod unix_exec;
#[cfg(unix)]
use unix_exec::run_resolved;

''' + text[pos:]
    old = '''        let detached_child = cmd.spawn().map_err(BigSubprocessError::Spawn)?;
        notify_spawn_observer(detached_child.id(), &self.program);
        Ok(())'''
    assert text.count(old) == 1
    text = text.replace(old, '''        let child = cmd.spawn().map_err(BigSubprocessError::Spawn)?;
        notify_spawn_observer(child.id(), &self.program);
        let child = std::sync::Arc::new(std::sync::Mutex::new(Some(child)));
        let worker_child = std::sync::Arc::clone(&child);
        let reaper = std::thread::Builder::new().name("subprocess-reaper".into()).spawn(move || {
            if let Ok(mut slot) = worker_child.lock() && let Some(mut child) = slot.take() {
                let _ = child.wait();
            }
        });
        if let Err(error) = reaper {
            // Failed thread creation must not leak an unowned child.
            if let Ok(mut slot) = child.lock() && let Some(mut child) = slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Err(BigSubprocessError::Io(error));
        }
        Ok(())''')
    text = text.replace('capture modes; the child is not reaped by this process.', 'capture modes; a dedicated reaper collects its exit status.')
    # Bytewise matching also protects logs with a malformed UTF-8 prefix.
    start = text.index('fn redact_in_place(')
    end = text.index('\n#[cfg(test)]', start)
    text = text[:start] + '''fn redact_in_place(needles: &[String], buf: &mut Vec<u8>) {
    for needle in needles.iter().filter(|needle| !needle.is_empty()) {
        let needle = needle.as_bytes();
        let mut result = Vec::with_capacity(buf.len());
        let mut offset = 0;
        while offset < buf.len() {
            if buf[offset..].starts_with(needle) {
                result.extend_from_slice(b"[REDACTED]");
                offset += needle.len();
            } else {
                result.push(buf[offset]);
                offset += 1;
            }
        }
        *buf = result;
    }
}
''' + text[end:]
    # Distinguish truncation from a successful command's complete output.
    text = text.replace('    Timeout,', '''    Timeout,
    /// Captured output exceeded its configured safety budget.
    #[error("subprocess {stream} exceeded the {limit}-byte capture limit")]
    CaptureLimit { stream: &'static str, limit: usize },''', 1)
    Path(path).write_text(text)
    write(root + 'subprocess/unix_exec.rs', r'''//! Nonblocking Unix subprocess I/O with a single end-to-end deadline.
//!
//! No writer can block before output draining begins and no reader join can
//! outlive the deadline. The child owns a fresh process group. Its PID stays
//! unreaped until every pipe closes, so timeout cleanup cannot accidentally
//! signal a reused process-group ID while descendants keep those pipes open.

use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::{BigSubprocessError, BigSubprocessOutput, BigSubprocessResolved, BigSubprocessSpec,
    BigSubprocessStatus, DEFAULT_MAX_CAPTURE_BYTES, build_command, notify_spawn_observer, redact_in_place};

struct OwnedChild {
    child: Child,
    finished: bool,
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.finished {
            if let Ok(group) = i32::try_from(self.child.id()) {
                // SAFETY: process_group(0) below creates a group whose ID is
                // the live, unreaped child PID. A negative PID targets only
                // that owned group, never the caller's group.
                unsafe { libc::kill(-group, libc::SIGKILL) };
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub(super) fn run_resolved(
    spec: &BigSubprocessSpec,
    _resolved: &BigSubprocessResolved,
    cancel: &AtomicBool,
) -> Result<BigSubprocessOutput, BigSubprocessError> {
    if cancel.load(Ordering::Acquire) { return Err(BigSubprocessError::Cancelled); }
    let started = Instant::now();
    let mut command = build_command(spec);
    command.process_group(0);
    let child = command.spawn().map_err(BigSubprocessError::Spawn)?;
    notify_spawn_observer(child.id(), &spec.program);
    let mut child = OwnedChild { child, finished: false };
    let mut input = child.child.stdin.take();
    let mut output = child.child.stdout.take();
    let mut errors = child.child.stderr.take();
    for fd in [input.as_ref().map(AsRawFd::as_raw_fd), output.as_ref().map(AsRawFd::as_raw_fd), errors.as_ref().map(AsRawFd::as_raw_fd)].into_iter().flatten() {
        nonblocking(fd).map_err(BigSubprocessError::Io)?;
    }
    let bytes = spec.stdin.as_deref().unwrap_or_default();
    let mut sent = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status: ExitStatus = loop {
        if cancel.load(Ordering::Acquire) { return Err(BigSubprocessError::Cancelled); }
        if started.elapsed() >= spec.timeout { return Err(BigSubprocessError::Timeout); }
        write_input(&mut input, bytes, &mut sent).map_err(BigSubprocessError::Io)?;
        read_available(&mut output, &mut stdout, "stdout")?;
        read_available(&mut errors, &mut stderr, "stderr")?;
        // Do not reap while a descendant can still be holding a captured FD.
        if input.is_none() && output.is_none() && errors.is_none()
            && let Some(status) = child.child.try_wait().map_err(BigSubprocessError::Io)? {
                child.finished = true;
                break status;
            }
        let remaining = spec.timeout.saturating_sub(started.elapsed()).min(Duration::from_millis(20));
        let descriptors = [
            input.as_ref().map(|pipe| (pipe.as_raw_fd(), libc::POLLOUT)),
            output.as_ref().map(|pipe| (pipe.as_raw_fd(), libc::POLLIN)),
            errors.as_ref().map(|pipe| (pipe.as_raw_fd(), libc::POLLIN)),
        ];
        poll(&descriptors, remaining).map_err(BigSubprocessError::Io)?;
    };
    redact_in_place(&spec.redact_substrings, &mut stdout);
    redact_in_place(&spec.redact_substrings, &mut stderr);
    Ok(BigSubprocessOutput { status: BigSubprocessStatus::Exited(status.code()), stdout, stderr })
}

fn nonblocking(fd: std::os::fd::RawFd) -> io::Result<()> {
    // SAFETY: fd is borrowed from a live Child pipe. F_GETFL takes no third
    // argument; F_SETFL receives the old flags plus O_NONBLOCK.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 { return Err(io::Error::last_os_error()); }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn write_input(pipe: &mut Option<ChildStdin>, bytes: &[u8], sent: &mut usize) -> io::Result<()> {
    if *sent == bytes.len() { pipe.take(); return Ok(()); }
    let Some(writer) = pipe.as_mut() else { return Ok(()); };
    let end = sent.saturating_add(16 * 1024).min(bytes.len());
    match writer.write(&bytes[*sent..end]) {
        Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "subprocess input closed")),
        Ok(count) => *sent += count,
        Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
        // A command may deliberately stop consuming input; still collect its
        // status and diagnostics rather than abandoning process cleanup.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => { pipe.take(); }
        Err(error) => return Err(error),
    }
    if *sent == bytes.len() { pipe.take(); }
    Ok(())
}

fn read_available<R: Read>(pipe: &mut Option<R>, buffer: &mut Vec<u8>, stream: &'static str) -> Result<(), BigSubprocessError> {
    let Some(reader) = pipe.as_mut() else { return Ok(()); };
    let mut chunk = [0_u8; 8192];
    // A prolific child cannot starve the other pipe or the timeout check.
    for _ in 0..8 {
        match reader.read(&mut chunk) {
            Ok(0) => { pipe.take(); return Ok(()); }
            Ok(count) => {
                if count > DEFAULT_MAX_CAPTURE_BYTES.saturating_sub(buffer.len()) {
                    return Err(BigSubprocessError::CaptureLimit { stream, limit: DEFAULT_MAX_CAPTURE_BYTES });
                }
                buffer.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(BigSubprocessError::Io(error)),
        }
    }
    Ok(())
}

fn poll(descriptors: &[Option<(std::os::fd::RawFd, i16)>; 3], duration: Duration) -> io::Result<()> {
    let mut fds = descriptors.map(|descriptor| {
        let (fd, events) = descriptor.unwrap_or((-1, 0));
        libc::pollfd { fd, events, revents: 0 }
    });
    let timeout = i32::try_from(duration.as_millis()).unwrap_or(20).max(1);
    // SAFETY: fds contains exactly three initialized descriptors, owned by
    // streams that stay live until the call returns. Negative FDs are ignored.
    let result = unsafe { libc::poll(fds.as_mut_ptr(), 3, timeout) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted { return Err(error); }
    }
    Ok(())
}
''')
    write('tests/review_subprocess.rs', r'''//! Real-child regressions for the shared subprocess boundary.
#![cfg(not(miri))]
use std::time::{Duration, Instant};
use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessSpec};

#[test]
fn an_explicit_empty_allow_list_denies_execution() {
    let spec = BigSubprocessSpec::builder().program("true").allow_list(Vec::<String>::new()).build();
    assert!(spec.try_resolved().is_err());
}

#[test]
fn a_child_that_never_reads_stdin_cannot_bypass_the_timeout() {
    let started = Instant::now();
    let result = BigSubprocessSpec::builder().program("sleep").arg("10")
        .stdin(vec![b'x'; 1024 * 1024]).timeout(Duration::from_millis(100)).build().run();
    assert!(matches!(result, Err(BigSubprocessError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn full_duplex_pipes_do_not_deadlock() {
    let payload = vec![b'x'; 512 * 1024];
    let result = BigSubprocessSpec::builder().program("cat")
        .stdin(payload.clone()).timeout(Duration::from_secs(3)).build().run().unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, payload);
}

#[test]
fn inherited_pipe_lifetimes_are_bounded_too() {
    let started = Instant::now();
    // Literal test fixture; no untrusted value is passed to the shell.
    let result = BigSubprocessSpec::builder().program("sh").args(["-c", "sleep 10 & exit 0"])
        .timeout(Duration::from_millis(100)).build().run();
    assert!(matches!(result, Err(BigSubprocessError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn oversized_output_is_a_reported_error_not_silent_truncation() {
    let result = BigSubprocessSpec::builder().program("head").args(["-c", "17000000", "/dev/zero"])
        .timeout(Duration::from_secs(5)).build().run();
    assert!(matches!(result, Err(BigSubprocessError::CaptureLimit { .. })));
}

#[test]
fn non_utf8_output_cannot_disable_secret_redaction() {
    let result = BigSubprocessSpec::builder().program("printf")
        .arg("\\377 test-token").redact("test-token").build().run().unwrap();
    assert_eq!(result.stdout, b"\xff [REDACTED]");
}
''')
    commit(TITLE, [path, root+'subprocess/unix_exec.rs', 'tests/review_subprocess.rs'])
