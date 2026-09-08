# Calibration system

Reproducible offline scoring of the BigLinux mic filter chain.
Used to validate HPF cutoffs, EQ presets, and GTCRN parameters
against objective speech-quality metrics.

## What lives where

```
scripts/calibrate/
├── lib/
│   ├── chain.py         # Offline chain emulator (HPF + GTCRN + EQ)
│   ├── metrics.py       # PESQ, STOI, DNSMOS, LUFS, SI-SDR, band energies
│   ├── signals.py       # WAV input loading
│   └── dnsmos.py        # DNSMOS P.835 ONNX wrapper
├── score_pair.py        # CLI: score one (clean, processed) pair
├── run_sweep.py         # CLI: matrix sweep → report.csv + report.md
├── setup.sh             # One-shot bootstrap (venv + models + dataset)
└── requirements.txt     # Python deps
```

Generated artefacts (cache, datasets, reports) live **outside the
repo** at `${XDG_CACHE_HOME:-~/.cache}/biglinux-microphone/calibration/`.

## Bootstrap

```bash
./scripts/calibrate/setup.sh                # full: venv + DNSMOS + DNS-5
./scripts/calibrate/setup.sh --skip-dataset # quick: just venv + DNSMOS
source scripts/calibrate/.venv/bin/activate
```

What `setup.sh` fetches:

| Resource | Size | Path |
|---|---|---|
| DNSMOS P.835 ONNX     | ~1 MB    | `<cache>/models/dnsmos/sig_bak_ovr.onnx` |
| VoiceBank+DEMAND test | ~250 MB  | `<cache>/datasets/voicebank_demand/{clean,noisy}_testset_wav/` |

GTCRN ONNX is not fetched: the sibling `gtcrn-ladspa` checkout already
ships `gtcrn_dns3_simple.onnx` and `gtcrn_vctk_simple.onnx` under its
`ladspa/models/` directory. Pass one with `--gtcrn-model` to enable the
denoiser stage.

## Common workflows

### Score a single processed file

```bash
python scripts/calibrate/score_pair.py --processed out.wav
python scripts/calibrate/score_pair.py --reference clean.wav --processed out.wav
```

Adds PESQ + STOI + SI-SDR when `--reference` is provided.

### Full preset / cutoff sweep

```bash
python scripts/calibrate/run_sweep.py \
    --gtcrn-model path/to/gtcrn-ladspa/ladspa/models/gtcrn_dns3_simple.onnx
```

Output: `<cache>/calibration/reports/report.{csv,md}`. Report ranks
configurations by DNSMOS OVRL across every sample found.

## What it covers / what it doesn't

| | Covered | Notes |
|---|---|---|
| HPF biquad cascade | Yes (exact) | Same RBJ math as PipeWire `bq_highpass` |
| EQ peaking biquads | Yes (exact) | Matches `bq_peaking` at q=1.41 |
| GTCRN denoiser     | Yes (exact) | Calls the live ONNX model |
| Compressor / gate  | No | Use the packaged LADSPA path for those stages |
| Pitch shifter      | No | Use the packaged LADSPA path for that stage |

The offline emulator intentionally stops at the stages it can reproduce
exactly. Product-level measurements for the other stages use the packaged
PipeWire/LADSPA chain instead of maintaining approximate DSP here.

## Maintaining as the chain evolves

When HPF, GTCRN, or EQ changes in `src/pipeline/mic.rs`:

1. Keep only exact HPF/EQ math in `lib/chain.py`; use `lib/denoisers.py`
   directly for model inference.
2. Add a current calibration axis to `run_sweep.py:_build_matrix` only when
   the production setting exists.
3. Run the Rust gate and a smoke `run_sweep.py
   --limit-samples 1` to verify the emulator still loads.
