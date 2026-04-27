//! Self-listen loopback (pw-loopback subprocess).
//!
//! When the user wants to hear their own microphone — e.g. to calibrate
//! the filter intensity or to verify a noisy environment — we spawn
//! `pw-loopback` to bridge the default source to the default sink.
//! Because the mic filter chain is exposed as a WirePlumber smart
//! filter, the loopback transparently picks up the **filtered** signal
//! while the chain is loaded, and the raw hardware signal otherwise.
//!
//! The handle owns the child process. Drop kills it; the user can
//! also call [`Loopback::stop`] explicitly. If `pw-loopback` exits on
//! its own (e.g. the daemon went away) [`Loopback::is_alive`] starts
//! returning `false` so the UI can refresh its toggle.
//!
//! Feedback warning: loopback to a sink connected to speakers can
//! create acoustic feedback. The UI surfaces this as a tooltip; the
//! service intentionally stays unaware — feedback policy is a UX
//! concern, not a transport concern.

use std::io;
use std::process::{Child, Command, Stdio};

use log::{debug, warn};

/// Default `pw-loopback` capture target. `@DEFAULT_SOURCE@` follows
/// whatever WirePlumber currently picks as the default source — i.e.
/// the smart-filtered mic when our chain is up.
pub const DEFAULT_CAPTURE_TARGET: &str = "@DEFAULT_SOURCE@";

/// `node.name` advertised by the loopback playback side. Stable so the
/// user can identify it in `pavucontrol` / `wpctl status`.
pub const LOOPBACK_NODE_NAME: &str = "biglinux-mic-monitor";

/// `media.name` advertised by the loopback so it shows up nicely in
/// per-stream mixer UIs.
pub const LOOPBACK_MEDIA_NAME: &str = "BigLinux Microphone Monitor";

/// Running loopback handle. Drop kills the child and reaps it.
pub struct Loopback {
    child: Child,
}

/// Tunables used when spawning the loopback. Defaults match what the
/// Python implementation shipped: low latency, no extra delay, audio
/// auto-channels.
#[derive(Debug, Clone)]
pub struct LoopbackOptions {
    /// Capture-side `node.target`. See [`DEFAULT_CAPTURE_TARGET`].
    pub capture_target: String,
    /// Extra delay applied by `pw-loopback`, in milliseconds. 0 = none.
    pub delay_ms: u32,
}

impl Default for LoopbackOptions {
    fn default() -> Self {
        Self {
            capture_target: DEFAULT_CAPTURE_TARGET.to_owned(),
            delay_ms: 0,
        }
    }
}

impl Loopback {
    /// Spawn `pw-loopback` and return a handle on success.
    ///
    /// Errors when the binary is not on `$PATH` or the OS cannot fork
    /// the process. The PipeWire daemon does not need to already host
    /// the source — `pw-loopback` waits for it.
    pub fn start(opts: &LoopbackOptions) -> io::Result<Self> {
        let delay_seconds = f64::from(opts.delay_ms) / 1000.0;
        let mut cmd = Command::new("pw-loopback");
        cmd.args([
            "--capture-props=media.class=Stream/Input/Audio",
            "--playback-props=media.class=Stream/Output/Audio",
            "--latency=100ms",
        ])
        .arg(format!("--delay={delay_seconds}"))
        .arg(format!(
            "--capture-props=node.target={}",
            opts.capture_target
        ))
        .arg(format!("--playback-props=media.name={LOOPBACK_MEDIA_NAME}",))
        .arg(format!("--playback-props=node.name={LOOPBACK_NODE_NAME}",))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

        let child = cmd.spawn()?;
        debug!(
            "loopback: spawned pw-loopback pid={} target={}",
            child.id(),
            opts.capture_target,
        );
        Ok(Self { child })
    }

    /// Whether the loopback subprocess is still running.
    #[must_use]
    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) | Err(_) => false,
        }
    }

    /// Terminate the loopback. Errors from the syscall path are
    /// swallowed (already-dead child, etc.) — the goal is "child gone".
    pub fn stop(mut self) {
        if let Err(e) = self.child.kill() {
            warn!("loopback: kill failed: {e}");
        }
        let _ = self.child.wait();
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_default_uses_default_source() {
        let o = LoopbackOptions::default();
        assert_eq!(o.capture_target, DEFAULT_CAPTURE_TARGET);
        assert_eq!(o.delay_ms, 0);
    }

    #[test]
    fn node_name_is_stable() {
        assert_eq!(LOOPBACK_NODE_NAME, "biglinux-mic-monitor");
    }
}
