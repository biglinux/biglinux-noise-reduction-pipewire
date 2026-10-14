#!/usr/bin/env python3
"""Measure per-system memory, CPU cost and install footprint.

The quality run (compare_systems.py) already reports RTF; this adds the two
axes it does not: peak resident memory and the on-disk footprint each system
actually deploys. Both are measured by spawning the real processing of ONE clip
as a child and reading that child's rusage (`os.wait4`), so the number is the
peak RSS of exactly that work — ffmpeg+plugin for LADSPA, python+ONNX Runtime
for ONNX. The two hosts differ, so the report says which host each RSS includes
rather than pretending they are directly comparable.

Footprint separates the *core artifact* (our self-contained `.so`) from the
*runtime footprint* (an ONNX model plus the ONNX Runtime shared libraries it
cannot run without).

    python bench_resources.py --json reports/resources.json
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import cache_root

HERE = Path(__file__).resolve().parent
CACHE = cache_root()
NOISY_DIR = CACHE / "datasets/voicebank_demand/noisy_testset_wav"


def one_clip() -> Path:
    clips = sorted(NOISY_DIR.glob("*.wav"))
    if not clips:
        raise SystemExit(f"no clips in {NOISY_DIR}; run ./setup.sh")
    return clips[0]


def ort_runtime_bytes() -> int:
    """Total size of the ONNX Runtime shared libraries — the dependency an
    ONNX system drags in that a native `.so` does not."""
    try:
        import onnxruntime
    except ImportError:
        return 0
    root = Path(onnxruntime.__file__).parent
    return sum(p.stat().st_size for p in root.rglob("*.so"))


def measure(argv: list[str]) -> tuple[int, float]:
    """Run argv as a child; return (peak_rss_bytes, cpu_seconds) for it."""
    proc = subprocess.Popen(argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _, status, ru = os.wait4(proc.pid, 0)
    proc.returncode = os.waitstatus_to_exitcode(status)
    if proc.returncode != 0:
        raise subprocess.CalledProcessError(proc.returncode, argv)
    return ru.ru_maxrss * 1024, ru.ru_utime + ru.ru_stime


def ladspa_argv(system: dict, clip: Path) -> list[str]:
    sr = system["sample_rate"]
    flt = f"ladspa=f={system['so']}:p={system['plugin_label']}"
    if system.get("controls"):
        flt += f":controls={system['controls']}"
    chain = f"aresample={sr},{flt},aresample={sr}"
    return ["ffmpeg", "-hide_banner", "-loglevel", "error", "-i", str(clip),
            "-af", chain, "-f", "null", "-"]


def onnx_argv(system: dict, clip: Path) -> list[str]:
    script = (
        f"import sys;sys.path.insert(0,{str(HERE)!r});"
        f"from lib import denoisers;import soundfile as sf;"
        f"x,sr=sf.read({str(clip)!r},dtype='float32');"
        f"denoisers.load({system['onnx_name']!r},__import__('pathlib').Path({system['onnx']!r})).run(x,sr)"
    )
    return [sys.executable, "-c", script]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--json", type=Path)
    args = ap.parse_args()

    clip = one_clip()
    ort_bytes = ort_runtime_bytes()
    with (HERE / "systems.toml").open("rb") as f:
        systems = tomllib.load(f)["system"]

    rows = []
    for s in systems:
        if s["kind"] == "ladspa":
            argv = ladspa_argv(s, clip)
            core = Path(s["so"]).stat().st_size
            runtime = 0
            host = "ffmpeg+plugin"
        else:
            argv = onnx_argv(s, clip)
            core = Path(s["onnx"]).stat().st_size
            runtime = ort_bytes
            host = "python+onnxruntime"
        # Only peak RSS is kept: a one-clip CPU time is startup-dominated
        # (building an ONNX session costs seconds, dwarfing the inference) and
        # would misrepresent the steady per-clip cost, which the RTF column of
        # the quality run already reports honestly.
        rss, _cpu = measure(argv)
        row = {
            "label": s["label"],
            "origin": s.get("origin", "—"),
            "core_bytes": core,
            "runtime_bytes": runtime,
            "peak_rss_bytes": rss,
            "rss_host": host,
        }
        rows.append(row)
        print(
            f"{s['label']:26} core {core/1e6:6.2f} MB  "
            f"runtime {runtime/1e6:7.2f} MB  peak RSS {rss/1e6:6.1f} MB ({host})",
            flush=True,
        )

    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps({"clip": clip.name, "rows": rows}, indent=2))
        print(f"wrote {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
