//! pw-cat subprocess capture.
//!
//! Starts `pw-cat --record` with raw 32-bit float output and feeds the
//! bytes into a ring buffer that drives the [`super::analyzer::Analyzer`].
//!
//! Subprocess lifecycle:
//!
//! - `spawn` creates the child and wraps `stdout` in a `BufReader`.
//! - `Capture::pump` pulls exactly `hop_size` fresh samples,
//!   slides them into the internal ring buffer, and reports read errors
//!   so the caller can emit `Event::Fatal` and stop.
//! - Dropping the [`Capture`] kills the child so we never leave orphan
//!   `pw-cat` processes behind.

use std::io::{self, BufReader, Read};
use std::process::ChildStdout;

use big_os_kit::subprocess::{BigSubprocessChild, BigSubprocessOutputMode, BigSubprocessSpec};
use log::debug;

/// **What this stream says about itself, and why it has to say anything.**
///
/// The spectrum reads the microphone for as long as this window is open. In the
/// graph that is a recorder like any other: the desktop's own "something is
/// listening to you" indicator counts capture streams, and without a mark this
/// one lights it for the whole session — the analyser somebody opened to LOOK at
/// their microphone reported as somebody recording them. The mark is the same
/// one the shell's own level meters carry (`media.role=monitor`), which is what
/// its classifier already excludes, plus the identity so a person reading
/// `pw-top` or the sound centre sees whose stream it is instead of "pw-cat".
const MARKED_AS_A_METER: &str = concat!(
    "{ media.role=monitor media.category=Monitor",
    " application.id=br.com.biglinux.microphone",
    " application.name=\"Filter noise\"",
    " node.name=biglinux-microphone.meter.spectrum",
    " node.description=\"Filter noise — analisador de espectro\" }",
);

/// The command that reads the default input, marked as the meter it is.
fn spec(sample_rate: u32) -> BigSubprocessSpec {
    BigSubprocessSpec::builder()
        .program("/usr/bin/pw-cat")
        .args([
            "--record",
            "-",
            "--target",
            "@DEFAULT_SOURCE@",
            "--raw",
            "--format",
            "f32",
            "--rate",
            &sample_rate.to_string(),
            "--channels",
            "1",
            "-P",
            MARKED_AS_A_METER,
        ])
        .stdout(BigSubprocessOutputMode::Capture)
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list(["/usr/bin/pw-cat"])
        .build()
}

/// Live capture handle.
pub(super) struct Capture {
    child: std::sync::Arc<std::sync::Mutex<BigSubprocessChild>>,
    stdout: BufReader<ChildStdout>,
    ring: Vec<f32>,
    read_buf: Vec<u8>,
    write_pos: usize,
    fft_size: usize,
    hop_size: usize,
    /// Samples produced since construction; used by
    /// [`Self::ready`] to decide whether the ring is
    /// filled for the first time.
    samples_read: usize,
}

impl Capture {
    /// Spawn `pw-cat` and return a handle that yields `hop_size`-sized
    /// sample slices sliding over a `fft_size` buffer.
    pub(super) fn spawn(sample_rate: u32, fft_size: usize, hop_size: usize) -> io::Result<Self> {
        assert!(hop_size > 0 && hop_size <= fft_size);

        let mut child = spec(sample_rate).spawn().map_err(io::Error::other)?;

        let Some(stdout) = child.take_stdout() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("pw-cat: no stdout handle"));
        };

        debug!(
            "audio monitor: spawned pw-cat target=@DEFAULT_SOURCE@ rate={sample_rate} fft={fft_size}",
        );

        Ok(Self {
            child: std::sync::Arc::new(std::sync::Mutex::new(child)),
            stdout: BufReader::with_capacity(fft_size * 4, stdout),
            ring: vec![0.0; fft_size],
            read_buf: vec![0; hop_size * 4],
            write_pos: 0,
            fft_size,
            hop_size,
            samples_read: 0,
        })
    }

    /// Cloneable process handle used by the monitor owner to interrupt a
    /// blocking stdout read during shutdown.
    pub(super) fn cancellation_handle(
        &self,
    ) -> std::sync::Arc<std::sync::Mutex<BigSubprocessChild>> {
        self.child.clone()
    }

    /// True when at least one full window of audio has accumulated and a
    /// call to [`Self::copy_window_into`] will produce meaningful data.
    #[must_use]
    pub(super) fn ready(&self) -> bool {
        self.samples_read >= self.fft_size
    }

    /// Read the next `hop_size` samples from `pw-cat` and slide them into
    /// the ring buffer. Returns the number of samples actually read (0
    /// only on EOF).
    pub(super) fn pump(&mut self) -> io::Result<usize> {
        self.stdout.read_exact(&mut self.read_buf).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                io::Error::new(io::ErrorKind::BrokenPipe, "pw-cat closed stdout")
            } else {
                e
            }
        })?;

        for chunk in self.read_buf.chunks_exact(4) {
            let sample = f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            self.ring[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % self.fft_size;
            self.samples_read = self.samples_read.saturating_add(1);
        }
        Ok(self.hop_size)
    }

    /// Copy into reusable caller-owned storage in chronological order.
    pub(super) fn copy_window_into(&self, out: &mut Vec<f32>) {
        out.clear();
        out.extend_from_slice(&self.ring[self.write_pos..]);
        out.extend_from_slice(&self.ring[..self.write_pos]);
    }

}

impl Drop for Capture {
    fn drop(&mut self) {
        // Best-effort shutdown: try TERM via `kill`, ignore errors
        // (child already exited, permission denied, …). Orphaned pw-cat
        // processes are the sole thing we're protecting against.
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MARKED_AS_A_METER, spec};

    /// The mark travels with the command, and it has to be exactly the one the
    /// shell's classifier looks for. Checked here because the alternative is
    /// finding out from a desktop that says somebody is recording you whenever
    /// this window is open — which is what happened before the mark existed.
    #[test]
    fn the_spectrum_stream_says_it_is_only_a_meter() {
        let argv = spec(48_000).resolved().argv;
        let marks = argv
            .iter()
            .position(|arg| arg == "-P")
            .map(|at| argv[at + 1].clone())
            .expect("the properties are passed");
        assert!(marks.contains("media.role=monitor"), "{marks}");
        assert!(
            marks.contains("application.id=br.com.biglinux.microphone"),
            "{marks}"
        );
        assert!(
            marks.contains("node.name=biglinux-microphone.meter.spectrum"),
            "{marks}"
        );
        assert_eq!(marks, MARKED_AS_A_METER);
        // Still reading the default input, and still raw floats: the mark must
        // not have changed what the analyser is fed.
        assert!(argv.contains(&String::from("@DEFAULT_SOURCE@")));
        assert!(argv.contains(&String::from("f32")));
    }
}
