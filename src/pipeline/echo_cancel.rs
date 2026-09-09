//! Echo-cancellation filter generator.
//!
//! Use case: the user is on a meeting/call **without headphones**, so
//! speaker output bleeds back into the microphone. Acoustic echo
//! cancellation (AEC) subtracts the reference signal from the captured
//! mic before any other processing — including GTCRN — so the neural
//! denoiser doesn't waste capacity attenuating audio it later has to
//! restore.
//!
//! Hosted by `biglinux-microphone-pwloader` (started via
//! `biglinux-microphone-aec.service`), the AEC module loads inside its
//! own client process that connects to the main PipeWire daemon. The
//! daemon drives every node, so AEC, mic, and output filter graphs all
//! share one clock. Independent loader lifecycles let AEC toggle on and
//! off without reloading the mic chain.
//!
//! Topology:
//!
//! ```text
//! selected hw mic ─┐
//!                  ├─► libpipewire-module-echo-cancel ─► echo-cancel-source
//! default sink mon ┘                                     (Audio/Source)
//! ```
//!
//! `monitor.mode = true` is the load-bearing detail: instead of
//! creating an `Audio/Sink` virtual sink that apps would have to target
//! explicitly, the EC module's sink-side stream captures the monitor of
//! whatever the real default sink is. Apps continue to play to the
//! user's actual speaker; the AEC reference comes from that sink's
//! monitor port — exactly the audio that bleeds into the mic. We do not
//! override `sink.props`: the module's own defaults under monitor.mode
//! (passive, monitor-tap, no client-visible sink) match what we want,
//! and adding our own `node.passive`/`stream.capture.sink` lines
//! empirically broke convergence on PipeWire 1.6.x — the cleaned source
//! came out bit-identical to the raw mic.
//!
//! When AEC is enabled, the EC source is a plain virtual `Audio/Source`.
//! The mic filter-chain pins its capture side to this source with
//! `target.object = "echo-cancel-source"`, while `mic-biglinux` remains
//! the only WirePlumber smart source filter that apps see. A packaged
//! WirePlumber Lua hook then pins `echo-cancel-capture` to the currently
//! selected **physical** source at link time. That is the crucial split:
//! PipeWire's config stays generic, and WirePlumber follows microphone
//! changes live without ever hard-coding `alsa_input.*` names.
//!
//! [`AEC_NODE_LATENCY`] (= 20 ms) gives WebRTC exactly two 10 ms frames
//! per processing block. `libspa-aec-webrtc` rejects buffers that are
//! not an integer multiple of 10 ms; using a 1024-frame quantum here
//! causes ERR counters on `echo-cancel-source` under load.
//!
//! The block size trades graph wakeup cost against call latency. The AEC,
//! mic, and output filter chains all join the *same* PipeWire daemon, and the
//! mic chain pins `node.lock-quantum = true`. With the AEC loaded,
//! that lock pulls the **graph-wide** quantum down to whatever the
//! AEC declares, so every active node — including the always-on
//! `output-biglinux` smart filter chain (mixer + HPF + GTCRN + gate +
//! compressor + EQ) — wakes up at the AEC's rate rather than the
//! distro default's 24 Hz. With AEC off, the mic chain drops to the
//! 1024-frame default and the output chain processes only when apps
//! play. The CPU asymmetry users report ("turning on EC almost doubles
//! audio CPU") is exactly that wakeup-rate change. A bigger block
//! halves the wakeup count everywhere downstream, but it is also the
//! delay every model waits out — and it costs that twice, which is why
//! the constant settled at two frames instead of four. See
//! [`AEC_NODE_LATENCY`] for the measured latency ladder.
//!
//! WebRTC AEC tunables:
//!
//! - `noise_suppression = false` — GTCRN is far better at this and runs
//!   downstream.
//! - `high_pass_filter = true` — the chain's own biquad HPF sits
//!   *downstream* of the AEC, so it cannot clean what the adaptive
//!   filter sees. WebRTC's internal HPF removes DC/rumble before
//!   adaptation, which is what the canceller needs to converge on
//!   consumer mics; the audible voice shaping still belongs to the
//!   downstream biquad.
//! - `gain_control = false` — measured A/B on PipeWire 1.6.6
//!   (speech clip through speakers, raw mic vs `echo-cancel-source`
//!   recorded simultaneously, internal HPF on in both runs): echo
//!   attenuation was 10.5 dB with AGC off vs 1.5 dB with AGC on — the
//!   AGC re-amplifies the residual echo after cancellation. The internal
//!   HPF removes the DC/rumble that otherwise blocks convergence, so AGC
//!   is unnecessary.
//! - `voice_detection = true` — quality boost with no toggle benefit.
//!
//! `delay_agnostic` and `extended_filter` are accepted by older
//! WebRTC AEC builds but silently ignored by `libspa-aec-webrtc`
//! built against `libwebrtc-audio-processing-1` (PipeWire 1.6.x), so
//! we drop them — `strings libspa-aec-webrtc.so | grep ^webrtc\.`
//! lists the supported params on the running system.
//!
//! ## On-demand activation
//!
//! The module already defaults `node.passive = true` on its capture
//! and (monitor-mode) sink streams and gives every stream one shared
//! `node.link-group` — no passive props are needed here. But that
//! link-group is a hazard: with the reference tap linked to the
//! default sink's monitor, *any* playback drags the whole AEC group —
//! including the physical microphone — into RUNNING even when no app
//! records (verified on PipeWire 1.6.6: a suspended mic woke as soon
//! as the linked monitor had traffic). The fix lives in the packaged
//! WirePlumber hook (`echo-cancel-routing.lua`): an "AEC gate" keeps
//! `echo-cancel-capture` and `echo-cancel-sink` unlinked until at
//! least one real recording stream exists, and unlinks them again
//! when the last one goes away. Unlinked streams pause, the module
//! calls `spa_audio_aec_deactivate`, and the mic + APM idle at zero
//! cost; a linking rescan re-opens the gate when recording starts.

