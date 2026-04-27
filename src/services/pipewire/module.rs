//! systemd unit helpers for the mic + output filter-chains.
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
//!   filter-chain module only if we reload
//!   `filter-chain.service`. That's the one case this module handles
//!   and it affects exclusively our `mic-biglinux` virtual source —
//!   the rest of the system keeps playing.
//! - **Output chain lifecycle** toggles our dedicated `pipewire -c`
//!   instance (`biglinux-microphone-output.service`). Only streams
//!   routed through the output filter see a brief interruption; the
//!   rest of the audio graph is untouched.
//!
//! We never restart `wireplumber.service` any more. A stale drop-in in
//! `~/.config/wireplumber/wireplumber.conf.d/` is fine: WirePlumber
//! reads it on its next natural start-up, and live metadata covers the
//! interactive case.

use std::io;
use std::process::{Command, Stdio};

use log::debug;

/// Restart the user-level `filter-chain.service` so it picks up changes
/// in `filter-chain.conf.d/`. Only the **mic** virtual source briefly
/// disappears from the graph; recorders reconnect to the hardware
/// default automatically thanks to WirePlumber's follow-default policy.
///
/// `reset-failed` is issued first so a unit that was crash-looping on a
/// previous version's stale config (e.g. the `zeroramp` builtin missing
/// on older PipeWire) can come back without a manual `systemctl
/// reset-failed`. systemd otherwise rejects the restart with "Start
/// request repeated too quickly".
pub fn restart_filter_chain_service() -> io::Result<()> {
    let _ = run_systemctl(["--user", "reset-failed", "filter-chain.service"]);
    run_systemctl(["--user", "restart", "filter-chain.service"])?;
    debug!("pipewire: filter-chain.service restarted");
    Ok(())
}

/// Stop `filter-chain.service`. Used when the user disables every mic
/// filter: without it, the daemon keeps the now-dangling module loaded
/// even after we delete the drop-in on disk.
pub fn stop_filter_chain_service() -> io::Result<()> {
    run_systemctl(["--user", "stop", "filter-chain.service"])?;
    debug!("pipewire: filter-chain.service stopped");
    Ok(())
}

/// Restart just the **mic** filter chain. Higher-level helper used by
/// the UI whenever the mic topology changes (filter added/removed,
/// master enable toggle). Wraps [`restart_filter_chain_service`] so
/// call sites don't have to know about the underlying unit.
pub fn reload_mic_chain() -> io::Result<()> {
    restart_filter_chain_service()
}

/// Start the standalone `biglinux-microphone-output.service` that
/// hosts the dedicated `pipewire -c` instance for the output chain.
/// Idempotent. Clears `failed` state first — see
/// [`restart_filter_chain_service`] for the rationale.
pub fn start_output_service() -> io::Result<()> {
    let _ = run_systemctl([
        "--user",
        "reset-failed",
        "biglinux-microphone-output.service",
    ]);
    run_systemctl(["--user", "start", "biglinux-microphone-output.service"])?;
    debug!("pipewire: biglinux-microphone-output.service started");
    Ok(())
}

/// Restart the output service — used whenever the on-disk
/// `biglinux-microphone-output.conf` has changed and the running
/// instance must pick it up.
pub fn restart_output_service() -> io::Result<()> {
    let _ = run_systemctl([
        "--user",
        "reset-failed",
        "biglinux-microphone-output.service",
    ]);
    run_systemctl(["--user", "restart", "biglinux-microphone-output.service"])?;
    debug!("pipewire: biglinux-microphone-output.service restarted");
    Ok(())
}

/// Stop the output service so its virtual sink disappears from the
/// graph. Apps previously routed through it fall back to the default
/// sink via WirePlumber's automatic follow-default.
pub fn stop_output_service() -> io::Result<()> {
    run_systemctl(["--user", "stop", "biglinux-microphone-output.service"])?;
    debug!("pipewire: biglinux-microphone-output.service stopped");
    Ok(())
}

/// Start the standalone echo-cancel service. The unit has a
/// `ConditionPathExists=` on the conf file — `start` is a no-op when
/// the configurator hasn't written it (i.e. AEC is off).
pub fn start_echo_cancel_service() -> io::Result<()> {
    let _ = run_systemctl([
        "--user",
        "reset-failed",
        "biglinux-microphone-echocancel.service",
    ]);
    run_systemctl(["--user", "start", "biglinux-microphone-echocancel.service"])?;
    debug!("pipewire: biglinux-microphone-echocancel.service started");
    Ok(())
}

/// Restart the echo-cancel service so a freshly-written conf takes
/// effect. Only used when AEC is on.
pub fn restart_echo_cancel_service() -> io::Result<()> {
    let _ = run_systemctl([
        "--user",
        "reset-failed",
        "biglinux-microphone-echocancel.service",
    ]);
    run_systemctl([
        "--user",
        "restart",
        "biglinux-microphone-echocancel.service",
    ])?;
    debug!("pipewire: biglinux-microphone-echocancel.service restarted");
    Ok(())
}

/// Stop the echo-cancel service when the user toggles AEC off. The
/// virtual `echo-cancel-source` disappears; the mic chain is rewritten
/// without `filter.smart.target` so it falls back to the default
/// hardware mic.
pub fn stop_echo_cancel_service() -> io::Result<()> {
    run_systemctl(["--user", "stop", "biglinux-microphone-echocancel.service"])?;
    debug!("pipewire: biglinux-microphone-echocancel.service stopped");
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
