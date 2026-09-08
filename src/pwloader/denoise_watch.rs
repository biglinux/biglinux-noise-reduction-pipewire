//! Watches whether the denoiser is keeping up, from outside the thread that
//! could stop keeping up.
//!
//! Inference runs on a worker thread inside this process, one hop ahead of the
//! audio. When that worker falls behind — a loaded machine, a model too heavy
//! for the CPU — the audio callback still finishes on time and PipeWire
//! reports no xrun. The user hears unprocessed microphone and nothing counts
//! it. The plugin counts it, in two process-global atomics.
//!
//! This reads them from the loader's main loop rather than from the worker,
//! deliberately: a starved thread cannot report its own starvation, and the
//! main loop carries no inference load.
//!
//! What comes out is two journal entries — entered degraded, recovered — not a
//! periodic metric. A log is the right place for a service saying it changed
//! state and the wrong place to keep a time series. `MESSAGE_ID` makes them
//! queryable:
//!
//! ```text
//! journalctl --user -u biglinux-microphone-mic \
//!     MESSAGE_ID=8f3c1d2e4b5a6c7d8e9f0a1b2c3d4e5f -o json
//! ```

use std::ffi::CString;
use std::path::Path;
use std::time::{Duration, Instant};

/// Stable identifier for both entries, so a reader can find them without
/// matching on wording. Generated once; never change it.
const MESSAGE_ID: &str = "8f3c1d2e4b5a6c7d8e9f0a1b2c3d4e5f";

/// Fraction of hops that must come back enhanced for the chain to be healthy.
const HEALTHY: f64 = 0.98;
/// Below this, the user is hearing the raw microphone often enough to notice.
const DEGRADED: f64 = 0.90;
/// How long a verdict must hold before it is announced, so a moment of load
/// does not flap the state.
const SETTLE: Duration = Duration::from_secs(30);

/// Reads the counters a denoiser plugin exports.
///
/// Found by `dlopen`ing the plugin this process already loaded: the second
/// open of a path returns the same link map, so the accessor reads the very
/// statics the audio thread writes. No IPC, because there is no boundary —
/// the plugin runs inside this process.
pub struct DenoiseWatch {
    // SAFETY INVARIANT: `hops` is a symbol from `handle`, which is never
    // closed while this struct lives.
    handle: *mut libc::c_void,
    hops: unsafe extern "C" fn(*mut u64, *mut u64),
    last: Option<(u64, u64)>,
    /// Whether the last announced state was "degraded".
    announced_degraded: bool,
    /// When the current verdict first held.
    verdict_since: Option<(bool, Instant)>,
}

impl DenoiseWatch {
    /// Open the plugin at `path` and resolve its counter accessor.
    ///
    /// `None` when the file is not a denoiser that exports one — every other
    /// plugin in a chain, and any build older than this contract.
    #[must_use]
    pub fn open(path: &Path) -> Option<Self> {
        let cpath = CString::new(path.as_os_str().to_str()?).ok()?;
        // SAFETY: `cpath` is a valid NUL-terminated string. The handle is
        // stored and never closed, so the symbol below stays valid.
        let handle = unsafe { libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return None;
        }
        let name = CString::new("dpdfnet_hops").ok()?;
        // SAFETY: `handle` is a live library handle from the call above.
        let symbol = unsafe { libc::dlsym(handle, name.as_ptr()) };
        if symbol.is_null() {
            // SAFETY: closing a handle we just opened and are about to drop.
            unsafe { libc::dlclose(handle) };
            return None;
        }
        // SAFETY: the plugin declares this symbol with exactly this signature.
        let hops = unsafe {
            std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn(*mut u64, *mut u64)>(
                symbol,
            )
        };
        Some(Self {
            handle,
            hops,
            last: None,
            announced_degraded: false,
            verdict_since: None,
        })
    }

    /// Sample the counters and announce a state change if one has settled.
    ///
    /// Call from a timer. Windows are the difference between samples, so the
    /// verdict is about the interval that just passed rather than the whole
    /// session.
    pub fn tick(&mut self) {
        let (mut total, mut enhanced) = (0_u64, 0_u64);
        // SAFETY: both pointers are valid for one write each.
        unsafe { (self.hops)(&raw mut total, &raw mut enhanced) };

        let Some((prev_total, prev_enhanced)) = self.last.replace((total, enhanced)) else {
            return;
        };
        // A reset zeroes the counters; skip the window that straddles it.
        if total < prev_total || enhanced < prev_enhanced {
            self.verdict_since = None;
            return;
        }
        let asked = total - prev_total;
        // Nothing captured in this window: no evidence either way.
        if asked == 0 {
            self.verdict_since = None;
            return;
        }

        let ratio = (enhanced - prev_enhanced) as f64 / asked as f64;
        let degraded = is_degraded(ratio, self.announced_degraded);

        match self.verdict_since {
            Some((held, since)) if held == degraded => {
                if since.elapsed() >= SETTLE && degraded != self.announced_degraded {
                    Self::announce(degraded, ratio);
                    self.announced_degraded = degraded;
                    self.verdict_since = Some((degraded, Instant::now()));
                }
            }
            _ => self.verdict_since = Some((degraded, Instant::now())),
        }
    }

