# Integration, live acceptance and small UI contract

This document is addressed to the implementation agent. The supplied `code/` is a
partial snapshot, not a Cargo workspace. Use the complete repository and its
already installed toolchain. Do not install a toolchain as part of applying this
bundle. Build results below are requirements, not results claimed by the reviewer.

## 1. Apply without losing existing work

Start a clean integration branch based on the exact supplied snapshot. Preserve
unrelated local modifications before applying. From the bundle directory:

```sh
./apply.sh /path/to/project --check
./apply.sh /path/to/project
./tests/run-core-rust.sh /path/to/project
```

The last command compiles the actual patched, `std`-only controller, extractor and
policy in an isolated test harness. Expected discovery: 54 tests (20 original
policy tests plus 34 new core tests). It does not test `agc.rs`'s external I/O,
Serde, PipeWire argument construction, or source parsing.

In the complete workspace, use its normal feature selection and locked dependencies:

```sh
cargo fmt --all
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Run any additional project-specific feature/build/package checks. In particular,
compile the CLI AGC entry point and run the three additional new Rust tests in
`services/pipewire/sources.rs`, `services/audio_monitor/capture.rs` and
`config/audio.rs`. `rustfmt` was not run during this review: formatting adjustments
may be needed. Do not weaken existing safety assertions to make a build pass.

No dependency or model-weight additions are required. No LADSPA ABI or real-time
callback is changed. `reviewed-code/` is a comparison copy only: do not overwrite a
complete repository with this partial tree.

## 2. Confirm the actual control topology first

Verify that the real source selected by `snapshot_sources()` is also the raw input
of the microphone filter chain. The snapshot excludes the graph/session policy
that could choose a different device. A pinned raw meter must not accidentally
measure `mic-biglinux`, a monitor-of-output, a different default device, a gated
virtual source, or a source behind another AGC.

Inspect the live links, node names and serials. This patch selects `pw-cat` targets
by `node.name`; a PipeWire object ID is not interchangeable with an object serial.
The explicit-target meter cannot silently fall back or move. [S4, S5]

Check positive and negative capture-volume changes against the raw meter. Software
volume should approximately follow `60*log10(new_fraction/old_fraction)` after the
capture pipeline settles. Hardware capture steps can be coarse or nonlinear;
readback is not proof of an exact acoustic gain change. Whether ALSA hardware
controls are used depends on device/profile and `api.alsa.soft-mixer`, not simply
whether the visible knob is below 100%. [S2, S3, S5]

The supplied capture path has no frame-associated volume epoch or capture timestamp.
Consequently, a frame buffered before a volume change can briefly be normalized
using the new readback. The numerical delay case is useful stress testing, not a
proof of synchronization. Measure response and stability with the actual latency,
under CPU load, and after profile/rate changes. This patch keeps the existing
`pw-cat` latency rather than silently changing global capture behavior.

## 3. Collect evidence without changing audio settings

Use the explicit raw node ID shown by `wpctl status -n`. Discover the actual user
service unit from the project; this bundle does not invent its name. For example:

```sh
# Replace NODE_ID and YOUR_AGC_UNIT with the real values.
./tests/live-observe.sh NODE_ID 180 YOUR_AGC_UNIT
# Long-run rejection phase:
./tests/live-observe.sh NODE_ID 660 YOUR_AGC_UNIT
```

The observer records versions, device metadata, volume/mute readback twice per
second, and optionally the existing service journal. It does not record PCM,
change volume, start or stop services, or upload anything. Review device names and
logs before sharing. A timed-out read appears as `READBACK_ERROR`.

Enable the application's existing debug-logging mechanism. The entry point named
in the supplied request is `biglinux-microphone-cli agc`; do not start a second
copy while the service is already running. A duplicated controller, conferencing
application AGC, or another source-volume manager can invalidate the experiment.
Temporarily disable competing AGC for isolated tests, then explicitly test its
coexistence behavior. Do not change unrelated service configuration blindly.

Use headphones, comfortable playback volume, and no live microphone-to-loudspeaker
loop. Test high input by choosing an existing safe capture setting, not by making
uncomfortably loud sounds. Use each microphone in its intended position: a webcam
on a monitor must work from the normal seat, not only directly against the mouth.

Record timestamps for phase changes and the actual readback. The debug `gain` is
policy state, and `vol X→Y` is the current/desired command state; neither proves a
hardware write succeeded until readback. `speech@base` and `noise` are normalized
peak-reference estimates, not LUFS. `p` is a binary evidence permission, not a
calibrated neural speech probability. `snr` is raw peak/background contrast.

## 4. Live acceptance matrix

### A. Idle, mute and initial volume

Start with Auto off: there must be no AGC capture child and no AGC writes. Turn Auto
on with the mic muted, then with its source volume explicitly zero. It must not
unmute, jump to 100%, or raise the zero setting. Expect `muted` and no positive
writes. Run unmuted room noise for at least five seconds before speaking: steady
noise must not enroll a primary or boost.

Also start at a low nonzero volume, such as 30%, on a genuinely quiet mic. A raw
noise reading at the analyzer's -120 dBFS floor is treated as a finite upper bound
(`noise_bound=true`), not infinite SNR or explicit source mute. Measurable normal
speech with natural gaps can still acquire a reference and increase volume. If all
speech is also below the numeric meter floor, the available measurement contains
no basis for autonomous boost; report that condition instead of a blind kickstart.
Never interpret denoiser/gate-created digital zero as this raw-noise bound.

### B. Normal speech and weak microphone

After a short background interval, speak naturally for 10–30 seconds with normal
pauses. Acquisition requires at least 1.2 seconds of eligible evidence within the
2.4-second window, plus modulation and contrast. This is accumulated eligible
speech time, not a promise to calibrate 1.2 seconds after clicking Auto.

Expect `p=1`, `tracking_speech`, a primary reference and gradual positive writes
when the voice is too quiet. `speech@base` should remain approximately stable for
a stable voice while the actual amplified level changes. The default robust
peak-reference target is -12 dBFS with a ±4 dB policy comfort band. Small additional
quantization error is possible. This is not a sample-peak limiter or a perceived
loudness guarantee.

Test the AKG C44-USB, the Lenovo FHD webcam, and held-out devices without changing
detector constants. A sufficiently weak input must be able to reach **3.00**;
there is no artificial lower ceiling. If the physical maximum is still inadequate,
the expected result is a stable limit, not a false report that target was reached.
If the raw noise ceiling binds first, the voice can correctly remain below target.

### C. Long-run quieter background speech

Establish the user's primary first. Then have the user stop and a more distant
person speak alone for at least ten minutes. Verify in the trace that this person's
normalized peaks remain more than 10 dB below the learned primary. Expect `p=0`,
`frozen_no_trusted_speech`, no upward volume drift, and a primary that does not
leak downward with elapsed time. High raw contrast alone must not override the
primary requirement. Protective reductions may still occur for excessive peaks.

The level condition matters: a loud distant person, nearby television or amplified
loudspeaker can violate it. Do not label this test a universal distance detector.
Repeat during monitor reconnects to verify that the primary reference survives.

### D. Noise, transients, pauses and changed voice

Test steady fan noise, a sustained noise-level step, and a single brief click or
cough. Steady unmodulated input must not acquire a primary; a single brief cough
must not authorize sustained boosting or become a permanent single-peak anchor.
During actual speech pauses, stale policy boost must not be written merely because
an earlier frame had permission. The current frame must remain eligible.

Also test keyboard bursts, music, and strongly pulsed machinery. Document failures:
level modulation is not a real speech classifier, and patterned non-speech can
satisfy the heuristic. Quiet voices, some speech impairments, whispered or nearly
constant-level speech may fail the variation/contrast criteria. Their sound must
continue passing through the audio chain; this extractor controls gain, not a gate.
Provide manual level/explicit guided recalibration rather than repeated failed
instructions to speak louder.

A sustained shout can raise the retained reference. It intentionally does not
self-decay afterward. Verify the explicit relearn flow after voice/position changes;
record this trade-off in product documentation.

### E. Fast downward response

Start from a safely chosen excessive capture setting and speak normally. A current
peak above `min(target + 4, -3)` authorizes a rate-limited downward correction even
when `p=0`. At the default target this threshold is -8 dBFS. The configured maximum
fall is 14 dB/s; upward motion remains capped at 8 dB/s. Actual command quantization,
I/O delay and 5 Hz write coalescing affect observed steps.

Confirm no waiting for the sustained-speech window is necessary for this cut. Test
manual hold separately: it takes precedence, and must not be silently overridden.
The off-RT loop cannot repair ADC clipping or constrain the first transient. Verify
the real DSP limiter independently as described below.

### F. User ownership and enable/disable

While Auto is tracking, move the source-volume slider externally. It must rebaseline,
discard its old gain correction and enter `manual_hold` indefinitely rather than
fight the user or reapply a saved correction. The retained acoustic reference is
shifted into the new baseline. Current-peak protection must also respect the hold.

Rearm deliberately: turn Auto off, leave it off for at least two seconds so the
one-second settings poll observes it, then turn it on. Changing only the target
slider is not a rearm command. An off/on toggle entirely between polls is not
observable with the current settings API; the UI should not depend on that race.

Mute/unmute during operation: no unmute commands, no blind gain increase, fresh
background/evidence after resumption. Another application's capture-volume change
is treated conservatively as external ownership, not identified as the human.

### G. Device loss, metadata failure and capture recovery

Switch between real default sources and verify that a new source starts a new
reference. Unplug/replug devices, restart capture, and exercise temporary missing
default metadata. For a known unchanged source, a recoverable monitor interruption
must not erase the primary or clear manual hold. A stale frame cannot be repeatedly
counted as fresh speech. Expect `stale_audio`, `source_unavailable`,
`readback_unavailable`, or `monitor_recovering` with no writes from stale evidence.

Check the pinned stream never falls back to a speaker monitor or another microphone.
The CLI design still has non-atomic object-ID/read/write races and synchronous
subprocess latency. Rapid ID reuse, blocked subprocesses and concurrent metadata
changes require integration testing. They are not proved safe by pure unit tests.

### H. Quantization, failed ACK and backend limits

Exercise a coarse capture control and a source that refuses the full requested
range. The controller must accumulate small changes rather than stall forever at
one readback quantum; it must also stop repeating an identical maximum request when
the backend does not deliver it (`actuator_limited`). This is acknowledgement of
that device's behavior, not a globally reduced 300% limit.

A failed write or missing/muted post-write readback enters `actuator_fault`; require
an explicit Auto cycle before more autonomous writes. No silent endless retry.
Measure subprocess load: decisions remain 20 Hz, write transactions at most 5 Hz,
and each transaction can involve multiple `wpctl` calls. This is not a native
persistent PipeWire control client. Check CPU use, wakeups, xruns and latency under
real deployment load before accepting the resource budget.

### I. End-to-end filter output and competing applications

Test standard/high DFN3, DPDFNet, denoiser bypass, strength changes, silence-floor
curves, gate, compressor and EQ. The AGC raw-noise estimate must not be sourced from
the denoised/gated virtual source. Validate the app actually records `mic-biglinux`
and that final perceived voice level is useful after the entire chain.

The supplied `pipeline/mic.rs` references `gain_safety`, but the implementation of
that module was not supplied. Verify its real limiter placement, headroom, clipping
behavior, makeup gain, and bypass semantics in the complete repository. A final
DSP limiter does not undo input ADC clipping. A future output-level observer could
report quality/overload, but a second independent AGC must not fight this source
controller. Do not put a denoised -120 dBFS noise value into the raw noise ceiling.

### J. Mandatory negative result: cold-start TV ambiguity

Start without a learned primary and present a TV or another person's speech with
the same level/contrast/modulation as a close user. It may be enrolled and boosted.
The deterministic tests intentionally retain an indistinguishable-trace example.
This is a known functional boundary, not a test to weaken or conceal. A claim that
Auto never boosts television or any distant person would be unsupported.

## 5. Minimal UI contract for ordinary users

The actual UI was not supplied. These are integration requirements/recommendations,
not widget changes claimed by the patches.

Keep the primary screen to **Automatic microphone level**, **Test microphone**,
and an optional simple **Voice level** control. A three-label presentation such as
“Softer — Normal — Louder” is easier to explain than negative dBFS values. Keep a
manual alternative available, particularly for quiet/atypical speech. Do not add
SNR, primary margin, gain speed, VAD thresholds or noise-floor sliders to this screen.

The test flow should ask for a brief quiet interval and a normal short sentence
from the usual position. Explain why: “This helps us set the level for your voice.”
For ambiguous cold starts, offer explicit setup/relearning rather than claiming
speaker identification. A deliberate owner-controlled setup establishes intent;
it does not turn later amplitude comparisons into voice biometrics.

Show plain-language states, with text rather than color alone: “Speak normally to
check your level,” “Automatic level is active,” “Using the level you selected,”
“Microphone is muted,” or “Automatic adjustment paused.” When noise/physical gain
limits prevent adequate level, explain positioning or microphone selection instead
of presenting a successful check or demanding an impossible louder setting.

An explicit “Check again” action should eventually have a real acknowledged rearm
request/generation in the complete UI/service API. This patch's API observes an
Auto-off/Auto-on transition through periodic settings polling; do not implement an
instant invisible off/on toggle and assume it was seen. The guided setup/reference
is session-local here, not a persisted identity enrollment feature.

Use the exported target constants for percentage mapping. The existing 70–100%
UI endpoints remain, but their dB range is now -18..-6, with 85% at -12. Remove any
hardcoded old slope. Existing explicitly stored values within the new range remain
unchanged; an old stored -7.5 is not silently migrated to -12. Out-of-range old
values are bounded by the controller. A reset-to-default action should show the
new default honestly. Auto's default `enabled=false` is unchanged.

## 6. Release decision

Do not call this hardware-validated or ready for a universal automatic-default
rollout until the Rust builds/tests and the live matrix pass. Record both supplied
mics and held-out devices, several rooms/distances, speech characteristics and
language samples. Report acquisition rate, gain drift while absent, recovery,
false enrollment, clipping, resource use and comfort—not just “unit tests green.”

If the product requirement remains “never boost any TV or distant voice, even on
first use,” this level-only design does not meet it. Keep the conservative control
fixes, and add an explicit intent/enrollment or adequately validated target-speaker
mechanism before making that guarantee. See [S6] and the optional research route
[S9, S10].
