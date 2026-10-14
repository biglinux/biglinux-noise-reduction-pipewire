# AGC rationale

References `[S#]` are expanded, with URLs and verification scope, in `SOURCES.md`.
All thresholds below are global engineering starting points, not values derived
from the two calibration microphones or a claim of universal acoustic validation.

## 1. Separate three different questions

The controller must distinguish: **may gain increase?**, **what level should be
tracked?**, and **must excessive level be reduced now?** The old implementation
used an instantaneous peak, an old noise minimum and a decaying loud reference as
if they answered all three. A quieter speaker eventually passed a reference which
leaked downward; the 0.5-second hold also labeled actual pauses as trusted speech.

The new extractor requires accumulated, varying, above-background observations.
Its robust primary reference never decays downward in a source session. A pause
preserves the reference but supplies no authorization to increase gain. A separate
current-peak guard can reduce gain without waiting for acquisition. WebRTC AGC2
likewise separates permission to increase gain from gain reduction, noise limits
and headroom; this patch is not a port of its VAD or a claim of equivalent accuracy.
[S1]

## 2. Normalize the observations, not the finished history

Every raw-source sample-peak observation is transformed as:

```
peak_at_base = measured_peak - (actual_applied_volume_db - baseline_volume_db)
```

Only then is it used by the noise estimate, evidence window and primary reference.
Subtracting the current gain from an envelope that already mixes older gain states
cannot repair that history. Actual successful readback, not a requested floating
point value, updates the adapter's applied volume.

The wpctl conversion remains `dB = 60*log10(v)`. Its tool selects the cubic mixer
scale, and the mixer converts that value to linear amplitude by cubing it. Thus
300% is approximately **+28.627 dB**, not +9.542 dB. Whether a device implements
part of a volume adjustment in hardware is driver/profile dependent; a knob below
100% does not by itself prove pre-ADC gain. [S2, S3, S5]

The monitor does not expose a hardware capture timestamp. This normalization is
therefore not sample-accurate command/PCM alignment. The numerical suite includes
a 150 ms gain-delay stress case, but real driver latency and non-linear hardware
volume response still require measurement. Do not infer a verified ADC transfer
function merely from the correct wpctl curve.

## 3. Relative evidence, with bounded memory

| Setting | Value | Reason / trade-off |
|---|---:|---|
| Background evidence | 0.5 s | Avoid making the first loud frame a complete calibration. |
| Evidence window | 2.4 s | Accumulate speech-like dynamics across short word gaps. |
| Eligible observed time | 1.2 s | Reject brief clicks/coughs as calibration events. |
| Acquisition contrast | 18 dB | Require a conservative margin above the raw background estimate. |
| Tracking contrast | 12 dB | Permit normal variation once an anchor exists. |
| Current level below primary | At most 10 dB | Quieter input outside this relative window cannot authorize a rise. |
| Weighted 80th−20th percentile spread | At least 3 dB | Reject constant levels and small stationary jitter. |
| Primary statistic | Weighted 80th percentile | A single exceptional peak does not become the reference. |
| Noise fall / rise time constants | 0.15 / 10 s | Follow quieter background promptly; absorb a new steady fan cautiously. |
| Storage bound | 256 observations | Bounded off-RT memory; normal 20 Hz usage needs about 48 observations. |

`p=1` means **permission under this heuristic**, not a calibrated probability of
speech or an assertion of speaker identity. `snr` is current peak-to-background
contrast, not a physical separation of speech and noise powers. The gate affects
AGC decisions only: rejection does not mute the audio or discard speech samples.

The primary statistic can rise only with sufficient evidence and never decreases
merely because its owner is absent. A sustained much louder utterance or moved mic
can establish a higher reference. Consequently a long shout, a different speaking
position or genuinely much quieter subsequent speech may require an explicit
relearn. This is the deliberate alternative to silently forgetting the owner.

Sensitivity offsets shift the stored levels equally; the permission rules depend
on differences. This does not make the hardware's available gain, raw noise, DSP,
frequency response or effective signal-to-noise ratio identical across microphones.

### Very quiet inputs and the analyzer floor

The analyzer reports at most -120 dBFS for very small amplitudes. On a genuinely
raw, ungated source this is **censored noise data**: the noise is no greater than
that bound at the current capture gain. It is not negative infinity and is not the
same as a source mute. The patch reports `noise_bound=true`, uses a conservative
finite bound, and still requires measurable, sustained, modulated input to boost.
Zero alone never authorizes a rise. This lets, for example, a 30%-volume mic with
measurable speech calibrate without first jumping to 100%.

An explicit PipeWire mute or zero volume does not supply calibration evidence and
is never automatically overridden. Missing frames are not silence samples. A
post-denoiser or gated signal cannot supply the raw-noise bound described above.

## 4. Gain control, limits and actuator ownership

The established `GainPolicy` noise ceiling, manual hold and hardware request/ACK
protocol remain in place. All 20 original policy tests are preserved unchanged.
The source adapter uses software-policy mode; it never fabricates verified ADC
clipping evidence from the post-volume meter.

