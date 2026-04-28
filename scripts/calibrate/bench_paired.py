#!/usr/bin/env python3
"""Paired-reference benchmark on VoiceBank+DEMAND.

DNSMOS-only ranking is biased toward training-set distributions and
the bench in `bench_models.py` uses noisy-only audio. This script
runs each model on `noisy_testset_wav/<name>.wav` and scores against
`clean_testset_wav/<name>.wav` with PESQ-WB, STOI, eSTOI, SI-SDR —
the metrics that actually measure how close the enhanced signal is
to the clean reference.

Caveat: `gtcrn_vctk` was trained on the VoiceBank+DEMAND train split,
so the test split is in-distribution for it. Treat its row as an
upper bound and compare the others to each other.

Output: `paired_report.{csv,md}` next to the calibration reports.
"""

from __future__ import annotations

import argparse
import csv
import sys
from dataclasses import dataclass
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

from bench_models import _default_cache  # noqa: E402
from lib import denoisers, metrics, signals  # noqa: E402


@dataclass
class ModelEntry:
    name: str
    onnx_path: Path


def _discover(cache: Path) -> list[ModelEntry]:
    out: list[ModelEntry] = []
    gtcrn_root = Path(
        "/home/bruno/codigo-pacotes/multimidia/gtcrn-ladspa/stream/onnx_models"
    )
    for variant in ("gtcrn_simple.onnx", "gtcrn_vctk.onnx"):
        p = gtcrn_root / variant
        if p.exists():
            out.append(ModelEntry(p.stem, p))
    ulunas_dir = cache / "models/ulunas"
    for variant in ("ulunas_stream_simple.onnx", "ulunas_stream.onnx"):
        p = ulunas_dir / variant
        if p.exists():
            out.append(ModelEntry(p.stem, p))
    dpdfnet_dir = cache / "models/dpdfnet"
    for p in sorted(dpdfnet_dir.glob("*.onnx")):
        out.append(ModelEntry(p.stem, p))
    return out


def _align_lag(reference: np.ndarray, processed: np.ndarray, max_ms: int, sr: int) -> tuple[np.ndarray, np.ndarray, int]:
    """Cross-correlate within ±max_ms to find streaming-model group
    delay, then trim both to a common aligned span. PESQ does its own
    alignment, but STOI and SI-SDR are sample-aligned and collapse to
    nonsense if the model leaks a few frames of latency."""
    max_lag = int(sr * max_ms / 1000)
    n = min(reference.size, processed.size)
    a = reference[:n] - float(np.mean(reference[:n]))
    b = processed[:n] - float(np.mean(processed[:n]))
    # FFT-based cross-correlation of mean-removed signals.
    nfft = 1 << int(np.ceil(np.log2(2 * n)))
    A = np.fft.rfft(a, nfft)
    B = np.fft.rfft(b, nfft)
    xc = np.fft.irfft(B * np.conj(A), nfft)
    # Lags = b-shift relative to a: positive lag means b lags a.
    pos = xc[: max_lag + 1]
    neg = xc[-max_lag:][::-1]
    if np.max(np.abs(pos)) >= np.max(np.abs(neg)):
        lag = int(np.argmax(np.abs(pos)))
    else:
        lag = -int(np.argmax(np.abs(neg))) - 1
    if lag > 0:
        return reference[: n - lag], processed[lag:n], lag
    if lag < 0:
        return reference[-lag : n], processed[: n + lag], lag
    return reference[:n], processed[:n], 0


def _score(reference: np.ndarray, processed: np.ndarray, sr: int) -> dict[str, float]:
    ref_a, proc_a, _ = _align_lag(reference, processed, max_ms=80, sr=sr)
    return {
        "pesq_wb": metrics.pesq_wb(reference, processed, sr),
        "stoi": metrics.stoi(ref_a, proc_a, sr, extended=False),
        "estoi": metrics.stoi(ref_a, proc_a, sr, extended=True),
        "si_sdr_db": metrics.si_sdr_db(ref_a, proc_a),
    }


def _mean(xs: list[float]) -> float:
    xs = [v for v in xs if not np.isnan(v)]
    return float(np.mean(xs)) if xs else float("nan")


