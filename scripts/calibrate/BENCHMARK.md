# Speech-enhancement comparative benchmark

A reproducible, adversarial comparison of the two BigLinux W8A16 LADSPA denoisers
against relevant external systems, on the same machine and material. The point is
to find where **our own** ports lose — quantization gap, jitter, latency, hard
conditions — not to pick flattering numbers. Every value in `COMPARISON.md` is
produced by the scripts here from raw per-file JSON; none is hand-edited.

Anyone can clone this repo, provision a machine as below, run the scripts, and get
comparable numbers.

## Systems compared

| System | Origin | Backend | Version / commit |
|---|---|---|---|
| DFN3 W8A16 | BigLinux (ours) | Native Rust/SIMD, int8 weights / int16 activations | `deepfilternet-quantized-ladspa @ b47a9ef` |
| DFN3 W8A16 (gate off) | BigLinux (ours) | same `.so`, SNR gate / stage-skip disabled (diagnostic) | `@ b47a9ef` |
| DFN3-LL W8A16 | BigLinux (ours) | Native Rust/SIMD, low-latency variant | `@ b47a9ef` |
| DeepFilterNet upstream | external | Official Rust (tract), f32 | `DeepFilterNet v0.5.6-89-gd375b2d` |
| RNNoise (werman) | external | Native C, f32 | `noise-suppression-for-voice 1.21-1` |
| DPDFNet2 / DPDFNet8 48k HR | external | ONNX Runtime, f32 | `ceva-ip/DPDFNet @ 84828ad` |
| GTCRN | external | ONNX Runtime (16 kHz model) in a LADSPA wrapper | `gtcrn-ladspa 26.04.27-0355` |

`systems.toml` is the source of truth for paths, labels, controls and provenance.
Upstream-reported params/MACs stay in their own labelled columns and are never mixed
with measured numbers.

## Datasets

