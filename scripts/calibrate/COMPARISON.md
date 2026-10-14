## How does it compare?

> These systems have different architectures and, in some cases, different native sample rates and deployment goals. Results below use the same hardware and test material. Published upstream specifications are kept separate from measurements performed here.

Machine: CPU x86_64, kernel 7.1.4-1-MANJARO. Loudness normalized to -26.0 dBFS. Resampler: ffmpeg aresample (soxr) for ladspa; scipy resample_poly for onnx.

### Architecture (published / verifiable)

| Implementation | Origin | Rate | Backend | Quantization | External runtime | Params | MACs |
|---|---|---|---|---|---|---|---|
| DFN3 W8A16 | BigLinux (ours) | 48k | Native Rust/SIMD | W8A16 | None | — | — |
| DFN3 W8A16 (gate off) | BigLinux (ours) | 48k | Native Rust/SIMD | W8A16 | None | — | — |
| DFN3-LL W8A16 | BigLinux (ours) | 48k | Native Rust/SIMD | W8A16 | None | — | — |
| DeepFilterNet upstream | external | 48k | Official Rust (tract) | none (f32) | None | — | — |
| RNNoise (werman) | external | 48k | Native C | none (f32) | None | — | — |
| DPDFNet2 48k HR | external | 48k | ONNX Runtime | none (f32) | onnxruntime 1.30.0 | 2.58 M (upstream reported) | 2.42 G (upstream reported) |
| DPDFNet8 48k HR | external | 48k | ONNX Runtime | none (f32) | onnxruntime 1.30.0 | 3.63 M (upstream reported) | 7.17 G (upstream reported) |
| GTCRN | external | 48k | ONNX Runtime (16 kHz model, in LADSPA) | none (f32) | onnxruntime (in-plugin) | — | — |

### Quality — VoiceBank+DEMAND test split (824 pairs)

PESQ-WB / eSTOI / SI-SDRi vs the clean reference (higher = better); delay is the aligned latency; RTF = processing / audio (lower = better). RTF is host-measured: LADSPA rows run through ffmpeg and pay a per-clip model reload, ONNX rows reuse a warm session — so LADSPA RTF is if anything pessimistic. A host-consistent steady-state per-frame measure is the dedicated bin (pending).

| Implementation | Origin | PESQ-WB | eSTOI | SI-SDRi (dB) | DNSMOS OVRL | delay (ms) | RTF |
|---|---|---:|---:|---:|---:|---:|---:|
| raw input | — | 1.97 | 0.787 | +0.00 | 3.27 | 0 | 0.000 |
| DFN3 W8A16 | BigLinux (ours) | 2.08 | 0.799 | +0.65 | 3.13 | 40 | 0.064 |
| DFN3 W8A16 (gate off) | BigLinux (ours) | 3.17 | 0.865 | +10.59 | 3.86 | 40 | 0.067 |
| DFN3-LL W8A16 | BigLinux (ours) | 2.50 | 0.846 | +8.69 | 3.53 | 20 | 0.071 |
| DeepFilterNet upstream | external | 3.08 | 0.871 | +10.35 | 3.87 | 20 | 0.439 |
| RNNoise (werman) | external | 2.11 | 0.778 | +3.98 | 3.43 | 191 | 0.044 |
| DPDFNet2 48k HR | external | 1.90 | 0.830 | +7.17 | 3.59 | 40 | 0.164 |
| DPDFNet8 48k HR | external | 2.02 | 0.841 | +7.21 | 3.61 | 40 | 0.430 |
| GTCRN | external | 1.88 | 0.743 | -4.38 | 3.49 | 43 | 0.095 |

### Quality — voicebank_demand_reverb (200 pairs)

PESQ-WB / eSTOI / SI-SDRi vs the clean reference (higher = better); delay is the aligned latency; RTF = processing / audio (lower = better). RTF is host-measured: LADSPA rows run through ffmpeg and pay a per-clip model reload, ONNX rows reuse a warm session — so LADSPA RTF is if anything pessimistic. A host-consistent steady-state per-frame measure is the dedicated bin (pending).

| Implementation | Origin | PESQ-WB | eSTOI | SI-SDRi (dB) | DNSMOS OVRL | delay (ms) | RTF |
|---|---|---:|---:|---:|---:|---:|---:|
| raw input | — | 1.25 | 0.519 | +0.00 | 2.06 | 13 | 0.000 |
| DFN3 W8A16 | BigLinux (ours) | 1.49 | 0.641 | +1.42 | 2.99 | 52 | 0.065 |
| DFN3 W8A16 (gate off) | BigLinux (ours) | 1.64 | 0.653 | +1.51 | 3.53 | 52 | 0.067 |
| DFN3-LL W8A16 | BigLinux (ours) | 1.50 | 0.652 | +1.57 | 3.03 | 33 | 0.090 |
| DeepFilterNet upstream | external | 1.69 | 0.662 | +1.38 | 3.54 | 33 | 0.506 |
| RNNoise (werman) | external | 1.34 | 0.527 | -0.08 | 2.68 | 204 | 0.058 |
| DPDFNet2 48k HR | external | 1.38 | 0.642 | +0.77 | 3.20 | 53 | 0.196 |
| DPDFNet8 48k HR | external | 1.42 | 0.661 | +0.89 | 3.22 | 53 | 0.497 |
| GTCRN | external | 1.34 | 0.539 | -0.56 | 2.91 | 57 | 0.112 |