def _bench_one(
    entry: ModelEntry | None,
    pairs: list[tuple[Path, Path]],
    backend: str,
) -> dict:
    """Run one model (or noisy passthrough if entry is None) on pairs."""
    runner = (
        denoisers.load(entry.name, entry.onnx_path, backend=backend)
        if entry is not None
        else None
    )
    if runner is not None:
        runner.session()  # force load

    pesqs: list[float] = []
    stois: list[float] = []
    estois: list[float] = []
    sdrs: list[float] = []

    for clean_path, noisy_path in pairs:
        try:
            clean, sr_c = signals.read_wav(clean_path)
            noisy, sr_n = signals.read_wav(noisy_path)
        except Exception as e:  # noqa: BLE001
            print(f"  skip {noisy_path.name}: {e}", file=sys.stderr)
            continue
        if sr_c != sr_n:
            print(f"  skip {noisy_path.name}: sr mismatch", file=sys.stderr)
            continue

        if runner is None:
            y, sr = noisy, sr_n
        else:
            try:
                y, _ = runner.run(noisy, sr_n)
                sr = sr_n
            except Exception as e:  # noqa: BLE001
                print(f"  fail on {noisy_path.name}: {e}", file=sys.stderr)
                continue

        n = min(len(clean), len(y))
        m = _score(clean[:n], y[:n], sr)
        pesqs.append(m["pesq_wb"])
        stois.append(m["stoi"])
        estois.append(m["estoi"])
        sdrs.append(m["si_sdr_db"])
        label = entry.name if entry else "noisy"
        print(
            f"    {label:24s} {noisy_path.name:30s} "
            f"pesq={pesqs[-1]:.2f} stoi={stois[-1]:.3f} sdr={sdrs[-1]:+.1f}"
        )

    return {
        "model": entry.name if entry else "noisy",
        "backend": backend if entry else "-",
        "pesq_wb": _mean(pesqs),
        "stoi": _mean(stois),
        "estoi": _mean(estois),
        "si_sdr_db": _mean(sdrs),
        "n_samples": len(pesqs),
    }


def _write_md(rows: list[dict], dest: Path) -> None:
    rows = sorted(rows, key=lambda r: -r["pesq_wb"] if not np.isnan(r["pesq_wb"]) else 0)
    lines = [
        "# Paired-reference denoiser benchmark (VoiceBank+DEMAND)",
        "",
        f"Rows: {len(rows)}",
        "",
        "Reference-based metrics: PESQ-WB / STOI / eSTOI / SI-SDR. Higher = better.",
        "",
        "Caveat: `gtcrn_vctk` trained on this dataset's train split — its row is in-distribution and biased.",
        "",
        "| model | backend | PESQ-WB | STOI | eSTOI | SI-SDR | n |",
        "|---|---|---:|---:|---:|---:|---:|",
    ]
    for r in rows:
        lines.append(
            f"| `{r['model']}` "
            f"| {r['backend']} "
            f"| {r['pesq_wb']:.2f} "
            f"| {r['stoi']:.3f} "
            f"| {r['estoi']:.3f} "
            f"| {r['si_sdr_db']:+.2f} "
            f"| {r['n_samples']} |"
        )
    dest.write_text("\n".join(lines) + "\n")


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--dataset",
        type=Path,
        default=_default_cache() / "datasets/voicebank_demand",
    )
    p.add_argument("--out", type=Path, default=_default_cache() / "reports")
    p.add_argument("--limit", type=int, default=80, help="0 = all")
    p.add_argument("--backend", default="onnxruntime", choices=("onnxruntime", "openvino"))
    p.add_argument("--include-noisy", action="store_true", default=True)
    args = p.parse_args()

    clean_dir = args.dataset / "clean_testset_wav"
    noisy_dir = args.dataset / "noisy_testset_wav"
    if not clean_dir.exists() or not noisy_dir.exists():
        print(f"missing dataset under {args.dataset}", file=sys.stderr)
        return 1

    pairs: list[tuple[Path, Path]] = []
    for noisy in sorted(noisy_dir.glob("*.wav")):
        clean = clean_dir / noisy.name
        if clean.exists():
            pairs.append((clean, noisy))
    if args.limit > 0:
        pairs = pairs[: args.limit]
    if not pairs:
        print("no pairs found", file=sys.stderr)
        return 1

    cache = _default_cache()
    entries = _discover(cache)
    if not entries:
        print("no models discovered", file=sys.stderr)
        return 1

    rows: list[dict] = []
    if args.include_noisy:
        print("\n=== noisy (passthrough)")
        rows.append(_bench_one(None, pairs, args.backend))

    for entry in entries:
        print(f"\n=== {entry.name}  [{args.backend}]")
        try:
            rows.append(_bench_one(entry, pairs, args.backend))
        except Exception as e:  # noqa: BLE001
            print(f"  model failed: {e}", file=sys.stderr)

    args.out.mkdir(parents=True, exist_ok=True)
    csv_path = args.out / "paired_report.csv"
    with csv_path.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=sorted({k for r in rows for k in r}))
        w.writeheader()
        w.writerows(rows)
    print(f"\nwrote {csv_path}")

    md_path = args.out / "paired_report.md"
    _write_md(rows, md_path)
    print(f"wrote {md_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
