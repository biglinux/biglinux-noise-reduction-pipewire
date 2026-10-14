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
/// Our own analyser. Named so the consumer test that decides whether anybody is
/// recording can tell the meter apart from a real application — counting it
/// would let the meter be its own reason to keep running.
pub const METER_NODE_NAME: &str = "biglinux-microphone.meter.spectrum";

const MARKED_AS_A_METER: &str = concat!(
    "{ media.role=monitor media.category=Monitor",
    " application.id=br.com.biglinux.microphone",
    " application.name=\"Filter noise\"",
    " node.name=biglinux-microphone.meter.spectrum",
    " node.description=\"Filter noise — analisador de espectro\" }",
);

/// An explicitly selected AGC meter must never silently move to another source.
/// The default UI meter retains its existing automatic-routing behaviour.
fn meter_properties(target: &str) -> String {
    if matches!(
        target,
        "auto" | "@DEFAULT_SOURCE@" | "@DEFAULT_AUDIO_SOURCE@"
    ) {
        MARKED_AS_A_METER.to_string()
    } else {
        format!(
            "{} node.dont-fallback=true node.dont-reconnect=true node.dont-move=true }}",
            MARKED_AS_A_METER.trim_end_matches('}')
        )
    }
}

/// The command that reads `target` (a node name or `@DEFAULT_SOURCE@`), marked as
/// the meter it is.
fn spec(sample_rate: u32, target: &str, latency_ms: u32) -> BigSubprocessSpec {
    let target = match target {
        "@DEFAULT_SOURCE@" | "@DEFAULT_AUDIO_SOURCE@" => "auto",
        other => other,
    };
    BigSubprocessSpec::builder()
        .program("/usr/bin/pw-cat")
        .args([
            "--record",
            "-",
            "--target",
            target,
            "--raw",
            "--format",
            "f32",
            "--rate",
            &sample_rate.to_string(),
            "--latency",
            &format!("{latency_ms}ms"),
            "--channels",
            "1",
            "-P",
            &meter_properties(target),
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
    pub(super) captured_at: std::time::Instant,
    pub(super) rms_db: f32,
    pub(super) peak_db: f32,
}

impl Capture {
    /// Spawn `pw-cat` and return a handle that yields `hop_size`-sized
    /// sample slices sliding over a `fft_size` buffer.
    pub(super) fn spawn(
        sample_rate: u32,
        fft_size: usize,
        hop_size: usize,
        target: &str,
        latency_ms: u32,
    ) -> io::Result<Self> {
        if fft_size < 4
            || hop_size == 0
            || hop_size > sample_rate as usize
            || !(1..=1000).contains(&latency_ms)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid capture dimensions",
            ));
        }

        let mut child = spec(sample_rate, target, latency_ms)
            .spawn()
            .map_err(io::Error::other)?;

        let Some(stdout) = child.take_stdout() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("pw-cat: no stdout handle"));
        };

        debug!("audio monitor: spawned pw-cat target={target} rate={sample_rate} fft={fft_size}");

        Ok(Self {
            child: std::sync::Arc::new(std::sync::Mutex::new(child)),
            stdout: BufReader::with_capacity(fft_size * 4, stdout),
            ring: vec![0.0; fft_size],
            read_buf: vec![0; hop_size * 4],
            write_pos: 0,
            fft_size,
            hop_size,
            samples_read: 0,
            captured_at: std::time::Instant::now(),
            rms_db: -120.0,
            peak_db: -120.0,
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

        self.captured_at = std::time::Instant::now();
        let (rms_db, peak_db) = decode_hop(&self.read_buf, &mut self.ring, &mut self.write_pos)?;
        self.rms_db = rms_db;
        self.peak_db = peak_db;
        self.samples_read = self.samples_read.saturating_add(self.hop_size);
        Ok(self.hop_size)
    }

    /// Copy into reusable caller-owned storage in chronological order.
    pub(super) fn copy_window_into(&self, out: &mut Vec<f32>) {
        out.clear();
        out.extend_from_slice(&self.ring[self.write_pos..]);
        out.extend_from_slice(&self.ring[..self.write_pos]);
    }
}

/// Decode without per-hop allocation. Levels cover the entire hop, while the
/// ring retains only its latest FFT window. Reject invalid PCM rather than
/// turning NaN into silence that might authorize an upward gain correction.
fn decode_hop(bytes: &[u8], ring: &mut [f32], write_pos: &mut usize) -> io::Result<(f32, f32)> {
    let mut square = 0.0_f64;
    let mut peak = 0.0_f32;
    for chunk in bytes.as_chunks::<4>().0 {
        let sample = f32::from_ne_bytes(*chunk);
        if !sample.is_finite() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "non-finite microphone PCM",
            ));
        }
        square += f64::from(sample).powi(2);
        peak = peak.max(sample.abs());
        ring[*write_pos] = sample;
        *write_pos = (*write_pos + 1) % ring.len();
    }
    let count = bytes.len() / 4;
    let rms = if count == 0 {
        0.0
    } else {
        (square / count as f64).sqrt() as f32
    };
    Ok((super::amplitude_to_db(rms), super::amplitude_to_db(peak)))
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

    #[test]
    fn explicit_meter_cannot_fall_back_or_move_to_a_different_microphone() {
        let argv = spec(48_000, "alsa_input.test", 50).resolved().argv;
        let at = argv.iter().position(|arg| arg == "-P").unwrap();
        for property in [
            "node.dont-fallback=true",
            "node.dont-reconnect=true",
            "node.dont-move=true",
        ] {
            assert!(argv[at + 1].contains(property));
        }
        assert!(argv.contains(&String::from("alsa_input.test")));
    }

    /// The mark travels with the command, and it has to be exactly the one the
    /// shell's classifier looks for. Checked here because the alternative is
    /// finding out from a desktop that says somebody is recording you whenever
    /// this window is open — which is what happened before the mark existed.
    #[test]
    fn the_spectrum_stream_says_it_is_only_a_meter() {
        let argv = spec(48_000, "@DEFAULT_SOURCE@", 100).resolved().argv;
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
        assert!(argv.contains(&String::from("auto")));
        assert!(argv.contains(&String::from("f32")));
    }
    #[test]
    fn an_agc_hop_longer_than_the_fft_keeps_its_early_peak() {
        let mut samples = vec![0.0_f32; 2400];
        samples[0] = 1.0; // This impulse is outside the final 2048-sample FFT.
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_ne_bytes()).collect();
        let mut ring = vec![0.0; 2048];
        let (rms, peak) = super::decode_hop(&bytes, &mut ring, &mut 0).unwrap();
        assert_eq!(peak, 0.0);
        assert!((rms - 10.0 * (1.0_f32 / 2400.0).log10()).abs() < 1e-4);
        assert!(ring.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn nonfinite_pcm_is_not_interpreted_as_silence() {
        for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(super::decode_hop(&sample.to_ne_bytes(), &mut [0.0; 4], &mut 0).is_err());
        }
    }
}
