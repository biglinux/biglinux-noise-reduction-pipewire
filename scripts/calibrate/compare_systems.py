#!/usr/bin/env python3
"""Diagnostic comparison of speech-enhancement systems on the same audio.

Drives every system in `systems.toml` over VoiceBank+DEMAND noisy/clean pairs
and scores each against the clean reference. LADSPA systems run through ffmpeg
(`lib/shipped.process_filter`); ONNX systems run through the streaming OLA loop
(`lib/denoisers`). Both pay their own resample cost.

The point is not a leaderboard: it is to see where our own W8A16 ports lose —
against the f32 DeepFilterNet upstream (quantization gap), against DPDFNet, and
where RTF/latency hurt. Every number here is measured on this machine, this run;
`published_*` values from `systems.toml` are never mixed in — the report keeps
them in a separate column labelled "upstream reported".

    python compare_systems.py --limit 20 --json results/mvp.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import resource
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
import soundfile as sf
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import cache_root, denoisers, metrics, shipped

CACHE = cache_root()
DATASET = CACHE / "datasets/voicebank_demand"
DNSMOS_MODEL = CACHE / "models/dnsmos/sig_bak_ovr.onnx"
SYSTEMS_TOML = Path(__file__).resolve().parent / "systems.toml"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()[:12]


def load_systems() -> list[dict]:
    with SYSTEMS_TOML.open("rb") as f:
        return tomllib.load(f)["system"]


def pairs(limit: int, noisy_dir: Path, clean_dir: Path) -> list[tuple[Path, Path]]:
    if not noisy_dir.is_dir():
        raise SystemExit(f"missing {noisy_dir}; run ./setup.sh first")
    found = sorted(noisy_dir.glob("*.wav"))[:limit]
    return [(n, clean_dir / n.name) for n in found if (clean_dir / n.name).exists()]


def enhance(system: dict, noisy: np.ndarray, sr: int) -> tuple[np.ndarray, float]:
    """Run one system on one clip. Returns (enhanced, processing_seconds).

    Raises RuntimeError (ladspa) if the plugin refuses to load, so a
    passthrough is never scored as a denoiser.
    """
    if system["kind"] == "ladspa":
        flt = f"ladspa=f={system['so']}:p={system['plugin_label']}"
        if system.get("controls"):
            flt += f":controls={system['controls']}"
        t0 = time.perf_counter()
        out = shipped.process_filter(noisy, sr, system["sample_rate"], flt, system["label"])
        return out, time.perf_counter() - t0
    if system["kind"] == "onnx":
        # Build the ORT session once per system, not once per clip: session
        # creation and its warm-up dwarf a single clip's inference.
        runner = system.get("_runner")
        if runner is None:
            # onnx paths are machine-agnostic: relative ones resolve under the
            # calibrate cache models dir (same convention as DATASET/DNSMOS_MODEL),
            # so systems.toml carries no developer-specific absolute paths.
            onnx = Path(system["onnx"]).expanduser()
            if not onnx.is_absolute():
                onnx = CACHE / onnx
            runner = system["_runner"] = denoisers.load(system["onnx_name"], onnx)
        return runner.run(noisy, sr)
    raise SystemExit(f"{system['id']}: unknown kind {system['kind']!r}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--limit", type=int, default=20, help="pairs to score")
    parser.add_argument("--json", type=Path, help="write raw per-file rows here")
    parser.add_argument("--only", action="append", default=[], help="system id filter (repeatable)")
    parser.add_argument("--dataset", default="voicebank_demand", help="corpus label for the rows")
    parser.add_argument("--noisy-dir", type=Path, default=DATASET / "noisy_testset_wav")
    parser.add_argument("--clean-dir", type=Path, default=DATASET / "clean_testset_wav")
    args = parser.parse_args()

    takes = pairs(args.limit, args.noisy_dir, args.clean_dir)
    if not takes:
        raise SystemExit(f"no noisy/clean pairs found in {args.noisy_dir}")
    if not DNSMOS_MODEL.exists():
        raise SystemExit(f"missing {DNSMOS_MODEL}; run ./setup.sh")

    systems = [s for s in load_systems() if not args.only or s["id"] in args.only]
    print(f"{len(takes)} pairs, {len(systems)} systems, one run\n")

    # Raw floor per file: the unprocessed SI-SDR, so SI-SDRi is a delta.
    raw_si: dict[str, float] = {}
    rows: list[dict] = []

    # "raw input" row first, as the floor every system is measured against.
    for entry in [None, *systems]:
        name = "raw input" if entry is None else entry["label"]
        per_file: list[dict] = []
        for noisy_path, clean_path in takes:
            noisy, sr = sf.read(noisy_path, dtype="float32")
            clean, _ = sf.read(clean_path, dtype="float32")
            audio_dur = noisy.size / sr
            try:
                if entry is None:
                    processed, dt = noisy, 0.0
                else:
                    processed, dt = enhance(entry, noisy, sr)
            except RuntimeError as error:
                print(f"{name:24} skipped: {error}")
                per_file = []
                break
            m = metrics.score_pair(clean, processed, sr, dnsmos_model=DNSMOS_MODEL)
            if entry is None:
                raw_si[noisy_path.name] = m.get("si_sdr_db", float("nan"))
                m["si_sdri_db"] = 0.0
            else:
                m["si_sdri_db"] = m.get("si_sdr_db", float("nan")) - raw_si.get(
                    noisy_path.name, float("nan")
                )
            m["rtf"] = dt / audio_dur if audio_dur > 0 else float("nan")
            row = {"system": name, "file": noisy_path.name, "dataset": args.dataset, **m}
            per_file.append(row)
            rows.append(row)

        if not per_file:
            continue
        agg = {
            k: float(np.nanmean([r[k] for r in per_file]))
            for k in ("pesq_wb", "estoi", "si_sdri_db", "dnsmos_ovrl", "delay_ms", "rtf")
            if k in per_file[0]
        }
        print(
            f"{name:24} PESQ {agg.get('pesq_wb', float('nan')):.2f}  "
            f"eSTOI {agg.get('estoi', float('nan')):.3f}  "
            f"SI-SDRi {agg.get('si_sdri_db', float('nan')):6.2f}  "
            f"DNSMOS {agg.get('dnsmos_ovrl', float('nan')):.2f}  "
            f"delay {agg.get('delay_ms', float('nan')):5.1f}ms  "
            f"RTF {agg.get('rtf', float('nan')):.3f}",
            flush=True,
        )

    provenance = {
        "corpus": args.dataset,
        "pairs": len(takes),
        "loudness_dbfs": -26.0,
        "dnsmos_sha256": sha256(DNSMOS_MODEL),
        "resampler": "ffmpeg aresample (soxr) for ladspa; scipy resample_poly for onnx",
        "cpu": platform.processor() or platform.machine(),
        "kernel": subprocess.run(["uname", "-r"], capture_output=True, text=True, check=False).stdout.strip(),
        "peak_rss_self_kb": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
        "peak_rss_children_kb": resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss,
    }
    print("\nprovenance:", json.dumps(provenance))

    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps({"provenance": provenance, "rows": rows}, indent=2))
        print(f"wrote {args.json} ({len(rows)} rows)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
