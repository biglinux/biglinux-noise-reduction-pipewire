#!/usr/bin/env python3
"""Generate the Markdown comparison tables from a compare_systems.py JSON.

Reads the raw per-file rows and `systems.toml`, aggregates per system, and
writes README-ready tables. Numbers are never hand-edited — this is the only
place the tables come from. Published upstream specs (`published_*` in
systems.toml) stay in their own columns, labelled, and are never blended with
measured values.

    python gen_report.py results/mvp_e1.json --out reports/COMPARISON.md
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
import tomllib

HERE = Path(__file__).resolve().parent


def load_systems() -> dict[str, dict]:
    with (HERE / "systems.toml").open("rb") as f:
        return {s["label"]: s for s in tomllib.load(f)["system"]}


def aggregate(rows: list[dict]) -> dict[str, dict]:
    by_sys: dict[str, list[dict]] = {}
    for r in rows:
        by_sys.setdefault(r["system"], []).append(r)
    agg: dict[str, dict] = {}
    for name, entries in by_sys.items():
        agg[name] = {
            k: float(np.nanmean([e[k] for e in entries if k in e]))
            for k in ("pesq_wb", "estoi", "si_sdri_db", "dnsmos_ovrl", "delay_ms", "rtf")
        }
    return agg


def _num(v: float, fmt: str) -> str:
    return "—" if v is None or (isinstance(v, float) and np.isnan(v)) else format(v, fmt)


def _origin(systems: dict[str, dict], label: str) -> str:
    return systems.get(label, {}).get("origin", "—")


def table_a(systems: dict[str, dict], present: list[str]) -> list[str]:
    lines = [
        "### Architecture (published / verifiable)",
        "",
        "| Implementation | Origin | Rate | Backend | Quantization | External runtime | Params | MACs |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for label in present:
        s = systems.get(label, {})
        lines.append(
            f"| {label} | {s.get('origin', '—')} "
            f"| {s.get('sample_rate', '—') and str(int(s['sample_rate'] / 1000)) + 'k'} "
            f"| {s.get('backend', '—')} | {s.get('quantization', '—')} "
            f"| {s.get('external_runtime', '—')} | {s.get('published_params', '—')} "
            f"| {s.get('published_macs', '—')} |"
        )
    return lines


def table_b(agg: dict[str, dict], order: list[str], systems: dict[str, dict], title: str) -> list[str]:
    lines = [
        f"### Quality — {title}",
        "",
        (
            "PESQ-WB / eSTOI / SI-SDRi vs the clean reference (higher = better); "
            "delay is the aligned latency; RTF = processing / audio (lower = better). "
            "RTF is host-measured: LADSPA rows run through ffmpeg and pay a per-clip "
            "model reload, ONNX rows reuse a warm session — so LADSPA RTF is if anything "
            "pessimistic. A host-consistent steady-state per-frame measure is the "
            "dedicated bin (pending)."
        ),
        "",
        "| Implementation | Origin | PESQ-WB | eSTOI | SI-SDRi (dB) | DNSMOS OVRL | delay (ms) | RTF |",
        "|---|---|---:|---:|---:|---:|---:|---:|",
    ]
    for name in order:
        if name not in agg:
            continue
        a = agg[name]
        origin = "—" if name == "raw input" else _origin(systems, name)
        lines.append(
            f"| {name} | {origin} | {_num(a['pesq_wb'], '.2f')} | {_num(a['estoi'], '.3f')} "
            f"| {_num(a['si_sdri_db'], '+.2f')} | {_num(a['dnsmos_ovrl'], '.2f')} "
            f"| {_num(a['delay_ms'], '.0f')} | {_num(a['rtf'], '.3f')} |"
        )
    return lines


def table_c(resources: dict, order: list[str]) -> list[str]:
    """Deploy footprint, measured per system on one clip.

    Core artifact is the deployable file (our self-contained `.so`, or the ONNX
    model); runtime dep is the ONNX Runtime a native plugin does not need. Peak
    RSS is the whole processing host's peak (ffmpeg for LADSPA, python+ORT for
    ONNX). Processing cost is deliberately NOT here: a one-clip CPU number is
    dominated by one-time startup (importing onnxruntime and building a session
    costs seconds, dwarfing the sub-second inference), so it slanders the ONNX
    systems. The steady-state processing cost is the RTF column above; a fair
    per-frame CPU/jitter comparison across hosts is the dedicated bin (pending).
    """
    by_label = {r["label"]: r for r in resources.get("rows", [])}
    lines = [
        f"### Deploy footprint (measured, one clip: {resources.get('clip', '—')})",
        "",
        (
            "Core artifact = the deployable file; runtime dep = ONNX Runtime a native "
            "plugin does not need. Peak RSS is the whole host's peak — LADSPA rows share "
            "ffmpeg's ~126 MB baseline, so the plugin's own memory is far smaller than the "
            "number shown; the ONNX rows carry python + ORT. Compare artifact size across "
            "hosts, RSS only within a host. Processing cost is the RTF column above."
        ),
        "",
        "| Implementation | Origin | Core artifact | Runtime dep | Peak RSS | RSS host |",
        "|---|---|---:|---:|---:|---|",
    ]
    for name in order:
        r = by_label.get(name)
        if r is None:
            continue
        runtime = f"{r['runtime_bytes'] / 1e6:.1f} MB" if r["runtime_bytes"] else "none"
        lines.append(
            f"| {name} | {r['origin']} | {r['core_bytes'] / 1e6:.2f} MB | {runtime} "
            f"| {r['peak_rss_bytes'] / 1e6:.0f} MB | {r['rss_host']} |"
        )
    return lines


def table_d(cpu: dict, order: list[str]) -> list[str]:
    """Steady-state processing cost, warm and host-consistent.

    Total process CPU per second of audio (all threads, one thread each) is the
    honest compute cost. LADSPA rows also carry the audio-thread per-block RTF
    (p50/p99, from ladspa_bench); a worker-backed plugin shows a near-free audio
    thread but a heavy total, since the work is on a worker core. ONNX rows are
    timed with a warm session, total CPU only.
    """
    by = {r["label"]: r for r in cpu.get("rows", [])}
    lines = [
        "### Steady-state CPU cost (measured warm: ladspa_bench / warm ONNX session)",
        "",
        (
            "Total CPU RTF is process CPU across all threads per second of audio — the "
            "honest compute cost. Audio-thread RTF is the per-block cost on the real-time "
            "thread; the gap between them is work a plugin pushed to a worker core."
        ),
        "",
        "| Implementation | Origin | audio-thread RTF p50 | p99 | total CPU RTF |",
        "|---|---|---:|---:|---:|",
    ]
    for name in order:
        r = by.get(name)
        if r is None:
            continue
        lines.append(
            f"| {name} | {r['origin']} | {_num(r.get('rtf_p50'), '.4f')} "
            f"| {_num(r.get('rtf_p99'), '.4f')} | {_num(r.get('cpu_rtf'), '.4f')} |"
        )
    return lines


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("quality", type=Path, nargs="+", help="compare_systems JSON(s), one per dataset")
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--resources", type=Path, help="bench_resources.py output (optional)")
    p.add_argument("--cpu", type=Path, help="bench_cpu.py output (optional)")
    args = p.parse_args()

    systems = load_systems()
    sys_order = [s["label"] for s in tomllib.loads((HERE / "systems.toml").read_text())["system"]]
    present_a = sys_order

    first = json.loads(args.quality[0].read_text())["provenance"]
    out = [
        "## How does it compare?",
        "",
        (
            "> These systems have different architectures and, in some cases, different "
            "native sample rates and deployment goals. Results below use the same hardware "
            "and test material. Published upstream specifications are kept separate from "
            "measurements performed here."
        ),
        "",
        (
            f"Machine: CPU {first.get('cpu')}, kernel {first.get('kernel')}. "
            f"Loudness normalized to {first.get('loudness_dbfs')} dBFS. "
            f"Resampler: {first.get('resampler')}."
        ),
        "",
        *table_a(systems, present_a),
        "",
    ]

    for qpath in args.quality:
        data = json.loads(qpath.read_text())
        agg = aggregate(data["rows"])
        prov = data["provenance"]
        order = [n for n in ["raw input", *sys_order] if n in agg]
        title = f"{prov.get('corpus')} ({prov.get('pairs')} pairs)"
        out += [*table_b(agg, order, systems, title), ""]

    if args.cpu and args.cpu.exists():
        out += [*table_d(json.loads(args.cpu.read_text()), present_a), ""]
    if args.resources and args.resources.exists():
        out += [*table_c(json.loads(args.resources.read_text()), present_a), ""]

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(out) + "\n")
    print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
