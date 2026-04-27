# Calibration system

Reproducible offline scoring of the BigLinux mic filter chain.
Used to validate HPF cutoffs, EQ presets, gate thresholds and
GTCRN parameters against objective speech-quality metrics.

## What lives where

```
scripts/calibrate/
├── lib/
│   ├── chain.py         # Offline chain emulator (HPF + GTCRN + EQ + comp)
│   ├── metrics.py       # PESQ, STOI, DNSMOS, LUFS, SI-SDR, band energies
│   ├── signals.py       # Sweep / sine / noise / SNR mixer
│   └── dnsmos.py        # DNSMOS P.835 ONNX wrapper
├── gen_signals.py       # CLI: produce canonical test signals
├── score_pair.py        # CLI: score one (clean, processed) pair
├── run_sweep.py         # CLI: matrix sweep → report.csv + report.md
├── setup.sh             # One-shot bootstrap (venv + models + dataset)
├── datasets.toml        # External-resource manifest
└── requirements.txt     # Python deps
```

Generated artefacts (cache, datasets, reports) live **outside the
repo** at `${XDG_CACHE_HOME:-~/.cache}/biglinux-noise-reduction-pipewire/calibration/`.

## Bootstrap

```bash
./scripts/calibrate/setup.sh                # full: venv + DNSMOS + DNS-5
./scripts/calibrate/setup.sh --skip-dataset # quick: just venv + DNSMOS
source scripts/calibrate/.venv/bin/activate
```

What `setup.sh` fetches:

| Resource | Size | Path |
|---|---|---|
| DNSMOS P.835 ONNX | ~50 MB | `<cache>/models/dnsmos/sig_bak_ovr.onnx` |
| DNSMOS P.808 ONNX | ~50 MB | `<cache>/models/dnsmos/model_v8.onnx` |
| DNS-5 blind set   | ~5 GB  | `<cache>/datasets/dns/wav/` |

GTCRN ONNX is not fetched: the sibling project at
`/home/bruno/codigo-pacotes/multimidia/gtcrn-ladspa/ladspa/models/`
already ships `gtcrn_dns3_simple.onnx` and `gtcrn_vctk_simple.onnx`.
Pass one with `--gtcrn-model` to enable the denoiser stage.

## Common workflows

### Score a single processed file

```bash
python scripts/calibrate/score_pair.py --processed out.wav
python scripts/calibrate/score_pair.py --reference clean.wav --processed out.wav
```

Adds PESQ + STOI + SI-SDR when `--reference` is provided.

### Probe HPF / EQ magnitude response with a sweep

```bash
python scripts/calibrate/gen_signals.py
python scripts/calibrate/score_pair.py --processed <cache>/calibration/signals/log_sweep_20_20k.wav
```

### Full preset / cutoff sweep

```bash
python scripts/calibrate/run_sweep.py \
    --gtcrn-model /home/bruno/codigo-pacotes/multimidia/gtcrn-ladspa/ladspa/models/gtcrn_dns3_simple.onnx
```

Output: `<cache>/calibration/reports/report.{csv,md}`. Report ranks
configurations by DNSMOS OVRL across every sample found.

## What it covers / what it doesn't

| | Covered | Notes |
|---|---|---|
| HPF biquad cascade | Yes (exact) | Same RBJ math as PipeWire `bq_highpass` |
| EQ peaking biquads | Yes (exact) | Matches `bq_peaking` at q=1.41 |
| GTCRN denoiser     | Yes (exact) | Calls the live ONNX model |
| Compressor (SC4)   | Approximated | Functional model, not sample-perfect |
| Gate (post-GTCRN)  | Not yet     | Skipped — the prod gate runs inside the LADSPA |
| Pitch shifter      | Not yet     | Voice-changer path; not on the calibration hot path |

The compressor and gate gaps don't block calibration of the new
HPF cascade or any EQ preset — the chain order means earlier-stage
changes are scored exactly. Add later if voice-changer or comp tuning
becomes a target.

## Maintaining as the chain evolves

When you add or rename a node in `src/pipeline/mic.rs`:

1. Mirror the math in `lib/chain.py` (or document why it's an approximation).
2. If the new stage exposes a parameter you'll calibrate, add it to
   `chain.ChainSettings` and a sweep axis in `run_sweep.py:_build_matrix`.
3. Run `./scripts/quality-check.sh` (Rust gate) and a smoke `run_sweep.py
   --limit-samples 1` to verify the emulator still loads.
