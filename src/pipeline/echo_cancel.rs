//! AEC is upstream of microphone denoising, dynamics and gain.
//!
//! `monitor.mode=true` captures the chosen playback monitor; the packaged
//! WirePlumber policy selects the physical microphone and the actual render
//! route. Incorrect/missing reference is not repairable with a stronger gate.
//! The reference must include software playback effects, but a digital monitor
//! cannot include nonlinear distortion produced later by a physical speaker.
//!
//! The GTCRN SPA contract is strictly mono (mono capture/output, mono monitor
//! reference); there is no stereo render override.
//!
//! The echo-cancel module consumes and emits exactly one internal block (768
//! frames for GTCRN) per graph cycle and drops a block when the graph quantum
//! differs, so the AEC nodes pin the quantum to that block with
//! `node.force-quantum` (not `node.lock-quantum`, which only freezes an inherited
//! value). See PipeWire's `module-echo-cancel.c` `process`. The WirePlumber
//! demand gate keeps AEC links inactive without a real recorder.

/// `node.name` of the virtual source created by the EC module.
pub const EC_SOURCE_NAME: &str = "echo-cancel-source";
/// Capture stream owned by the EC module. A WirePlumber Lua hook
/// targets this stream to the selected physical source at runtime.
pub const EC_CAPTURE_NODE_NAME: &str = "echo-cancel-capture";
/// `node.latency` requested by the mic filter-chain nodes (block-agnostic LADSPA,
/// so any value is safe). The AEC's own quantum is pinned separately to its block.
pub(crate) const AEC_NODE_LATENCY: &str = "960/48000";
/// File name of the AEC args body, consumed by
/// `biglinux-microphone-pwloader` (started by
/// `biglinux-microphone-aec.service`). The unit is started before
/// `biglinux-microphone-mic.service` (`After=`) so `echo-cancel-source`
/// already exists when the mic filter chain resolves its
/// `target.object`.
pub const ECHO_CANCEL_CONF_FILE: &str = "aec.args";

/// Installed GTCRN-AEC weights, passed to `libspa-aec-gtcrn` via `gtcrn.model`.
pub const GTCRN_MODEL_PATH: &str = "/usr/share/gtcrn-aec-native/localvqe-pi-aec-v1-49k-f32.gguf";

/// PipeWire `library.name` for the only echo-cancel engine we ship.
pub(crate) const AEC_LIBRARY: &str = "aec/libspa-aec-gtcrn";

/// Graph quantum, in frames, the echo-cancel module must run at. The module
/// processes exactly one internal block per cycle and drops a block when the
/// quantum differs from that block (`module-echo-cancel.c` `process`), so the
/// quantum has to equal it. GTCRN's block is 768 (`libspa-aec-gtcrn` declares
/// `768/48000`).
const AEC_QUANTUM: u32 = 768;

/// Render the AEC `args` body for `libpipewire-module-echo-cancel`,
/// consumed verbatim by `biglinux-microphone-pwloader`.
///
/// The output is just the `{ … }` block — no `context.modules`
/// wrapper, no comment header. The pwloader passes it straight to
/// `pw_context_load_module(libpipewire-module-echo-cancel, …)`.
///
/// The capture stream intentionally has no static `target.object`: a
/// WirePlumber Lua policy hook chooses the selected physical source
/// live, so the config stays portable across machines with different
/// or hot-swapped microphones. The reference side runs in
/// `monitor.mode = true`, tapping the monitor of the default sink
/// without exposing an Audio/Sink to clients — apps continue to play
/// to the real speaker.
#[cfg(test)]
fn build_echo_cancel_conf() -> String {
    build_echo_cancel_conf_settings(&crate::config::EchoCancelConfig::default())
}

