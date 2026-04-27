//! Echo-cancellation filter generator.
//!
//! Use case: the user is on a meeting/call **without headphones**, so
//! speaker output bleeds back into the microphone. Acoustic echo
//! cancellation (AEC) subtracts the reference signal from the captured
//! mic before any other processing — including GTCRN — so the neural
//! denoiser doesn't waste capacity attenuating audio it later has to
//! restore.
//!
//! The chain runs in its own `pipewire -c` process (managed by
//! `biglinux-microphone-echocancel.service`) for the same reason the
//! output chain does: keeping it isolated avoids module conflicts and
//! lets the user toggle AEC on/off without restarting the system
//! PipeWire daemon.
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
//! `node.latency = 960/48000` (= 20 ms) gives WebRTC exactly two 10 ms
//! frames per processing block. `libspa-aec-webrtc` rejects buffers
//! that are not an integer multiple of 10 ms; using the mic chain's
//! regular 1024-frame quantum here causes ERR counters on
//! `echo-cancel-source` under load.
//!
//! WebRTC AEC tunables:
//!
//! - `noise_suppression = false` — GTCRN is far better at this and runs
//!   downstream.
//! - `high_pass_filter = false` — our biquad HPF in the mic chain
//!   already cuts rumble; doubling up would over-attenuate low voice.
//! - `gain_control = true` — required for the AEC's adaptive filter to
//!   converge on speech-level reference; with it off the canceller
//!   passes the mic through unchanged (verified by recording raw mic
//!   and `echo-cancel-source` simultaneously while a voice clip
//!   played through the speakers).
//! - `voice_detection = true` — quality boost with no toggle benefit.
//!
//! `delay_agnostic` and `extended_filter` are accepted by older
//! WebRTC AEC builds but silently ignored by `libspa-aec-webrtc`
//! built against `libwebrtc-audio-processing-1` (PipeWire 1.6.x), so
//! we drop them — `strings libspa-aec-webrtc.so | grep ^webrtc\.`
//! lists the supported params on the running system.

use crate::config::AppSettings;

/// `node.name` of the virtual source created by the EC module.
pub const EC_SOURCE_NAME: &str = "echo-cancel-source";
/// Capture stream owned by the EC module. A WirePlumber Lua hook
/// targets this stream to the selected physical source at runtime.
pub const EC_CAPTURE_NODE_NAME: &str = "echo-cancel-capture";
/// WebRTC AEC processes 10 ms frames. At 48 kHz, 960 samples is two
/// frames and still leaves enough headroom for the downstream GTCRN
/// filter-chain when AEC feeds it.
pub(crate) const AEC_NODE_LATENCY: &str = "960/48000";
/// File name of the standalone pipewire config — same convention as
/// `biglinux-microphone-output.conf`, no directory prefix.
pub const ECHO_CANCEL_CONF_FILE: &str = "biglinux-microphone-echocancel.conf";

/// True when the EC chain is wanted by the current settings. Centralised
/// so [`super::apply_to_dirs`] and [`super::mic`] read the same flag.
#[must_use]
pub fn echo_cancel_wanted(settings: &AppSettings) -> bool {
    settings.echo_cancel.enabled
}

