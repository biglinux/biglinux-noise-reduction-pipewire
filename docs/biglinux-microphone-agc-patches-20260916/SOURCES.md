# Primary sources and verification boundaries

Research accessed during this review on 2026-09-16. Live documentation and default
branches can change; the behavior summarized here is what was inspected, not a
claim that the deployment already uses those versions. No third-party model weights
or implementation source files are bundled.

## [S1] WebRTC AGC2 reference implementation

https://webrtc.googlesource.com/src/+/refs/heads/main/modules/audio_processing/agc2/adaptive_digital_gain_controller.cc

Inspected blob: `adfc4fc3ccfc5970acd53a8fd603f2edd62e2329`.
Relevant functions separate gain-increase permission, noise limiting, headroom,
and low-confidence peak-driven decreases. This supports an asymmetric controller;
it does not validate the present level-only detector or supply speaker identity.
The patch is an independently written adaptation of the design principle, not a
port of WebRTC's VAD or DSP.

## [S2] WirePlumber wpctl implementation

https://raw.githubusercontent.com/PipeWire/wireplumber/master/src/tools/wpctl.c

The mixer API is configured for cubic scale. `get-volume` reports a fraction with
two decimal places and a mute marker. The volume setter has an optional explicit
limit; there is no universal rule that the command cannot exceed 100%.

## [S3] WirePlumber mixer API implementation

https://raw.githubusercontent.com/PipeWire/wireplumber/master/modules/module-mixer-api.c

The cubic scale maps the external fraction `v` to linear amplitude `v^3`, and uses
its inverse for reporting. Consequently `20*log10(v^3) = 60*log10(v)`. This is the
mapping used by the patch; device quantization and the actual audio route still
need measurement.

## [S4] PipeWire pw-cat manual

https://docs.pipewire.org/page_man_pw-cat_1.html

Documentation displayed PipeWire 1.6.8 when inspected. An explicit target can be a
node name or object serial. The documented default latency is 100 ms. Raw capture
format and target syntax were checked. No claim is made that command completion
synchronizes a volume step with already buffered analyzer frames.

## [S5] PipeWire properties manual

https://docs.pipewire.org/page_man_pipewire-props_7.html

Checked `target.object`, `node.dont-fallback`, `node.dont-reconnect`,
`node.dont-move`, and `api.alsa.soft-mixer`. The explicit-target meter uses the
no-fallback/no-reconnect/no-move controls. Soft-mixer configuration illustrates why
hardware ADC control cannot be inferred solely from a visible volume percentage.

## [S6] Target speech extraction and speaker-selection information

K. Žmolíková et al., *Neural Target Speech Extraction: An Overview*, 2023.

https://arxiv.org/abs/2301.13341

The abstract discusses why desired and undesired speech share characteristics and
how target extraction uses information such as enrollment, spatial cues or visual
cues. Only the abstract/metadata were inspected here, not a PDF. The indistinguishable
level-trace counterexample in this bundle is our direct observability argument,
not a claim that the paper proves this particular controller's behavior.

## [S7] PipeWire filter-chain documentation

https://docs.pipewire.org/page_module_filter_chain.html

Checked host graph/control-port context. A LADSPA control output is not, merely by
existing, an authenticated off-process AGC telemetry API. Any later side channel
needs an explicit producer/consumer contract, timestamps, freshness handling and
source identity. No such interface or LADSPA ABI change is introduced in this patch.

## [S8] ITU-T P.56 active speech level index

https://www.itu.int/rec/T-REC-P.56/en

The official index lists **P.56 (07/26)** as in force and prepublished, with the
2011 edition superseded; the index shows an August 2026 update. Only this index
was inspected, not the normative 2026 recommendation. The patch does not implement
or claim P.56 conformance. Its peak-reference statistic is not a standardized
active-speech-level or perceived-loudness measurement.

## [S9] Current single-microphone distance-estimation research

J. Wang et al., *Fast-SDE: Efficient Single-Microphone Sound Source Distance
Estimation in Reverberant Environments*, submitted 2026-06-10.

https://arxiv.org/abs/2606.12339

Abstract and metadata inspected. The work proposes learned distance estimation
using single-microphone input. This is a research candidate for held-out microphone
and room evaluation, not a benchmarked dependency or a validated replacement for
speaker selection in this product. No latency, accuracy or CPU claims from this
paper were reproduced here.

## [S10] Fast-SDE authors' repository

https://github.com/JiangWAV/FAST-SDE

README inspected: training, real-data fine-tuning and test workflows are described,
and the repository states an MIT license. Available deployable checkpoint weights
and their licensing were not independently verified. Nothing from this repository
is required to apply or run the proposed controller tests.

## Supplied source evidence (not external publication claims)

- `code/src/agc.rs`: baseline detector, post-extraction normalization and startup
  gain behavior. The patch moves normalization before all adaptive histories.
- `code/src/services/audio_monitor/analyzer.rs` and `types.rs`: sample-peak FFT
  analysis, floor at -120 dBFS, 2048-sample window and 512-sample hop at the default
  48 kHz. This is approximately 42.7 ms/window and 10.7 ms/hop, not a true-peak or
  active-speech loudness meter.
- `filters/dfn3-ladspa/src/lib.rs`: local SNR is formed as
  `sigmoid(val) * (LSNR_MAX - LSNR_MIN) + LSNR_MIN`, with bounds -15 and +35 dB.
  The DFN3LL implementation uses the corresponding mapping. Do not substitute
  this dB estimate directly for a calibrated probability or an owner detector.
- `filters/ops/src/lib.rs`: the silence expander uses the denoiser's local-SNR
  signal. Gated silence is not a measurement of the physical microphone noise.
- `dpdfnet/src/ladspa.rs`: the supplied plugin's five ports cover audio I/O,
  attenuation limit, latency and processing fault; no shared calibrated VAD
  probability interface is present.
- `code/src/pipeline/mic.rs`: calls the gain-safety builder. Its implementation
  was not supplied, so the actual end-to-end limiter cannot be certified here.

No claims of complete repository review, neural-model evaluation, full-paper
replication, human listening validation or universal microphone discrimination
follow from these sources.