The new peak guard cuts toward `min(target + 4 dB, -3 dBFS)`, using the existing
non-linear downward rate range up to **14 dB/s**. It cannot authorize a boost,
bypass mute/manual ownership, or issue a hardware clipping request. It is a slow
control guard, not a real-time limiter or an inter-sample true-peak measurement.

A second correction fixes saturation: the rate/deadband calculation uses the
unclamped level error, while the applied step remains bounded by the available
actuator range. Otherwise an input needing more than the available gain stopped
4 dB short of 300%. The final quantized step to the physical limit is allowed even
when it is smaller than the ordinary 0.4 dB write threshold.

wpctl prints volume to two decimals, and this project's setter uses the same
precision. Requests are quantized accordingly. Changed readback reconciles the
policy. An unchanged coarse readback does not erase every small pending increment;
a different request can accumulate until the next hardware step. Identical
acknowledged requests are not retried forever at a refusing backend's maximum.
`actuator_limited` exposes that condition rather than claiming the requested gain
was applied. Quantized hardware steps can exceed a single policy tick's dB step;
the policy rate bound is not a claim of infinitesimal physical fader resolution.

The control loop still evaluates at about 20 Hz; CLI write/readback transactions
are coalesced to at most 5 Hz. It checks mute and external volume immediately
before a write and reads the result afterward. This reduces unnecessary CLI churn
but does not make the CLI protocol atomic or remove synchronous subprocess waits.
A native object/property subscription would be the stronger long-term backend.

A manual volume change resets the old correction to zero, rebases coordinates,
and enters explicit manual hold. Reconnects and silence do not rearm it. Auto off,
observed by the settings poll, followed by Auto on creates a new calibration.
Monitor recovery preserves the primary reference and manual hold; a new source
gets independent calibration. Process restarts do not persist speaker references.

## 5. Why raw metering, not denoised metering or “boost effectiveness”

Use the exact raw source being controlled, bound by node name and without fallback.
The default-source alias is not a reliable identity shared by separately created
streams; pw-cat documents node names or object serials as explicit targets. [S4,S5]

The denoised output is useful for listening and end-to-end quality checks, but its
silence floor is not the physical microphone's noise floor. Adding a second FFT
monitor would not identify the owner. Avoid that additional continuous workload
until it answers a specific output-quality question.

For linear gain, `y=G(s+n)`. Dividing by G restores the same pre-gain signal for a
close speaker, a distant speaker, noise or a television. Their pre-gain level does
not rise because the controller raised G. Failure of that level to rise therefore
cannot be a valid “distant speaker” detector. Post-gain response can reveal an
ineffective actuator, not speaker identity. Analog gain may change electronic
noise/quantization contributions, but does not selectively improve the room's
acoustic direct-voice-to-background ratio.

With the retained -50 dBFS noise-ceiling target, a -12 dBFS speech target requires
about 38 dB of speech/background contrast for both desired levels to coexist.
The existing 4 dB comfort zone makes this a soft steady-state window, not a strict
instantaneous noise guarantee. A low-SNR microphone cannot simultaneously satisfy
arbitrary voice loudness and low raw noise by gain alone. The correct response is
to explain the constraint, not quietly remove the noise guard.

## 6. Limits and the route to stronger discrimination

Identical observations must produce identical decisions. In particular, unknown
microphone sensitivity and room propagation can make a weak nearby voice and a
stronger remote voice observationally indistinguishable to this controller. A TV
at cold start can therefore still be enrolled. The suite deliberately contains
that counterexample. Target-speaker methods use additional identifying clues,
such as enrollment speech, spatial information or visual speech activity. [S6]

The included DFN3 and DFN3LL sources map their LSNR head into **-15..35 dB**; this
is not a calibrated P(speech). DPDFNet's supplied LADSPA interface exposes input,
output, attenuation limit, latency and processing-fault ports, not the same LSNR
signal. See their exact file anchors in `SOURCES.md`.

No denoiser/RT ABI changes are included. A process-local atomic in the PipeWire
plugin is not shared with the CLI, and an output LADSPA control port does not by
itself create a wpctl-readable service. A future telemetry design needs an explicit
host bridge or preallocated IPC contract, source/instance epoch, timestamp,
sequence, units, validity and model capabilities. Only bounded nonblocking writes
belong in the audio callback; lifecycle and reading remain off-RT. [S7]

For stronger automatic distance evidence, Fast-SDE (June 2026) is a research
candidate, not an assumed drop-in classifier: evaluate it on held-out microphones,
rooms, reverberation and television playback before deployment. Its published
repository describes training and real-data fine-tuning; this review did not
benchmark it or verify a deployable weights bundle. [S9,S10]

Finally, equal sample peaks are not equal perceived loudness. The new -12 dBFS
reference is a conservative engineering target, not LUFS or P.56 conformance.
Active-speech level and listening comparisons belong in acceptance testing. [S8]
