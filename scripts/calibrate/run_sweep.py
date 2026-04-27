#!/usr/bin/env python3
"""Sweep parameter grid across input samples and score each output.

Default sweep is small and fast (~2 min on a 5-sample directory): it
evaluates the current production defaults and the new presence preset
against every common-sense alternative we'd consider tuning. Override
the matrix via `--config` to pin the exact axes a calibration session
needs.

Output: `report.csv` plus a markdown summary `report.md`. Both go to
the cache by default so the repo stays clean.
"""

from __future__ import annotations

import argparse
import csv
import itertools
import json
import os
import sys
from dataclasses import asdict, replace
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import chain, metrics, signals  # noqa: E402


def _default_cache() -> Path:
    return Path(
        os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache"))
    ) / "biglinux-noise-reduction-pipewire/calibration"


def _discover_samples(*roots: Path) -> list[Path]:
    out = []
    for root in roots:
        if not root.exists():
            continue
        for ext in ("*.wav", "*.ogg", "*.m4a", "*.flac", "*.mp3"):
            out.extend(sorted(root.glob(ext)))
    return out


def _eq_preset(name: str) -> tuple[float, ...]:
    """Mirror of the production preset table — kept here so the
    calibration runner doesn't need a Rust build."""
    presets = {
        "flat": (0.0,) * 10,
        # Updated presence (post f336542 / 4ba77fe).
        "presence": (0.0, 0.0, 0.0, -3.0, -2.0, 2.0, 8.0, 10.0, 5.0, 0.0),
        "default_voice": (0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0, 0.0),
        "podcast": (5.0, 5.0, 10.0, 5.0, 0.0, 5.0, 10.0, 5.0, 0.0, -5.0),
        "de_esser": (0.0, 0.0, 0.0, 0.0, 0.0, -5.0, -15.0, -25.0, -20.0, -10.0),
    }
    if name not in presets:
        raise SystemExit(f"unknown preset: {name}")
    return presets[name]


def _build_matrix(args) -> list[chain.ChainSettings]:
    """Default sweep: production-relevant axes, no combinatorial blow-up."""
    base = chain.ChainSettings(
        gtcrn_enabled=args.gtcrn_model is not None,
        gtcrn_model=args.gtcrn_model,
    )
    matrix: list[chain.ChainSettings] = []

    # 1. Bypass — establishes the unprocessed reference floor.
    matrix.append(replace(base, gtcrn_enabled=False, hpf_enabled=False, eq_enabled=False))

    # 2. HPF candidates.
    for freq in (40.0, 80.0, 100.0):
        for cascaded in (False, True):
            matrix.append(
                replace(base, hpf_enabled=True, hpf_freq_hz=freq, hpf_cascaded=cascaded)
            )

    # 3. EQ presets (HPF on at 80/cascaded — the new prod default).
    for preset in ("flat", "presence", "default_voice", "podcast"):
        matrix.append(
            replace(
                base,
                hpf_enabled=True,
                hpf_freq_hz=80.0,
                hpf_cascaded=True,
                eq_enabled=True,
                eq_gains_db=_eq_preset(preset),
            )
        )

    return matrix