    fn announce(degraded: bool, ratio: f64) {
        let message = journal_message(degraded, ratio);
        let sent = std::os::unix::net::UnixDatagram::unbound().and_then(|socket| {
            socket.set_nonblocking(true)?;
            socket.send_to(message.as_bytes(), "/run/systemd/journal/socket")
        });
        if sent.is_err() {
            eprintln!(
                "noise reduction: {} ({:.1}% processed)",
                if degraded {
                    "not keeping up"
                } else {
                    "recovered"
                },
                ratio * 100.0
            );
        }
    }
}

fn journal_message(degraded: bool, ratio: f64) -> String {
    format!(
        "MESSAGE_ID={MESSAGE_ID}\nPRIORITY={}\nDENOISE_RATIO={:.1}\nMESSAGE=Noise reduction {}\n",
        if degraded { 4 } else { 6 },
        ratio * 100.0,
        if degraded {
            "is not keeping up"
        } else {
            "has recovered"
        }
    )
}

impl Drop for DenoiseWatch {
    fn drop(&mut self) {
        // SAFETY: this is our owned dlopen reference. No callback outlives
        // the watcher, and PipeWire owns its separate plugin reference.
        unsafe { libc::dlclose(self.handle) };
    }
}

/// Every `.so` a filter-chain configuration names.
///
/// The loader is handed the chain's arguments as text, so the plugins it is
/// about to load are right there. Reading them back is cheaper and more
/// honest than a second list that could disagree with the one in force.
#[must_use]
pub fn plugin_paths(chain_args: &str) -> Vec<String> {
    chain_args
        .lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once("plugin")?;
            let rest = rest.trim_start().strip_prefix('=')?.trim();
            let path = rest.trim_matches('"');
            // A plugin path is written by us and is always lowercase, so the
            // exact comparison is the right one: this skips the chain's
            // builtin entries, it is not guessing at spelling.
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            path.ends_with(".so").then(|| path.to_owned())
        })
        .collect()
}

/// Whether this window counts as degraded, given what was last announced.
///
/// The two thresholds are the hysteresis, and which one applies depends on the
/// state already announced: a chain that has been called degraded has to climb
/// back over `HEALTHY` to be called healthy again, not merely back over
/// `DEGRADED`. One band would announce twice a minute on a chain sitting on the
/// line, and a warning that repeats is a warning nobody reads.
fn is_degraded(ratio: f64, announced_degraded: bool) -> bool {
    if announced_degraded {
        ratio < HEALTHY
    } else {
        ratio < DEGRADED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_paths_reads_the_chain_it_was_given() {
        let args = r#"
        filter.graph = {
            nodes = [
                { type = builtin name = "hpf" plugin = "builtin" label = "bq_highpass" }
                { type = ladspa name = "ai"
                  plugin = "/usr/lib/ladspa/libdpdfnet_dpdfnet2_48khz_hr_ladspa.so"
                  label = "dpdfnet2_48khz_hr_mono" }
            ]
        }
        "#;
        assert_eq!(
            plugin_paths(args),
            vec!["/usr/lib/ladspa/libdpdfnet_dpdfnet2_48khz_hr_ladspa.so"]
        );
    }

    #[test]
    fn a_chain_of_builtins_names_no_shared_object() {
        let args = r#"{ plugin = "builtin" label = "copy" }"#;
        assert!(plugin_paths(args).is_empty());
    }

    #[test]
    fn the_bands_do_not_let_a_chain_on_the_line_announce_twice() {
        // Freshly healthy: only a real drop below the lower band is degraded.
        assert!(!is_degraded(1.00, false));
        assert!(!is_degraded(0.95, false), "0.95 is between the bands");
        assert!(is_degraded(0.85, false));

        // Already announced: it takes a climb past the upper band to come back.
        assert!(is_degraded(0.95, true), "0.95 is not recovery yet");
        assert!(!is_degraded(0.99, true));
    }

    #[test]
    fn a_plugin_without_the_accessor_is_not_watched() {
        assert!(DenoiseWatch::open(Path::new("/usr/lib/ladspa/amp.so")).is_none());
        assert!(DenoiseWatch::open(Path::new("/does/not/exist.so")).is_none());
    }
}

#[cfg(test)]
mod journal_tests {
    #[test]
    fn journal_fields_belong_to_one_native_datagram() {
        let message = super::journal_message(true, 0.85);
        assert!(message.contains("\nPRIORITY=4\n"));
        assert!(message.contains("\nDENOISE_RATIO=85.0\n"));
        assert!(message.contains("\nMESSAGE=Noise reduction is not keeping up\n"));
    }
}
