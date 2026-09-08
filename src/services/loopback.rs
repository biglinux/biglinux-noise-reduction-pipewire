//! Self-listen loopback (pw-loopback subprocess).
//!
//! When the user wants to hear their own microphone — e.g. to calibrate
//! the filter intensity or to verify a noisy environment — we spawn
//! `pw-loopback` to bridge the default source to the default sink.
//! Because the mic filter chain is exposed as a WirePlumber smart
//! filter, the loopback transparently picks up the **filtered** signal
//! while the chain is loaded, and the raw hardware signal otherwise.
//!
//! The handle owns the child process. Drop kills it. If `pw-loopback`
//! exits on its own (e.g. the daemon went away) [`Loopback::is_alive`] starts
//! returning `false` so the UI can refresh its toggle.
//!
//! Feedback warning: loopback to a sink connected to speakers can
//! create acoustic feedback. The UI surfaces this as a tooltip; the
//! service intentionally stays unaware — feedback policy is a UX
//! concern, not a transport concern.

use std::io;

use big_os_kit::subprocess::{BigSubprocessChild, BigSubprocessOutputMode, BigSubprocessSpec};
use log::debug;

/// `node.name` advertised by the loopback playback side. Stable so the
/// user can identify it in `pavucontrol` / `wpctl status`.
const LOOPBACK_NODE_NAME: &str = "biglinux-mic-monitor";

/// `media.name` advertised by the loopback so it shows up nicely in
/// per-stream mixer UIs.
const LOOPBACK_MEDIA_NAME: &str = "Filter noise Monitor";

/// Running loopback handle. Drop kills the child and reaps it.
pub struct Loopback {
    child: BigSubprocessChild,
}

impl Loopback {
    /// Spawn `pw-loopback` and return a handle on success.
    ///
    /// Errors when the binary is not on `$PATH` or the OS cannot fork
    /// the process. The PipeWire daemon does not need to already host
    /// the source — `pw-loopback` waits for it.
    pub fn start(delay_ms: u32) -> io::Result<Self> {
        let delay_seconds = f64::from(delay_ms) / 1000.0;
        let child = BigSubprocessSpec::builder()
            .program("/usr/bin/pw-loopback")
            .args([
                "--capture-props=media.class=Stream/Input/Audio",
                "--playback-props=media.class=Stream/Output/Audio",
                "--latency=100",
            ])
            .arg(format!("--delay={delay_seconds}"))
            .arg(format!("--playback-props=media.name={LOOPBACK_MEDIA_NAME}"))
            .arg(format!("--playback-props=node.name={LOOPBACK_NODE_NAME}"))
            .stdout(BigSubprocessOutputMode::Null)
            .stderr(BigSubprocessOutputMode::Null)
            .allow_list(["/usr/bin/pw-loopback"])
            .build()
            .spawn()
            .map_err(io::Error::other)?;
        debug!("loopback: spawned pw-loopback pid={}", child.id());
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
}

impl Drop for Loopback {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
