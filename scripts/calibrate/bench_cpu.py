#!/usr/bin/env python3
"""Steady-state processing cost per system, comparable across hosts.

LADSPA systems are driven by the `ladspa_bench` Rust tool (warm dlopen, no
ffmpeg reload): it reports the audio-thread per-block RTF and the total process
CPU across all threads — the number that stays valid when a plugin offloads to
a worker. ONNX systems are timed here with a warm session over a handful of
clips, process CPU over audio duration. One thread throughout, matching the
real-time deployment. Emits reports/cpu.json for gen_report.

    python bench_cpu.py
"""

from __future__ import annotations

import json
import resource
import subprocess
import sys
from pathlib import Path

import soundfile as sf
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import cache_root, denoisers

HERE = Path(__file__).resolve().parent
CACHE = cache_root()
CLIPS = sorted((CACHE / "datasets/voicebank_demand/noisy_testset_wav").glob("*.wav"))[:10]
BENCH = HERE / "ladspa_bench/target/release/ladspa_bench"


def ladspa_cpu(s: dict) -> dict:
    out = Path("/tmp/ladspa_bench_cpu.json")
    args = [str(BENCH), s["so"], s["plugin_label"], "--seconds", "4", "--json", str(out)]
    for control in s.get("cpu_controls", "").split("|"):
        if control:
            args += ["--control", control]
    subprocess.run(args, stderr=subprocess.DEVNULL, check=True)
    d = json.loads(out.read_text())
    return {"rtf_p50": d["rtf_p50"], "rtf_p99": d["rtf_p99"], "cpu_rtf": d["cpu_rtf"]}


def onnx_cpu(s: dict) -> dict:
    runner = denoisers.load(s["onnx_name"], Path(s["onnx"]))
    warm, sr = sf.read(CLIPS[0], dtype="float32")
    runner.run(warm, sr)  # warm the session out of the measurement
    before = resource.getrusage(resource.RUSAGE_SELF)
    audio = 0.0
    for clip in CLIPS:
        x, sr = sf.read(clip, dtype="float32")
        runner.run(x, sr)
        audio += x.size / sr
    after = resource.getrusage(resource.RUSAGE_SELF)
    cpu = (after.ru_utime + after.ru_stime) - (before.ru_utime + before.ru_stime)
    # No audio-thread/worker split for the python OLA loop; only the total CPU
    # figure is meaningful, so the per-block columns stay empty.
    rtf = cpu / audio if audio else None
    return {"rtf_p50": None, "rtf_p99": None, "cpu_rtf": rtf}


def main() -> int:
    with (HERE / "systems.toml").open("rb") as f:
        systems = tomllib.load(f)["system"]
    rows = []
    for s in systems:
        measure = ladspa_cpu if s["kind"] == "ladspa" else onnx_cpu
        m = measure(s)
        rows.append({"label": s["label"], "origin": s.get("origin", "—"), **m})
        print(f"{s['label']:26} cpu_rtf {m['cpu_rtf']:.4f}", flush=True)
    out = HERE / "reports/cpu.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"rows": rows}, indent=2))
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
