#!/usr/bin/env python3
"""Benchmark multiple ONNX denoisers on the same sample set.

Measures three axes per model:
- Quality: DNSMOS OVRL/SIG/BAK averaged across `--limit-samples`.
- CPU cost: real-time factor (model wall-clock / audio duration).
  RTF < 1.0 means the model can run live on this single core.
- Memory: model file size on disk plus resident-set growth from
  loading + running one inference. Memory is per-instance — the prod
  pipeline holds one copy per channel, so plan budgets accordingly.

Output is `models_report.{csv,md}` next to the calibration reports.
"""

from __future__ import annotations

import argparse
import csv
import os
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import denoisers, metrics, signals  # noqa: E402


@dataclass
class ModelEntry:
    name: str
    onnx_path: Path


def _default_cache() -> Path:
    return Path(
        os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache"))
    ) / "biglinux-noise-reduction-pipewire/calibration"


def _discover_models(cache: Path) -> list[ModelEntry]:
    """Pick up every ONNX known to the calibration harness. The user
    may override the list through `--model name=path` flags."""
    out: list[ModelEntry] = []

    gtcrn_root = Path("/home/bruno/codigo-pacotes/multimidia/gtcrn-ladspa/stream/onnx_models")
    for variant in ("gtcrn_simple.onnx", "gtcrn.onnx", "gtcrn_vctk.onnx"):
        p = gtcrn_root / variant
        if p.exists():
            out.append(ModelEntry(p.stem, p))

    ulunas_dir = cache / "models/ulunas"
    for p in sorted(ulunas_dir.glob("*.onnx")):
        out.append(ModelEntry(p.stem, p))

    dpdfnet_dir = cache / "models/dpdfnet"
    for p in sorted(dpdfnet_dir.glob("*.onnx")):
        out.append(ModelEntry(p.stem, p))
    return out


def _measure_memory_mb() -> float:
    try:
        import psutil

        return psutil.Process().memory_info().rss / (1024 * 1024)
    except Exception:
        return float("nan")


def _bench_one(
    entry: ModelEntry,
    samples: list[Path],
    dnsmos_path: Path,
    backend: str,
) -> dict:
    """Run one model across all samples and aggregate the three axes."""
    rss_before = _measure_memory_mb()
    runner = denoisers.load(entry.name, entry.onnx_path, backend=backend)
    runner.session()  # force load
    rss_loaded = _measure_memory_mb()

    rtfs: list[float] = []
    ovrls: list[float] = []
    sigs: list[float] = []
    baks: list[float] = []

    for sample in samples:
        try:
            x, sr = signals.read_wav(sample)
        except Exception as e:  # noqa: BLE001
            print(f"  skip {sample.name}: {e}", file=sys.stderr)
            continue
        try:
            y, dt = runner.run(x, sr)
            duration_s = len(x) / sr
            rtf = dt / max(duration_s, 1e-6)
            m = metrics.score_pair(None, y, sr, dnsmos_model=dnsmos_path)
        except Exception as e:  # noqa: BLE001
            print(f"  fail {entry.name} on {sample.name}: {e}", file=sys.stderr)
            continue
        rtfs.append(rtf)
        ovrls.append(m.get("dnsmos_ovrl", float("nan")))
        sigs.append(m.get("dnsmos_sig", float("nan")))
        baks.append(m.get("dnsmos_bak", float("nan")))
        print(f"    {sample.name:40s}  ovrl={ovrls[-1]:.2f}  rtf={rtf:.3f}")

    rss_after = _measure_memory_mb()
    file_mb = entry.onnx_path.stat().st_size / (1024 * 1024)

    def _mean(xs: list[float]) -> float:
        xs = [x for x in xs if not np.isnan(x)]
        return float(np.mean(xs)) if xs else float("nan")

    return {
        "model": entry.name,
        "backend": backend,
        "ovrl": _mean(ovrls),
        "sig": _mean(sigs),
        "bak": _mean(baks),
        "rtf_mean": _mean(rtfs),
        "rtf_p95": float(np.percentile(rtfs, 95)) if rtfs else float("nan"),
        "file_mb": file_mb,
        "rss_load_mb": rss_loaded - rss_before,
        "rss_run_mb": rss_after - rss_before,
        "n_samples": len(rtfs),
    }


