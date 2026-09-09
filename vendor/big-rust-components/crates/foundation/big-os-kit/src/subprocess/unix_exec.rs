//! Nonblocking subprocess communication with one deadline for the whole exchange.
//!
//! Process exit and stream EOF are independent events. In particular, a
//! descendant can retain stdout/stderr after the direct child exits. Never
//! return its status, close captured readers, or kill that descendant merely
//! because the direct child has exited. Drain to EOF or report the deadline.

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::{
    BigSubprocessError, BigSubprocessOutput, BigSubprocessResolved, BigSubprocessSpec,
    BigSubprocessStatus, DEFAULT_MAX_CAPTURE_BYTES, build_command, notify_spawn_observer,
    redact_in_place,
};

/// The direct child remains unreaped while any of its captured pipes is open.
/// That pins the group identity until timeout/error cleanup has sent SIGKILL.
struct OwnedChild {
    child: Child,
    reaped: bool,
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        if let Ok(group) = i32::try_from(self.child.id()) {
            // SAFETY: process_group(0) creates this child's own group. We
            // have not reaped its leader, so the group ID cannot be reused.
            unsafe { libc::kill(-group, libc::SIGKILL) };
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Capture<R> {
    reader: Option<R>,
    bytes: Vec<u8>,
    stream: &'static str,
}

impl<R: Read + AsRawFd> Capture<R> {
    fn new(reader: Option<R>, stream: &'static str) -> Result<Self, BigSubprocessError> {
        if let Some(pipe) = &reader {
            nonblocking(pipe.as_raw_fd()).map_err(BigSubprocessError::Io)?;
        }
        Ok(Self {
            reader,
            bytes: Vec::new(),
            stream,
        })
    }

    /// Only read(2) returning zero closes the reader. WouldBlock and HUP
    /// notifications are not EOF: buffered or future descendant data matters.
    fn drain(&mut self) -> Result<(), BigSubprocessError> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(());
        };
        let mut chunk = [0_u8; 8192];
        // Bound work per turn so prolific stdout cannot starve stderr,
        // stdin, cancellation, or the end-to-end deadline.
        for _ in 0..8 {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    self.reader.take();
                    break;
                }
                Ok(count) => {
                    if count > DEFAULT_MAX_CAPTURE_BYTES.saturating_sub(self.bytes.len()) {
                        return Err(BigSubprocessError::CaptureLimit {
                            stream: self.stream,
                            limit: DEFAULT_MAX_CAPTURE_BYTES,
                        });
                    }
                    self.bytes.extend_from_slice(&chunk[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(BigSubprocessError::Io(error)),
            }
        }
        Ok(())
    }

    fn descriptor(&self) -> Option<(RawFd, i16)> {
        self.reader
            .as_ref()
            .map(|pipe| (pipe.as_raw_fd(), libc::POLLIN))
    }
}

pub(super) fn run_resolved(
    spec: &BigSubprocessSpec,
    _resolved: &BigSubprocessResolved,
    cancel: &AtomicBool,
) -> Result<BigSubprocessOutput, BigSubprocessError> {
    if cancel.load(Ordering::Acquire) {
        return Err(BigSubprocessError::Cancelled);
    }
    let started = Instant::now();
    let mut command = build_command(spec);
    command.process_group(0);
    // Own cleanup before observers or any fallible pipe setup can run.
    let mut owned = OwnedChild {
        child: command.spawn().map_err(BigSubprocessError::Spawn)?,
        reaped: false,
    };
    notify_spawn_observer(owned.child.id(), &spec.program);
    let mut input = owned.child.stdin.take();
    if let Some(pipe) = &input {
        nonblocking(pipe.as_raw_fd()).map_err(BigSubprocessError::Io)?;
    }
    let mut stdout = Capture::new(owned.child.stdout.take(), "stdout")?;
    let mut stderr = Capture::new(owned.child.stderr.take(), "stderr")?;
    let bytes = spec.stdin.as_deref().unwrap_or_default();
    let mut sent = 0;

    let status = loop {
        if cancel.load(Ordering::Acquire) {
            return Err(BigSubprocessError::Cancelled);
        }
        if started.elapsed() >= spec.timeout {
            return Err(BigSubprocessError::Timeout);
        }
        write_input(&mut input, bytes, &mut sent).map_err(BigSubprocessError::Io)?;
        stdout.drain()?;
        stderr.drain()?;

        // Never poll/reap the direct child before finishing the communication
        // contract. Its exit is not permission to truncate its descendants.
        if input.is_none() && stdout.reader.is_none() && stderr.reader.is_none() {
            if let Some(status) = owned.child.try_wait().map_err(BigSubprocessError::Io)? {
                owned.reaped = true;
                break status;
            }
        }
        let remaining = spec.timeout.saturating_sub(started.elapsed());
        poll(
            [
                input.as_ref().map(|pipe| (pipe.as_raw_fd(), libc::POLLOUT)),
                stdout.descriptor(),
                stderr.descriptor(),
            ],
            remaining.min(Duration::from_millis(20)),
        )
        .map_err(BigSubprocessError::Io)?;
    };
    redact_in_place(&spec.redact_substrings, &mut stdout.bytes);
    redact_in_place(&spec.redact_substrings, &mut stderr.bytes);
    Ok(BigSubprocessOutput {
        status: BigSubprocessStatus::Exited(status.code()),
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

fn nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fd is borrowed from a live owned pipe; neither operation takes
    // ownership. Preserve all existing flags when enabling O_NONBLOCK.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn write_input(pipe: &mut Option<ChildStdin>, bytes: &[u8], sent: &mut usize) -> io::Result<()> {
    if *sent == bytes.len() {
        pipe.take();
        return Ok(());
    }
    let Some(writer) = pipe.as_mut() else {
        return Ok(());
    };
    let end = sent.saturating_add(16 * 1024).min(bytes.len());
    match writer.write(&bytes[*sent..end]) {
        Ok(0) => {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "subprocess input closed",
            ));
        }
        Ok(count) => *sent += count,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) => {}
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
            pipe.take();
        }
        Err(error) => return Err(error),
    }
    if *sent == bytes.len() {
        pipe.take();
    }
    Ok(())
}

fn poll(descriptors: [Option<(RawFd, i16)>; 3], duration: Duration) -> io::Result<()> {
    let mut fds = descriptors.map(|descriptor| {
        let (fd, events) = descriptor.unwrap_or((-1, 0));
        libc::pollfd {
            fd,
            events,
            revents: 0,
        }
    });
    let timeout = i32::try_from(duration.as_millis()).unwrap_or(20).max(1);
    // SAFETY: the array contains exactly three initialized entries. Their
    // owned pipes remain alive during poll; negative descriptors are ignored.
    let result = unsafe { libc::poll(fds.as_mut_ptr(), 3, timeout) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    } else if fds.iter().any(|fd| fd.revents & libc::POLLNVAL != 0) {
        return Err(io::Error::other("invalid owned subprocess pipe descriptor"));
    }
    Ok(())
}