| Dataset | Condition | Pairs used | Reference |
|---|---|---|---|
| VoiceBank+DEMAND | additive noise, 48 kHz | 824 (full) | [datashare 10283/2791](https://datashare.ed.ac.uk/handle/10283/2791) |
| Noisy-reverberant (Edinburgh) | noise + reverberation, 48 kHz | 200 | [datashare 10283/2826](https://datashare.ed.ac.uk/handle/10283/2826) |
| DNS 2020 synthetic no-reverb | DNS noises, 16 kHz | 150 | [DNS-Challenge `interspeech2020/master`](https://github.com/microsoft/DNS-Challenge/tree/interspeech2020/master) |

WAVs are never committed. `datasets.toml` lists the URLs; `setup.sh` fetches
VoiceBank+DEMAND and the DNSMOS model. The reverb set reuses the VoiceBank clean
references (same VCTK speakers and filenames). The DNS set is git-LFS — sparse-checkout
`datasets/test_set/synthetic/no_reverb` then `git lfs pull --include` that path only
(a bare `git lfs pull` drags the whole multi-GB repo), and pair by the `fileid_N`
token that both `clean_fileid_N.wav` and the noisy name carry.

## Running

Provision (BigLinux / Arch): the LADSPA `.so`s come from packages —
`noise-suppression-for-voice`, `deepfilternet-ladspa`, `gtcrn-ladspa` — plus the two
BigLinux `.so`s in `/usr/lib/ladspa`. Python side: `python -m venv .venv` and
`pip install -r requirements.txt -r requirements-extra.txt` (pesq needs a C compiler).

```bash
# quality, one dataset at a time (the corpus label names the rows)
python compare_systems.py --limit 824 --json reports/full824.json
python compare_systems.py --limit 200 --dataset voicebank_demand_reverb \
    --noisy-dir <cache>/datasets/voicebank_demand_reverb/noisyreverb_testset_wav \
    --clean-dir <cache>/datasets/voicebank_demand/clean_testset_wav \
    --json reports/reverb200.json
# memory / disk footprint, and steady-state CPU
python bench_resources.py --json reports/resources.json
python bench_cpu.py                       # writes reports/cpu.json
# assemble the tables (numbers come only from here)
python gen_report.py reports/full824.json reports/reverb200.json reports/dns150.json \
    --cpu reports/cpu.json --resources reports/resources.json --out COMPARISON.md
```

Long runs survive an SSH drop as `systemd-run --user --unit=... ` services (enable
lingering first); poll the `.log` and JSON.

## Methodology

**Same input, real deployment.** Every system receives the identical clip. LADSPA
systems run through ffmpeg's `ladspa` filter with `aresample` in and out, so a 16 kHz
model (GTCRN) pays its resampler cost. ONNX systems run a streaming overlap-add loop
that resamples the same way. A plugin that refuses to load raises rather than scoring
a passthrough. An unprocessed "raw input" row is the floor every system is measured
against.

**Alignment.** Denoisers add 20–200 ms of latency; comparing sample-wise without
compensating produces a meaningless score, not a slightly worse one. `align_by_delay`
estimates the shift by cross-correlation (plain argmax — the lag that best aligns the
waveforms; an "earliest near-peak" variant was tried and reverted because it shaved
samples off the optimum and collapsed SI-SDR for the good systems) and reports the
delay per system so a wrong estimate is visible. Occasional per-clip misestimates
wash out across a full set.

**Quality metrics (vs clean).** PESQ-WB (at 16 kHz), STOI / eSTOI, SI-SDR and SI-SDRi
(improvement over the raw floor), and DNSMOS P.835 OVRL. Audio is loudness-normalized
to −26 dBFS before DNSMOS. Resampling for a metric never feeds the 48 kHz filter. No
single metric decides "best"; DNSMOS in particular is not comparable across runs
(clip length, loudness, model revision all move it) and must never be set beside a
published leaderboard.

**CPU (steady-state, warm).** `ladspa_bench` (Rust) drives a `.so` the way an audio
thread does — dlopen once, `run()` one quantum over and over, paced — and reports the
per-block audio-thread RTF (p50/p95/p99/max) **and** the total process CPU across all
threads. The total is what stays honest when a plugin offloads to a worker: DeepFilterNet
upstream shows a near-free `run()` (0.0006 audio-thread RTF) while a worker core burns
0.29 total. ONNX systems are timed with a warm session. Note the RTF column in the
quality tables is host-measured — LADSPA rows there pay a per-clip ffmpeg model reload,
so that RTF is pessimistic; the CPU table is the fair comparison.

**Memory / footprint.** Core artifact (the deployable file), runtime dependency (the
ONNX Runtime a native `.so` does not need), and peak RSS measured per system on one
clip via child `rusage`. RSS includes the host baseline (ffmpeg ~126 MB for LADSPA,
python+ORT for ONNX), stated per row — compare artifact size across hosts, RSS only
within a host.

## Outputs

- Raw per-file rows: `reports/*.json` (kept out of git; regenerable).
- Rendered tables: `COMPARISON.md`, generated only by `gen_report.py`.

## Known limitations

- LADSPA RTF in the quality tables includes ffmpeg cold-load; use the CPU table for a
  fair processing comparison.
- Cross-correlation alignment can misestimate a single clip; means over the set absorb
  it, but a tiny-N run can be skewed.
- DNSMOS is run-local, not comparable to published figures.
- Reverb (convolutive) is barely removed by any of these additive-noise denoisers —
  a real limit, not a harness bug.
- DPDFNet shows negative SI-SDRi on the DNS set (16 kHz clips upsampled to 48 kHz; the
  full-band model adds inaudible >8 kHz content that PESQ/DNSMOS ignore but SI-SDR
  penalizes).
- Reverb and DNS run at N=200/150, VoiceBank+DEMAND at the full 824.
- A curated hard-case set (fan, keyboard, babble, …) with preserved outputs for
  listening is not yet included; it is qualitative and complements, not replaces, these
  standardized datasets.

## Worked example: the DFN3 gate retune

The benchmark's headline finding — DFN3 at its shipped default gate degrades speech —
is the kind of defect this suite exists to catch, and `bench_gate_sweep.py` turns it
into a fix. DFN3 has three per-frame thresholds on the estimated SNR: a **noise gate**
(Min SNR, default −10 dB) that bypasses low-SNR frames, and two **stage-skips**
(Skip-all 30 dB, Skip-DF 20 dB) that skip processing on high-SNR frames to save CPU.
Sweeping them on 100 VoiceBank+DEMAND pairs, quality vs warm CPU:

| Config (Min/Skip-all/Skip-DF) | SI-SDRi | PESQ | DNSMOS | CPU RTF |
|---|---:|---:|---:|---:|
| shipped default (−10/30/20) | +1.04 | 2.26 | 3.40 | 0.030 |
| gate off, skip default (−40/30/20) | +1.18 | 2.65 | 3.67 | 0.051 |
| gate off, skip mild (−40/35/30) | +8.93 | 3.16 | 4.02 | 0.049 |
| **proposed (−10/40/40)** | **+11.02** | 2.83 | 3.80 | **0.028** |
| gate off / full (−40/40/40) | +11.19 | 3.26 | 4.05 | 0.051 |

Two independent effects fall out. The **stage-skip is pure loss**: at 30/20 it skips the
deep filter on frames that need it and collapses SI-SDRi from +11 to +1, while costing
the *same* CPU (0.051) as running them — it does not save compute on real noisy speech.
The **noise gate is the actual CPU lever**: −10 dB drops CPU from 0.051 to ~0.028 for a
modest PESQ cost. So the fix is `−10/40/40` — keep the noise gate for the CPU-bound i3,
disable the harmful skip — which recovers almost all quality (+11.02) at even lower CPU
than today's default. A quality-first tier can run `−40/40/40`. The current live chain
already runs with the gate open; this is about the `.so`'s default hints, the footgun a
naive LADSPA user hits.

## Reproducibility

The measurement machine for `COMPARISON.md` is an AMD Ryzen 5 5600H (12 threads,
AVX2+FMA), BigLinux (Manjaro), kernel 7.1.4, ONNX Runtime 1.30, one thread throughout.
Re-running produces the same numbers within per-clip alignment noise; publish a table
only after a second run agrees.
