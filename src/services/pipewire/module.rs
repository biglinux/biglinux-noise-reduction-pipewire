//! systemd unit helpers for the per-loader mic / AEC / output chains.
//!
//! The rewrite deliberately avoids restarting the system audio stack
//! for routine setting changes — that would interrupt every live
//! stream every time the user drags a slider. The layout is:
//!
//! - **Control values** (sliders, strength, model) update the running
//!   graph via [`crate::services::pipewire::apply_live`]. No service
//!   restart, no audio glitch.
//! - **Per-application output routing** is pushed straight into the
//!   PipeWire graph through metadata
//!   ([`crate::services::pipewire::route_stream`]). WirePlumber reads
//!   the metadata change in place — no `wireplumber.service` restart.
//! - **Mic chain topology changes** (a filter added or removed, the
//!   user enabling processing for the first time) reach the running
//!   loader only if we restart `biglinux-microphone-mic.service`. The
//!   restart affects exclusively our `mic-biglinux` virtual source.
//! - **AEC lifecycle** toggles `biglinux-microphone-aec.service`. The
//!   mic unit `Wants/After`s it, so starting AEC after the mic chain
//!   is up triggers a brief mic restart so the EC source is wired
//!   into the chain.
//! - **Output chain lifecycle** toggles
//!   `biglinux-microphone-output.service`. Only streams routed through
//!   the output filter see a brief interruption; the rest of the audio
//!   graph is untouched.
//!
//! Every unit runs the same `biglinux-microphone-pwloader` binary,
//! which connects as a regular client of the main PipeWire daemon.
//! That gives all three loaders one shared clock — no cross-process
//! drift, no `spa.alsa: front:1p ... resync` events.
//!
//! We never restart `wireplumber.service` any more. A stale drop-in in
//! `~/.config/wireplumber/wireplumber.conf.d/` is fine: WirePlumber
//! reads it on its next natural start-up, and live metadata covers the
//! interactive case.

use std::io;
use std::process::{Command, Stdio};

use log::debug;

const MIC_UNIT: &str = "biglinux-microphone-mic.service";
const AEC_UNIT: &str = "biglinux-microphone-aec.service";
const OUTPUT_UNIT: &str = "biglinux-microphone-output.service";

/// Restart the AEC loader. Only the `echo-cancel-source` virtual
/// source briefly disappears from the graph; the mic loader picks it
/// up again as soon as it's back. Clears `failed` state first to ride
/// out crash-loops left behind by previous packaged versions.
pub fn restart_aec_service() -> io::Result<()> {
    let _ = run_systemctl(["--user", "reset-failed", AEC_UNIT]);
    run_systemctl(["--user", "restart", AEC_UNIT])?;
    debug!("pipewire: {AEC_UNIT} restarted");
    Ok(())
}

/// Start the AEC loader if it isn't already running.
pub fn start_aec_service() -> io::Result<()> {
    let _ = run_systemctl(["--user", "reset-failed", AEC_UNIT]);
    run_systemctl(["--user", "start", AEC_UNIT])?;
    debug!("pipewire: {AEC_UNIT} started");
    Ok(())
}

/// Stop the AEC loader. The `echo-cancel-source` virtual source
/// disappears from the graph; the mic loader gracefully falls back to
/// linking against the hardware mic via WirePlumber's
/// follow-default policy.
pub fn stop_aec_service() -> io::Result<()> {
    run_systemctl(["--user", "stop", AEC_UNIT])?;
    debug!("pipewire: {AEC_UNIT} stopped");
    Ok(())
}

/// Restart the mic loader so it picks up topology changes in
/// `~/.config/biglinux-microphone/mic.args`. Recorders reconnect to
/// the hardware default automatically thanks to WirePlumber's
/// follow-default policy while the loader is briefly absent.
pub fn restart_mic_service() -> io::Result<()> {
    let _ = run_systemctl(["--user", "reset-failed", MIC_UNIT]);
    run_systemctl(["--user", "restart", MIC_UNIT])?;
    debug!("pipewire: {MIC_UNIT} restarted");
    Ok(())
}

/// Start the mic loader. Idempotent — systemd treats a `start` on a
/// running unit as a no-op.
pub fn start_mic_service() -> io::Result<()> {
    let _ = run_systemctl(["--user", "reset-failed", MIC_UNIT]);
    run_systemctl(["--user", "start", MIC_UNIT])?;
    debug!("pipewire: {MIC_UNIT} started");
    Ok(())
}

/// Stop the mic loader. Used when the user disables every mic filter:
/// without it the loader keeps the dangling module loaded even after
/// we delete the args file on disk.
pub fn stop_mic_service() -> io::Result<()> {
    run_systemctl(["--user", "stop", MIC_UNIT])?;
    debug!("pipewire: {MIC_UNIT} stopped");
    Ok(())
}

/// Restart just the mic chain — alias kept so legacy call sites do not
/// have to know about the underlying unit name.
pub fn reload_mic_chain() -> io::Result<()> {
    restart_mic_service()
}

/// Start the output loader. Idempotent. Clears `failed` state first.
pub fn start_output_service() -> io::Result<()> {
    let _ = run_systemctl(["--user", "reset-failed", OUTPUT_UNIT]);
    run_systemctl(["--user", "start", OUTPUT_UNIT])?;
    debug!("pipewire: {OUTPUT_UNIT} started");
    Ok(())
}

/// Restart the output loader — used whenever the on-disk
/// `output.args` has changed and the running instance must pick it up.
pub fn restart_output_service() -> io::Result<()> {
    let _ = run_systemctl(["--user", "reset-failed", OUTPUT_UNIT]);
    run_systemctl(["--user", "restart", OUTPUT_UNIT])?;
    debug!("pipewire: {OUTPUT_UNIT} restarted");
    Ok(())
}

/// Stop the output loader so its virtual sink disappears from the
/// graph. Apps previously routed through it fall back to the default
/// sink via WirePlumber's automatic follow-default.
pub fn stop_output_service() -> io::Result<()> {
    run_systemctl(["--user", "stop", OUTPUT_UNIT])?;
    debug!("pipewire: {OUTPUT_UNIT} stopped");
    Ok(())
}

fn run_systemctl<const N: usize>(args: [&str; N]) -> io::Result<()> {
    let status = Command::new("systemctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "systemctl {args:?} exited with {status}"
        )))
    }
}

#[cfg(test)]
mod tests {
    // Interaction with systemd requires a live user manager. Covered by
    // integration runs under a real session.
    #[test]
    fn noop() {}
}