pub(super) fn build_echo_cancel_conf_settings(
    settings: &crate::config::EchoCancelConfig,
) -> String {
    let aec_args = format!(
        "        gtcrn.model = \"{GTCRN_MODEL_PATH}\"\n\
             \x20       gtcrn.async-delay = {}\n\
             \x20       gtcrn.track-delay = {}\n\
             \x20       gtcrn.hb-adaptation-guard = {}\n\
             \x20       gtcrn.reference-silence-bypass = {}\n",
        settings.gtcrn_async_delay || settings.gtcrn_track_delay,
        settings.gtcrn_track_delay,
        settings.gtcrn_hb_guard,
        settings.gtcrn_reference_bypass
    );
    // The echo-cancel module emits and consumes exactly one internal block per
    // graph cycle and, per `module-echo-cancel.c` `process`, drops a whole block
    // whenever the graph quantum is not equal to that block size — the periodic
    // ~20 ms silence users heard. So pin the graph quantum to the GTCRN block with
    // `node.force-quantum`: it sets the driver's def=min=max, overriding the
    // daemon's 2048 default while our capture/source are linked and restoring it
    // when they leave. `node.latency` is set to the same value so the module's own
    // block-from-latency math lands on that exact block. `node.lock-quantum` is
    // NOT used: it only freezes whatever quantum was inherited at link time, which
    // is what made the chopping intermittent after a restart.
    format!(
        "{{\n\
         \x20   library.name = {AEC_LIBRARY}\n\
         \x20   node.latency = {AEC_QUANTUM}/48000\n\
         \x20   monitor.mode = true\n\
         \x20   audio.rate = 48000\n\
         \x20   audio.channels = 1\n\
         \x20   audio.position = [ MONO ]\n\
         \x20   buffer.max_size = 250\n\
         \x20   capture.props = {{\n\
         \x20       node.name    = \"{EC_CAPTURE_NODE_NAME}\"\n\
         \x20       node.pause-on-idle = false\n\
         \x20       node.force-quantum = {AEC_QUANTUM}\n\
         \x20   }}\n\
         \x20   source.props = {{\n\
         \x20       node.name        = \"{EC_SOURCE_NAME}\"\n\
         \x20       node.description = \"BigLinux Echo-Cancelled Mic\"\n\
         \x20       media.class      = Audio/Source\n\
         \x20       volume           = 1.0\n\
         \x20       node.pause-on-idle = false\n\
         \x20       node.force-quantum = {AEC_QUANTUM}\n\
         \x20   }}\n\
         \x20   aec.args = {{\n\
         {aec_args}\
         \x20   }}\n\
         }}\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conf_is_a_bare_module_args_body() {
        // The pwloader hands the file contents straight to
        // `pw_context_load_module(libpipewire-module-echo-cancel, …)`
        // — no `context.modules` wrapper, no comment header. The module
        // name itself comes from the systemd unit, not from the args.
        let conf = build_echo_cancel_conf();
        assert!(conf.starts_with('{'));
        assert!(conf.trim_end().ends_with('}'));
        assert!(!conf.contains("context.modules"));
        assert!(!conf.contains("libpipewire-module-echo-cancel"));
        assert!(conf.contains("aec/libspa-aec-gtcrn"));
    }

    #[test]
    fn conf_carries_only_gtcrn_args() {
        // The single engine we ship is GTCRN: the model path is present and no
        // legacy webrtc.* option leaks in.
        let conf = build_echo_cancel_conf();
        assert!(conf.contains(&format!("gtcrn.model = \"{GTCRN_MODEL_PATH}\"")));
        assert!(!conf.contains("webrtc."));
        assert!(!conf.contains("libspa-aec-webrtc"));
    }

    #[test]
    fn conf_exposes_named_source_in_monitor_mode() {
        // monitor.mode = true means no Audio/Sink virtual device is
        // created; the sink-side stream taps the default sink monitor
        // directly. Only the cleaned source node is visible to the rest
        // of the graph as Audio/Source.
        let conf = build_echo_cancel_conf();
        assert!(conf.contains(&format!("\"{EC_SOURCE_NAME}\"")));
        assert!(conf.contains("Audio/Source"));
        assert!(conf.contains("monitor.mode = true"));
        // GTCRN is strictly mono: no stereo render sink.
        assert!(!conf.contains("sink.props"));
        assert!(!conf.contains("node.passive"));
        assert!(
            !conf.contains("media.class      = Audio/Sink"),
            "monitor.mode=true must not declare a virtual AEC sink",
        );
    }

    #[test]
    fn both_nodes_force_the_block_quantum() {
        // The echo-cancel module drops a block whenever the graph quantum differs
        // from its internal block, which is the periodic ~20 ms silence. Pin the
        // quantum to the GTCRN block (768) on capture and source, and never
        // lock-quantum (which only freezes an inherited value).
        let conf = build_echo_cancel_conf();
        assert!(!conf.contains("node.lock-quantum"));
        assert_eq!(conf.matches("node.force-quantum = 768").count(), 2);
        assert!(conf.contains("node.latency = 768/48000"));
        assert_eq!(conf.matches("node.pause-on-idle = false").count(), 2);
        assert!(conf.contains("audio.channels = 1"));
        assert!(conf.contains("audio.position = [ MONO ]"));
        assert!(conf.contains("buffer.max_size = 250"));
    }

    #[test]
    fn conf_leaves_capture_target_to_wireplumber_policy() {
        let conf = build_echo_cancel_conf();
        assert!(conf.contains(&format!("node.name    = \"{EC_CAPTURE_NODE_NAME}\"")));
        assert!(
            !conf.contains("target.object"),
            "AEC capture target is chosen live by the WirePlumber Lua hook",
        );
    }
}

#[cfg(test)]
mod experimental_config_tests {
    use super::*;
    #[test]
    fn tracking_implies_worker_and_reaches_gtcrn_args() {
        let settings = crate::config::EchoCancelConfig {
            gtcrn_track_delay: true,
            gtcrn_hb_guard: true,
            ..Default::default()
        };
        let conf = build_echo_cancel_conf_settings(&settings);
        assert!(conf.contains("gtcrn.async-delay = true"));
        assert!(conf.contains("gtcrn.track-delay = true"));
        assert!(conf.contains("gtcrn.hb-adaptation-guard = true"));
    }
}
