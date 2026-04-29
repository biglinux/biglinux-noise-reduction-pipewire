//! Restart the user-scoped PipeWire audio stack.
//!
//! Drop-in conf changes under `~/.config/pipewire` and
//! `~/.config/wireplumber` are read once at process start, so applying
//! tunables from the GUI requires bouncing the user units. We always
//! restart the trio together — `pipewire`, `wireplumber`,
//! `pipewire-pulse` — because they sit on the same socket and depending
//! on which knob the user touched, leaving any of them stale produces
//! mismatched parameters.
//!
//! We invoke `systemctl --user` directly rather than going through D-Bus
//! to avoid pulling in `zbus` for a single command. The call returns
//! quickly (units restart asynchronously), and any failure bubbles up
//! as the exit-status text from `systemctl` so the UI can surface it.

use std::io;
use std::process::{Command, Stdio};

/// User units we restart on every Apply. Order is irrelevant —
/// `systemctl --user` resolves dependencies internally.
const UNITS: &[&str] = &["pipewire", "wireplumber", "pipewire-pulse"];

/// Restart the user-scoped PipeWire stack so freshly written drop-in
/// configuration files are picked up. Blocking; safe to dispatch onto
/// `gio::spawn_blocking` from the UI thread.
pub fn restart_pipewire_user_stack() -> io::Result<()> {
    let mut cmd = Command::new("systemctl");
    cmd.arg("--user")
        .arg("restart")
        .args(UNITS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let output = cmd.output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(io::Error::other(format!(
        "systemctl --user restart {} exited with {}: {stderr}",
        UNITS.join(" "),
        output.status,
    )))
}