/// `node.name` of the virtual source created by the EC module.
pub const EC_SOURCE_NAME: &str = "echo-cancel-source";
/// Capture stream owned by the EC module. A WirePlumber Lua hook
/// targets this stream to the selected physical source at runtime.
pub const EC_CAPTURE_NODE_NAME: &str = "echo-cancel-capture";
/// WebRTC AEC processes 10 ms frames. At 48 kHz, 960 samples is two of
/// them, so the canceller still sees whole frames.
///
/// It was four frames, to halve the graph-wide wakeup rate that
/// `node.lock-quantum = true` imposes on every active node. Measuring the
/// microphone path end to end is what changed it: the block is not only a
/// wakeup interval, it is also the delay every model waits out before it can
/// start, and it costs that twice — once filling the block and once inside the
/// plugin's own pipeline.
///
/// | block | plugin | whole path |
/// | --- | --- | --- |
/// | 480 | 30 ms | 40 ms |
/// | 960 | 40 ms | 60 ms |
/// | 1920 | 60 ms | 100 ms |
///
/// Forty milliseconds off a call is the difference people describe as talking
/// over each other, and the models pay almost nothing for it: DPDFNet v2 goes
/// from 0.7 % of its deadline to 1.1 %, GTCRN from 30 % to 39 %, and both were
/// measured with the canceller loaded. Four hundred and eighty is better again
/// and is left alone: 10 ms wakeups across every node on the graph is a cost
/// the whole desktop pays, not just this chain.
///
/// The `plugin` column is the shape a plugin has when its own handoff follows
/// the block, which `dpdfnet-ladspa` 26.08.29 did and neither GTCRN nor a
/// rebuilt DPDFNet does — those are flat, 44 ms and 70 ms whatever the block.
/// So the case for 960 over 1920 is the fill alone now, and it still holds.
///
/// Re-measured after that, because 480 also stops a rebuilt DPDFNet blocking
/// its callback on its worker. Both chains were run in a real graph at 480, 960
/// and 1920 under sixteen CPU stressors and two memory stressors:
///
/// | quantum | GTCRN | DPDFNet | ERR | sample splices |
/// | --- | --- | --- | --- | --- |
/// | 480 | ok | ok | 0 | 0 |
/// | 960 | ok | ok | 0 | 0 |
/// | 1920 | ok | ok | 0 | 0 |
///
/// Nothing cut at any of the three, so reliability does not choose between them
/// on this machine, and the decision falls back to headroom on the machines we
/// do not have. That is what keeps 960: GTCRN runs its network **on the audio
/// thread**, one inference per callback whatever the block, so its cost is
/// absolute rather than proportional — p99 4.83 ms measured. At 480 that leaves
/// 5.2 ms of a 10 ms period, and the DeepFilterNet3 underruns already on record
/// here were single hops of 10.4–17.5 ms, every one of which a 480 deadline
/// would have missed outright. At 960 the same inference leaves 11.8 ms.
///
/// DPDFNet would prefer 480 (0 % of the period blocked against 20 %), but its
/// blocking turned out to be a scheduling problem rather than a block-size one:
/// with its worker at `SCHED_FIFO`, p99 under the same load is 31 % of the
/// period and every hop comes back enhanced. Fixed in the plugin, so 960 costs
/// it compute time and nothing else.
///
/// A per-model quantum is possible — switching models reloads the chain anyway,
/// so the lock is released and re-taken — and is deliberately not done: it would
/// make the wakeup rate of every node on the desktop depend on which microphone
/// model somebody picked.
pub(crate) const AEC_NODE_LATENCY: &str = "960/48000";
/// File name of the AEC args body, consumed by
/// `biglinux-microphone-pwloader` (started by
/// `biglinux-microphone-aec.service`). The unit is started before
/// `biglinux-microphone-mic.service` (`After=`) so `echo-cancel-source`
/// already exists when the mic filter chain resolves its
/// `target.object`.
pub const ECHO_CANCEL_CONF_FILE: &str = "aec.args";