def _write_markdown(rows: list[dict], dest: Path) -> None:
    rows = sorted(rows, key=lambda r: (r.get("model", ""), r.get("backend", "")))
    lines = [
        "# Denoiser model benchmark",
        "",
        f"Rows: {len(rows)}",
        "",
        "Ranked by model name + backend so backend deltas line up. RTF < 1.0 is real-time.",
        "",
        "| model | backend | OVRL | SIG | BAK | RTF mean | RTF p95 | file MB | RSS load MB | n |",
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for r in rows:
        lines.append(
            f"| `{r['model']}` "
            f"| {r.get('backend', 'onnxruntime')} "
            f"| {r['ovrl']:.2f} "
            f"| {r['sig']:.2f} "
            f"| {r['bak']:.2f} "
            f"| {r['rtf_mean']:.3f} "
            f"| {r['rtf_p95']:.3f} "
            f"| {r['file_mb']:.1f} "
            f"| {r['rss_load_mb']:.1f} "
            f"| {r['n_samples']} |"
        )
    dest.write_text("\n".join(lines) + "\n")


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--samples-dir",
        type=Path,
        action="append",
        default=[],
        help="Add a directory of input audio. May be repeated.",
    )
    p.add_argument(
        "--dnsmos",
        type=Path,
        default=_default_cache() / "models/dnsmos/sig_bak_ovr.onnx",
    )
    p.add_argument("--out", type=Path, default=_default_cache() / "reports")
    p.add_argument("--limit-samples", type=int, default=20, help="0 = all")
    p.add_argument(
        "--model",
        action="append",
        default=[],
        help="Override discovery: name=path. May repeat.",
    )
    p.add_argument(
        "--backend",
        choices=("onnxruntime", "openvino", "both"),
        default="onnxruntime",
        help="Inference backend. `both` runs every model twice for A/B.",
    )
    args = p.parse_args()

    cache = _default_cache()
    if not args.samples_dir:
        args.samples_dir = [cache / "datasets/voicebank_demand/noisy_testset_wav"]

    samples: list[Path] = []
    for root in args.samples_dir:
        if root.exists():
            samples.extend(sorted(root.glob("*.wav")))
    if args.limit_samples > 0:
        samples = samples[: args.limit_samples]
    if not samples:
        print("no samples found", file=sys.stderr)
        return 1

    if args.model:
        entries = []
        for spec in args.model:
            if "=" not in spec:
                print(f"ignore --model {spec!r}: expected name=path", file=sys.stderr)
                continue
            name, path = spec.split("=", 1)
            entries.append(ModelEntry(name, Path(path)))
    else:
        entries = _discover_models(cache)

    if not entries:
        print("no models discovered", file=sys.stderr)
        return 1

    backends = (
        ["onnxruntime", "openvino"] if args.backend == "both" else [args.backend]
    )
    print(
        f"benchmarking {len(entries)} models on {len(samples)} samples "
        f"× {len(backends)} backend(s)"
    )
    rows: list[dict] = []
    for entry in entries:
        for backend in backends:
            print(f"\n=== {entry.name}  [{backend}]  ({entry.onnx_path})")
            try:
                rows.append(_bench_one(entry, samples, args.dnsmos, backend))
            except Exception as e:  # noqa: BLE001
                print(f"  model failed entirely: {e}", file=sys.stderr)

    if not rows:
        return 1

    args.out.mkdir(parents=True, exist_ok=True)
    csv_path = args.out / "models_report.csv"
    with csv_path.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=sorted({k for r in rows for k in r}))
        w.writeheader()
        w.writerows(rows)
    print(f"\nwrote {csv_path}")

    md_path = args.out / "models_report.md"
    _write_markdown(rows, md_path)
    print(f"wrote {md_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
