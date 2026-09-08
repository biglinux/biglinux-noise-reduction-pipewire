#!/usr/bin/env python3
"""Score every denoiser we ship, on the same audio, in the same run.

Answers one question: which of the models a user can select actually
sounds best, and what each costs. It drives the shipped LADSPA plugins
rather than their ONNX files, so the number describes the product.

Two kinds of score, because neither alone is trustworthy:

* On VoiceBank+DEMAND there is a clean reference, so PESQ, eSTOI and
  SI-SDR say how close the output is to the truth. They are only
  meaningful once the chain's latency is compensated, which
  `metrics.align_by_delay` does and reports.
* DNSMOS P.835 needs no reference and is what everyone else publishes,
  so it is the axis a reader can compare against other work. It also
  ranks systems poorly, which is why it is never the only column here.

Every number is produced in one run, on one machine, from one corpus.
DNSMOS in particular is not comparable across runs — clip length,
loudness, resampler and model revision all move it — so a figure from
here must never be set beside a published leaderboard.

    python compare_shipped.py --limit 40
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
import soundfile as sf

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import cache_root, dnsmos, metrics, shipped  # noqa: E402

CACHE = cache_root()
DATASET = CACHE / "datasets/voicebank_demand"
DNSMOS_MODEL = CACHE / "models/dnsmos/sig_bak_ovr.onnx"


def sha256(path: Path) -> str:
    """First twelve hex digits, enough to pin a file in a report."""
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return digest[:12]


def pairs(limit: int) -> list[tuple[Path, Path]]:
    """Noisy/clean pairs from the VoiceBank+DEMAND test split."""
    noisy_dir = DATASET / "noisy_testset_wav"
    clean_dir = DATASET / "clean_testset_wav"
    if not noisy_dir.is_dir():
        raise SystemExit(f"missing {noisy_dir}; run ./setup.sh first")
    found = sorted(noisy_dir.glob("*.wav"))[:limit]
    return [(n, clean_dir / n.name) for n in found if (clean_dir / n.name).exists()]


def score_one(noisy: np.ndarray, clean: np.ndarray, sr: int) -> dict[str, float]:
    """Every metric for one processed take."""
    out = metrics.score_pair(clean, noisy, sr, dnsmos_model=DNSMOS_MODEL)
    return {
        k: out[k]
        for k in (
            "pesq_wb",
            "stoi",
            "estoi",
            "si_sdr_db",
            "delay_ms",
            "dnsmos_sig",
            "dnsmos_bak",
            "dnsmos_ovrl",
        )
        if k in out
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--limit", type=int, default=40, help="takes to score")
    parser.add_argument("--json", type=Path, help="write the full table here")
    args = parser.parse_args()

    takes = pairs(args.limit)
    if not takes:
        raise SystemExit("no noisy/clean pairs found")
    if not DNSMOS_MODEL.exists():
        raise SystemExit(f"missing {DNSMOS_MODEL}; run ./setup.sh first")

    models = [m for m in shipped.catalogue() if m.loadable and m.realtime]
    print(f"{len(takes)} takes, {len(models)} models, one run\n")

    rows: list[dict] = []

    # The unprocessed input, so every model has a floor to be measured
    # against rather than only against each other.
    for entry in [None, *models]:
        name = "raw input" if entry is None else entry.name
        scores: list[dict[str, float]] = []
        seconds = 0.0
        started = time.perf_counter()
        failed = None

        for noisy_path, clean_path in takes:
            noisy, sr = sf.read(noisy_path, dtype="float32")
            clean, _ = sf.read(clean_path, dtype="float32")
            seconds += noisy.size / sr
            try:
                processed = noisy if entry is None else shipped.process(noisy, sr, entry)
            except RuntimeError as error:
                failed = str(error)
                break
            scores.append(score_one(processed, clean, sr))

        if failed:
            print(f"{name:24} skipped: {failed}")
            continue

        mean = {k: float(np.nanmean([s[k] for s in scores])) for k in scores[0]}
        # Wall clock over audio duration: what a real-time chain must beat.
        mean["rtf"] = (time.perf_counter() - started) / seconds
        mean["model"] = name
        rows.append(mean)
        print(
            f"{name:24} PESQ {mean.get('pesq_wb', float('nan')):.2f}  "
            f"eSTOI {mean.get('estoi', float('nan')):.3f}  "
            f"SI-SDR {mean.get('si_sdr_db', float('nan')):6.2f}  "
            f"DNSMOS ovrl {mean.get('dnsmos_ovrl', float('nan')):.2f}  "
            f"atraso {mean.get('delay_ms', float('nan')):5.1f} ms  "
            f"RTF {mean['rtf']:.2f}"
        )

    provenance = {
        "takes": len(takes),
        "corpus": "VoiceBank+DEMAND test split",
        "loudness_dbfs": dnsmos.NORMALIZE_DBFS,
        "dnsmos_sha256": sha256(DNSMOS_MODEL) if DNSMOS_MODEL.exists() else None,
        "resampler": "ffmpeg aresample (soxr default)",
        "cpu": platform.processor() or platform.machine(),
        "host": subprocess.run(
            ["uname", "-r"], capture_output=True, text=True, check=False
        ).stdout.strip(),
    }
    print("\nprovenance:", json.dumps(provenance))

    if args.json:
        args.json.write_text(json.dumps({"provenance": provenance, "rows": rows}, indent=2))
        print(f"wrote {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