def _label(s: chain.ChainSettings) -> str:
    parts = []
    parts.append(f"hpf={'on' if s.hpf_enabled else 'off'}")
    if s.hpf_enabled:
        parts[-1] += f"@{int(s.hpf_freq_hz)}{'_lr4' if s.hpf_cascaded else '_bw2'}"
    parts.append(f"nr={'on' if s.gtcrn_enabled else 'off'}")
    parts.append(f"eq={'on' if s.eq_enabled else 'off'}")
    if s.eq_enabled:
        # Reverse-lookup against the small preset table.
        gains = tuple(s.eq_gains_db)
        for name in ("flat", "presence", "default_voice", "podcast"):
            if gains == _eq_preset(name):
                parts[-1] += f":{name}"
                break
        else:
            parts[-1] += ":custom"
    return "|".join(parts)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--samples-dir",
        type=Path,
        action="append",
        default=[],
        help="Add a directory of input audio. May be repeated.",
    )
    p.add_argument("--gtcrn-model", type=Path, help="Path to GTCRN ONNX (optional)")
    p.add_argument(
        "--dnsmos",
        type=Path,
        default=_default_cache() / "models/dnsmos/sig_bak_ovr.onnx",
    )
    p.add_argument("--out", type=Path, default=_default_cache() / "reports")
    p.add_argument("--limit-samples", type=int, default=0, help="0 = all")
    args = p.parse_args()

    if not args.samples_dir:
        args.samples_dir = [
            Path("/home/bruno/codigo-pacotes/multimidia"),
            _default_cache() / "datasets/dns/wav",
            _default_cache() / "signals",
        ]

    samples = _discover_samples(*args.samples_dir)
    if args.limit_samples > 0:
        samples = samples[: args.limit_samples]
    if not samples:
        print("no input samples found — pass --samples-dir or run setup.sh", file=sys.stderr)
        return 1

    args.out.mkdir(parents=True, exist_ok=True)
    matrix = _build_matrix(args)
    rows: list[dict] = []

    for sample in samples:
        try:
            x, sr = signals.read_wav(sample)
        except Exception as e:  # noqa: BLE001
            print(f"skip {sample.name}: {e}", file=sys.stderr)
            continue
        for cfg in matrix:
            try:
                y = chain.apply_chain(x, sr, cfg)
                m = metrics.score_pair(None, y, sr, dnsmos_model=args.dnsmos)
            except Exception as e:  # noqa: BLE001
                print(f"  fail {_label(cfg)} on {sample.name}: {e}", file=sys.stderr)
                continue
            row = {"sample": sample.name, "config": _label(cfg), **m}
            rows.append(row)
            print(f"{sample.name:50s}  {_label(cfg):60s}  ovrl={m.get('dnsmos_ovrl', float('nan')):.2f}")

    if not rows:
        print("no rows produced", file=sys.stderr)
        return 1

    fieldnames = sorted({k for r in rows for k in r.keys()})
    csv_path = args.out / "report.csv"
    with csv_path.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fieldnames)
        w.writeheader()
        w.writerows(rows)
    print(f"\nwrote {csv_path} ({len(rows)} rows)")

    md_path = args.out / "report.md"
    _write_markdown(rows, md_path)
    print(f"wrote {md_path}")
    return 0


def _write_markdown(rows: list[dict], dest: Path) -> None:
    """Aggregate by config across samples, write a ranked table."""
    by_cfg: dict[str, list[dict]] = {}
    for r in rows:
        by_cfg.setdefault(r["config"], []).append(r)

    metric_keys = [
        "dnsmos_ovrl",
        "dnsmos_sig",
        "dnsmos_bak",
        "lufs",
        "crest_db",
        "energy_sub80_db",
        "energy_2k_4k_db",
    ]

    summary: list[tuple[str, dict[str, float]]] = []
    for cfg, entries in by_cfg.items():
        agg = {}
        for k in metric_keys:
            vals = [e.get(k, float("nan")) for e in entries]
            vals = [v for v in vals if not np.isnan(v)]
            agg[k] = float(np.mean(vals)) if vals else float("nan")
        summary.append((cfg, agg))
    summary.sort(key=lambda kv: -kv[1].get("dnsmos_ovrl", float("nan")))

    lines = [
        "# Calibration report",
        "",
        f"Configurations evaluated: {len(by_cfg)}",
        f"Samples per config: {len(next(iter(by_cfg.values())))}",
        "",
        "Ranked by DNSMOS OVRL (mean across samples). Higher = better.",
        "",
        "| config | OVRL | SIG | BAK | LUFS | crest dB | sub-80 dB | 2-4 kHz dB |",
        "|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for cfg, agg in summary:
        lines.append(
            f"| `{cfg}` "
            f"| {agg['dnsmos_ovrl']:.2f} "
            f"| {agg['dnsmos_sig']:.2f} "
            f"| {agg['dnsmos_bak']:.2f} "
            f"| {agg['lufs']:.1f} "
            f"| {agg['crest_db']:.1f} "
            f"| {agg['energy_sub80_db']:.1f} "
            f"| {agg['energy_2k_4k_db']:.1f} |"
        )
    dest.write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    sys.exit(main())