/// Render the standalone pipewire config that hosts the EC module.
#[must_use]
pub fn build_echo_cancel_conf(_settings: &AppSettings) -> String {
    // The EC module's capture stream intentionally has no static
    // `target.object`: a WirePlumber Lua policy hook chooses the
    // selected physical source live. Leaving this in PipeWire's config
    // avoids hard-coding device names for machines with different or
    // hot-swapped microphones. The reference side runs in
    // `monitor.mode = true`, which makes the module tap the monitor of
    // the default sink without exposing an Audio/Sink to clients —
    // apps continue to play to the real speaker.
    format!(
        "# BigLinux Microphone — auto-generated AEC config\n\
         # DO NOT EDIT: this file is rebuilt every time settings change.\n\
         \n\
         context.properties = {{\n\
         \x20   log.level = 0\n\
         \x20   default.clock.rate          = 48000\n\
         \x20   default.clock.quantum       = 960\n\
         \x20   default.clock.min-quantum   = 480\n\
         \x20   default.clock.max-quantum   = 1920\n\
         }}\n\
         \n\
         context.spa-libs = {{\n\
         \x20   audio.convert.* = audioconvert/libspa-audioconvert\n\
         \x20   support.*       = support/libspa-support\n\
         }}\n\
         \n\
         context.modules = [\n\
         \x20   {{ name = libpipewire-module-rt\n\
         \x20       args = {{\n\
         \x20           nice.level    = -11\n\
         \x20           rt.prio       = 88\n\
         \x20       }}\n\
         \x20       flags = [ ifexists nofail ]\n\
         \x20   }}\n\
         \x20   {{ name = libpipewire-module-protocol-native }}\n\
         \x20   {{ name = libpipewire-module-client-node }}\n\
         \x20   {{ name = libpipewire-module-adapter }}\n\
         \x20   {{ name = libpipewire-module-echo-cancel\n\
         \x20       args = {{\n\
         \x20           library.name = aec/libspa-aec-webrtc\n\
         \x20           node.latency = {AEC_NODE_LATENCY}\n\
         \x20           monitor.mode = true\n\
         \x20           audio.rate = 48000\n\
         \x20           audio.channels = 1\n\
         \x20           buffer.max_size = 250\n\
         \x20           capture.props = {{\n\
         \x20               node.name    = \"{EC_CAPTURE_NODE_NAME}\"\n\
         \x20           }}\n\
         \x20           source.props = {{\n\
         \x20               node.name        = \"{EC_SOURCE_NAME}\"\n\
         \x20               node.description = \"BigLinux Echo-Cancelled Mic\"\n\
         \x20               media.class      = Audio/Source\n\
         \x20               volume           = 1.0\n\
         \x20           }}\n\
         \x20           aec.args = {{\n\
         \x20               webrtc.gain_control       = true\n\
         \x20               webrtc.noise_suppression  = false\n\
         \x20               webrtc.high_pass_filter   = false\n\
         \x20               webrtc.voice_detection    = true\n\
         \x20           }}\n\
         \x20       }}\n\
         \x20       flags = [ ifexists nofail ]\n\
         \x20   }}\n\
         ]\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EchoCancelConfig;

    fn enabled() -> AppSettings {
        AppSettings {
            echo_cancel: EchoCancelConfig { enabled: true },
            ..AppSettings::default()
        }
    }

    #[test]
    fn defaults_on() {
        // EC defaults to enabled — covers the laptop-without-headphones
        // happy path; users with headphones or isolated mics opt out via
        // Advanced view.
        assert!(echo_cancel_wanted(&AppSettings::default()));
    }

    #[test]
    fn enabled_flag_is_honoured() {
        assert!(echo_cancel_wanted(&enabled()));
    }

    #[test]
    fn conf_loads_webrtc_aec_module() {
        let conf = build_echo_cancel_conf(&enabled());
        assert!(conf.contains("libpipewire-module-echo-cancel"));
        assert!(conf.contains("aec/libspa-aec-webrtc"));
    }

    #[test]
    fn conf_exposes_named_source_in_monitor_mode() {
        // monitor.mode = true means no Audio/Sink virtual device is
        // created; the sink-side stream taps the default sink monitor
        // directly. Only the cleaned source node is visible to the rest
        // of the graph as Audio/Source. We deliberately leave sink.props
        // unset — the module's monitor.mode defaults are what works.
        let conf = build_echo_cancel_conf(&enabled());
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
        // WebRTC AEC processes 10 ms frames. 960/48000 is exactly two
        // frames; 1024/48000 would make libspa-aec-webrtc return errors
        // under load.
        let conf = build_echo_cancel_conf(&enabled());
        assert!(conf.contains("default.clock.quantum       = 960"));
        assert!(conf.contains("node.latency = 960/48000"));
        assert!(conf.contains("audio.rate = 48000"));
        assert!(conf.contains("audio.channels = 1"));
        assert!(conf.contains("buffer.max_size = 250"));
    }

    #[test]
    fn conf_enables_agc_disables_ns_and_hpf() {
        let conf = build_echo_cancel_conf(&enabled());
        // GTCRN owns denoising/voice shaping downstream. AGC must stay
        // on — without it the WebRTC AEC adaptive filter does not
        // converge and the canceller passes mic through unchanged.
        assert!(conf.contains("webrtc.gain_control       = true"));
        assert!(conf.contains("webrtc.noise_suppression  = false"));
        assert!(conf.contains("webrtc.high_pass_filter   = false"));
    }

    #[test]
    fn conf_runs_with_realtime_module() {
        let conf = build_echo_cancel_conf(&enabled());
        assert!(conf.contains("libpipewire-module-rt"));
        assert!(conf.contains("libpipewire-module-protocol-native"));
        assert!(conf.contains("libpipewire-module-adapter"));
    }

    #[test]
    fn conf_leaves_capture_target_to_wireplumber_policy() {
        let conf = build_echo_cancel_conf(&enabled());
        assert!(conf.contains(&format!("node.name    = \"{EC_CAPTURE_NODE_NAME}\"")));
        assert!(
            !conf.contains("target.object"),
            "AEC capture target is chosen live by the WirePlumber Lua hook",
        );
    }
}
