//! pw-cat subprocess capture.
//!
//! Starts `pw-cat --record` with raw 32-bit float output and feeds the
//! bytes into a ring buffer that drives the [`super::analyzer::Analyzer`].
//!
//! Subprocess lifecycle:
//!
//! - `spawn` creates the child and wraps `stdout` in a `BufReader`.
//! - `Capture::pump_one_frame` pulls exactly `hop_size` fresh samples,
//!   slides them into the internal ring buffer, and returns a borrowed
//!   window ready for the analyser. When the child exits mid-read the
//!   method returns an IO error; the caller is expected to propagate
//!   it as an `Event::Fatal` and stop.
//! - Dropping the [`Capture`] kills the child so we never leave orphan
//!   `pw-cat` processes behind.

use std::io::{self, BufReader, Read};
use std::process::{Child, ChildStdout, Command, Stdio};

use log::debug;

/// Target identifier passed to `pw-cat --target`.
#[derive(Debug, Clone)]
pub enum CaptureTarget {
    /// Read from whatever PipeWire considers the default source. The
    /// mic filter-chain (when loaded) replaces the hardware default, so
    /// this automatically captures the *processed* signal — exactly
    /// what the UI's spectrum should show.
    DefaultSource,
    /// Read from a specific node by its `node.name` property.
    NodeName(String),
}

impl CaptureTarget {
    fn as_arg(&self) -> String {
        match self {
            Self::DefaultSource => "@DEFAULT_SOURCE@".to_owned(),
            Self::NodeName(name) => name.clone(),
        }
    }
}

/// Live capture handle.
pub struct Capture {
    child: Child,
    stdout: BufReader<ChildStdout>,
    ring: Vec<f32>,
    write_pos: usize,
    fft_size: usize,
    hop_size: usize,
    /// Samples produced since construction; used by
    /// [`Self::ready_for_first_frame`] to decide whether the ring is
    /// filled for the first time.
    samples_read: usize,
}

impl Capture {
    /// Spawn `pw-cat` and return a handle that yields `hop_size`-sized
    /// sample slices sliding over a `fft_size` buffer.
    pub fn spawn(
        target: &CaptureTarget,
        sample_rate: u32,
        fft_size: usize,
        hop_size: usize,
    ) -> io::Result<Self> {
        assert!(hop_size > 0 && hop_size <= fft_size);

        let mut child = Command::new("pw-cat")
            .args([
                "--record",
                "-",
                "--target",
                &target.as_arg(),
                "--raw",
                "--format",
                "f32",
                "--rate",
                &sample_rate.to_string(),
                "--channels",
                "1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("pw-cat: no stdout handle"))?;

        debug!(
            "audio monitor: spawned pw-cat target={} rate={} fft={}",
            target.as_arg(),
            sample_rate,
            fft_size,
        );

        Ok(Self {
            child,
            stdout: BufReader::with_capacity(fft_size * 4, stdout),
            ring: vec![0.0; fft_size],
            write_pos: 0,
            fft_size,
            hop_size,
            samples_read: 0,
        })
    }

    /// True when at least one full window of audio has accumulated and a
    /// call to [`Self::window_snapshot`] will produce meaningful data.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.samples_read >= self.fft_size
    }

    /// Read the next `hop_size` samples from `pw-cat` and slide them into
    /// the ring buffer. Returns the number of samples actually read (0
    /// only on EOF).
    pub fn pump(&mut self) -> io::Result<usize> {
        let mut buf = [0_u8; 4 * 4096];
        let want = (self.hop_size).min(buf.len() / 4);
        let byte_count = want * 4;
        self.stdout
            .read_exact(&mut buf[..byte_count])
            .map_err(|e| {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    io::Error::new(io::ErrorKind::BrokenPipe, "pw-cat closed stdout")
                } else {
                    e
                }
            })?;

        for chunk in buf[..byte_count].chunks_exact(4) {
            let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            self.ring[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % self.fft_size;
            self.samples_read = self.samples_read.saturating_add(1);
        }
        Ok(want)
    }

    /// Copy the current ring buffer contents into a linear slice ordered
    /// oldest → newest. Allocates once per frame; at 94 Hz on 2048-float
    /// windows that's ~770 KiB/s, negligible for a desktop app.
    #[must_use]
    pub fn window_snapshot(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.fft_size);
        out.extend_from_slice(&self.ring[self.write_pos..]);
        out.extend_from_slice(&self.ring[..self.write_pos]);
        out
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // Best-effort shutdown: try TERM via `kill`, ignore errors
        // (child already exited, permission denied, …). Orphaned pw-cat
        // processes are the sole thing we're protecting against.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_default_source_uses_atom() {
        assert_eq!(CaptureTarget::DefaultSource.as_arg(), "@DEFAULT_SOURCE@");
    }

    #[test]
    fn target_node_name_passes_through() {
        let t = CaptureTarget::NodeName("mic-biglinux".into());
        assert_eq!(t.as_arg(), "mic-biglinux");
    }

    // The spawn path requires pw-cat on PATH and a live PipeWire
    // session — exercised by the CLI `spectrum` command end-to-end.
}