/// Render the AEC `args` body for `libpipewire-module-echo-cancel`,
/// consumed verbatim by `biglinux-microphone-pwloader`.
///
/// The output is just the `{ … }` block — no `context.modules`
/// wrapper, no comment header. The pwloader passes it straight to
/// `pw_context_load_module(libpipewire-module-echo-cancel, …)`. The
/// AEC module pins its own latency via [`AEC_NODE_LATENCY`]
/// (= 20 ms — two WebRTC frames), which is what `libspa-aec-webrtc`
/// requires regardless of the daemon's quantum.
///
/// The capture stream intentionally has no static `target.object`: a
/// WirePlumber Lua policy hook chooses the selected physical source
/// live, so the config stays portable across machines with different
/// or hot-swapped microphones. The reference side runs in
/// `monitor.mode = true`, tapping the monitor of the default sink
/// without exposing an Audio/Sink to clients — apps continue to play
/// to the real speaker.
pub(super) fn build_echo_cancel_conf() -> String {
    format!(
        "{{\n\
         \x20   library.name = aec/libspa-aec-webrtc\n\
         \x20   node.latency = {AEC_NODE_LATENCY}\n\
         \x20   monitor.mode = true\n\
         \x20   audio.rate = 48000\n\
         \x20   audio.channels = 1\n\
         \x20   audio.position = [ MONO ]\n\
         \x20   buffer.max_size = 250\n\
         \x20   capture.props = {{\n\
         \x20       node.name    = \"{EC_CAPTURE_NODE_NAME}\"\n\
         \x20   }}\n\
         \x20   source.props = {{\n\
         \x20       node.name        = \"{EC_SOURCE_NAME}\"\n\
         \x20       node.description = \"BigLinux Echo-Cancelled Mic\"\n\
         \x20       media.class      = Audio/Source\n\
         \x20       volume           = 1.0\n\
         \x20   }}\n\
         \x20   aec.args = {{\n\
         \x20       webrtc.gain_control       = false\n\
         \x20       webrtc.noise_suppression  = false\n\
         \x20       webrtc.high_pass_filter   = true\n\
         \x20       webrtc.voice_detection    = true\n\
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
        assert!(conf.contains("aec/libspa-aec-webrtc"));
    }

    #[test]
    fn conf_exposes_named_source_in_monitor_mode() {
        // monitor.mode = true means no Audio/Sink virtual device is
        // created; the sink-side stream taps the default sink monitor
        // directly. Only the cleaned source node is visible to the rest
        // of the graph as Audio/Source. We deliberately leave sink.props
        // unset — the module's monitor.mode defaults are what works.
        let conf = build_echo_cancel_conf();
        assert!(conf.contains(&format!("\"{EC_SOURCE_NAME}\"")));
        assert!(conf.contains("Audio/Source"));
        assert!(conf.contains("monitor.mode = true"));
        assert!(
            !conf.contains("sink.props"),
            "monitor.mode defaults must not be overridden — verified to break AEC convergence",
        );
        assert!(
            !conf.contains("media.class      = Audio/Sink"),
            "monitor.mode=true must not declare a virtual AEC sink",
        );
    }

    #[test]
    fn conf_uses_webrtc_frame_compatible_mono_format() {
        // WebRTC AEC processes 10 ms frames. 960/48000 is exactly two of
        // them; 1024/48000 would make libspa-aec-webrtc return errors
        // under load.
        //
        // Two frames rather than four, because the block is also the delay
        // every model waits out, and it costs that twice: once filling the
        // block and once inside the plugin. Measured end to end, the whole
        // microphone path is 60 ms here against 100 ms at four frames, and
        // the models pay under a percent of their deadline for it.
        //
        // `audio.channels = 1` + `audio.position = [ MONO ]` keep the
        // AEC mono. The mic is mono and `libspa-aec-webrtc` expects
        // ref and capture channel counts to match. Stereo content from
        // the physical speaker monitor is downmixed to mono by
        // PipeWire's audioconvert when WirePlumber links the stereo
        // sink monitor (FL+FR) to this mono input — both channels
        // reach the canceller, just averaged.
        let conf = build_echo_cancel_conf();
        assert!(conf.contains("node.latency = 960/48000"));
        assert!(conf.contains("audio.rate = 48000"));
        assert!(conf.contains("audio.channels = 1"));
        assert!(conf.contains("audio.position = [ MONO ]"));
        assert!(conf.contains("buffer.max_size = 250"));
    }

    #[test]
    fn conf_enables_internal_hpf_disables_agc_and_ns() {
        let conf = build_echo_cancel_conf();
        // GTCRN owns denoising/voice shaping downstream. The internal
        // HPF must stay on: the chain's biquad HPF is downstream of the
        // AEC, so only WebRTC's own HPF can remove DC/rumble before the
        // adaptive filter sees the signal. AGC must stay off — measured
        // A/B (module docs): 10.5 dB echo attenuation without AGC vs
        // 1.5 dB with it (the AGC re-amplifies residual echo).
        assert!(conf.contains("webrtc.gain_control       = false"));
        assert!(conf.contains("webrtc.noise_suppression  = false"));
        assert!(conf.contains("webrtc.high_pass_filter   = true"));
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