### Quality — dns2020_synthetic_noreverb (150 pairs)

PESQ-WB / eSTOI / SI-SDRi vs the clean reference (higher = better); delay is the aligned latency; RTF = processing / audio (lower = better). RTF is host-measured: LADSPA rows run through ffmpeg and pay a per-clip model reload, ONNX rows reuse a warm session — so LADSPA RTF is if anything pessimistic. A host-consistent steady-state per-frame measure is the dedicated bin (pending).

| Implementation | Origin | PESQ-WB | eSTOI | SI-SDRi (dB) | DNSMOS OVRL | delay (ms) | RTF |
|---|---|---:|---:|---:|---:|---:|---:|
| raw input | — | 1.58 | 0.810 | +0.00 | 2.91 | 0 | 0.000 |
| DFN3 W8A16 | BigLinux (ours) | 1.81 | 0.827 | -3.16 | 3.01 | 40 | 0.032 |
| DFN3 W8A16 (gate off) | BigLinux (ours) | 2.57 | 0.901 | +4.27 | 3.96 | 40 | 0.036 |
| DFN3-LL W8A16 | BigLinux (ours) | 2.05 | 0.894 | +3.92 | 3.55 | 20 | 0.051 |
| DeepFilterNet upstream | external | 2.78 | 0.919 | +7.92 | 4.00 | 20 | 0.330 |
| RNNoise (werman) | external | 2.13 | 0.883 | +3.59 | 3.77 | 147 | 0.026 |
| DPDFNet2 48k HR | external | 2.47 | 0.907 | -5.12 | 3.87 | 40 | 0.161 |
| DPDFNet8 48k HR | external | 2.59 | 0.918 | -5.15 | 3.90 | 40 | 0.425 |
| GTCRN | external | 2.05 | 0.853 | -3.46 | 3.60 | 47 | 0.056 |

### Steady-state CPU cost (measured warm: ladspa_bench / warm ONNX session)

Total CPU RTF is process CPU across all threads per second of audio — the honest compute cost. Audio-thread RTF is the per-block cost on the real-time thread; the gap between them is work a plugin pushed to a worker core.

| Implementation | Origin | audio-thread RTF p50 | p99 | total CPU RTF |
|---|---|---:|---:|---:|
| DFN3 W8A16 | BigLinux (ours) | 0.0236 | 0.0384 | 0.0269 |
| DFN3 W8A16 (gate off) | BigLinux (ours) | 0.0463 | 0.0635 | 0.0489 |
| DFN3-LL W8A16 | BigLinux (ours) | 0.0323 | 0.0483 | 0.0352 |
| DeepFilterNet upstream | external | 0.0005 | 0.0009 | 0.2870 |
| RNNoise (werman) | external | 0.0314 | 0.0438 | 0.0340 |
| DPDFNet2 48k HR | external | — | — | 0.1571 |
| DPDFNet8 48k HR | external | — | — | 0.4235 |
| GTCRN | external | 0.0980 | 0.1451 | 0.0718 |

### Deploy footprint (measured, one clip: p232_001.wav)

Core artifact = the deployable file; runtime dep = ONNX Runtime a native plugin does not need. Peak RSS is the whole host's peak — LADSPA rows share ffmpeg's ~126 MB baseline, so the plugin's own memory is far smaller than the number shown; the ONNX rows carry python + ORT. Compare artifact size across hosts, RSS only within a host. Processing cost is the RTF column above.

| Implementation | Origin | Core artifact | Runtime dep | Peak RSS | RSS host |
|---|---|---:|---:|---:|---|
| DFN3 W8A16 | BigLinux (ours) | 4.37 MB | none | 126 MB | ffmpeg+plugin |
| DFN3 W8A16 (gate off) | BigLinux (ours) | 4.37 MB | none | 126 MB | ffmpeg+plugin |
| DFN3-LL W8A16 | BigLinux (ours) | 12.42 MB | none | 126 MB | ffmpeg+plugin |
| DeepFilterNet upstream | external | 54.99 MB | none | 216 MB | ffmpeg+plugin |
| RNNoise (werman) | external | 3.64 MB | none | 126 MB | ffmpeg+plugin |
| DPDFNet2 48k HR | external | 10.49 MB | 30.6 MB | 160 MB | python+onnxruntime |
| DPDFNet8 48k HR | external | 14.86 MB | 30.6 MB | 170 MB | python+onnxruntime |

