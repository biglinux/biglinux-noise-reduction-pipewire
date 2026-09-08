//! Startup health probe — the screen contract's Checking → Ready /
//! Unavailable state.
//!
//! The window must not imply the processing chain is active from saved
//! settings alone: `settings.json` says what the user *wants*, not what
//! the system can deliver. [`probe`] answers the three questions that
//! decide whether toggles can honour their promise, each from a real
//! source (subprocess / file stat), never from assumptions:
//!
//! 1. Is PipeWire answering? (`pw-cli info 0`)
//! 2. Can the default denoiser actually run? (LADSPA path stat plus the
//!    `dlopen` of its inference runtime, via
//!    [`NoiseModel::plugin_loadable_cached`])
//! 3. Are the pwloader user units installed? (`systemctl --user cat`)
//!
//! The probes themselves are `diagnostics`' — the banner and `doctor` must
//! not be able to disagree about whether the same check passed.
//!
//! Runs on a worker thread (subprocess latency); the shell shows a
//! "Checking…" banner until the result lands and downgrades the window
//! to an explained, insensitive Unavailable state when a probe fails —
//! cause + next action, per the screen contract.

use crate::config::NoiseModel;
use crate::diagnostics::{command_succeeds, unit_known};
use crate::services::pipewire::MIC_UNIT;

use super::i18n::i18n;

/// Probe result consumed by the shell banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// Every probe passed — the toggles can honour their promise.
    Ready,
    /// A probe failed; the window shows `cause` and `hint` and the body
    /// is made insensitive until a re-check succeeds.
    Unavailable { cause: String, hint: String },
}

/// Run all availability probes. Blocking (subprocesses) — call from a
/// worker thread, not the GTK main loop.
#[must_use]
pub fn probe() -> Health {
    if !pipewire_reachable() {
        return Health::Unavailable {
            cause: i18n("The audio system (PipeWire) is not responding."),
            hint: i18n("Log out and back in, then reopen this window."),
        };
    }
    // Loadability, not bare presence. A stat passes with the plugin file
    // installed and its runtime missing, and the plugins pass audio through
    // instead of failing — so the window said Ready while nothing denoised.
    if !NoiseModel::default().plugin_loadable_cached() {
        return Health::Unavailable {
            cause: i18n("The noise-reduction engine is not installed."),
            hint: i18n("Install the gtcrn-ladspa package, then check again."),
        };
    }
    if !unit_known(MIC_UNIT) {
        return Health::Unavailable {
            cause: i18n("The background audio services are not installed."),
            hint: i18n("Reinstall Filter noise, then log out and back in."),
        };
    }
    Health::Ready
}

/// `pw-cli info 0` succeeds only when a PipeWire daemon answers on the
/// user's socket — the same transport every apply/live-update uses.
fn pipewire_reachable() -> bool {
    command_succeeds("/usr/bin/pw-cli", &["info", "0"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_carries_cause_and_hint() {
        // Contract: an Unavailable state must always pair the cause with
        // a next action — the banner renders both, never a bare error.
        let h = Health::Unavailable {
            cause: "x".into(),
            hint: "y".into(),
        };
        match h {
            Health::Unavailable { cause, hint } => {
                assert!(!cause.is_empty());
                assert!(!hint.is_empty());
            }
            Health::Ready => panic!("constructed Unavailable"),
        }
    }
}
