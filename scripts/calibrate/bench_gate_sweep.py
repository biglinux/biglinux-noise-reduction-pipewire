#!/usr/bin/env python3
"""Sweep the DFN3 SNR-gate / stage-skip thresholds: quality vs CPU.

The benchmark showed the shipped default gate degrades speech (SI-SDRi near
zero) while the same model with the gate open reaches +10. The gate exists to
save CPU by skipping stages on easy frames, so the question is not "on or off"
but where the knee is: the loosest thresholds that keep quality yet still save
compute. This scores DFN3 at a handful of threshold sets on a VoiceBank+DEMAND
subset and measures each set's warm CPU with ladspa_bench, isolating the noise
gate (c1) from the stage-skip (c2/c3).

    python bench_gate_sweep.py --limit 100 --json reports/gate_sweep.json
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

import numpy as np
import soundfile as sf

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import cache_root, metrics

HERE = Path(__file__).resolve().parent
CACHE = cache_root()
DATASET = CACHE / "datasets/voicebank_demand"
DNSMOS = CACHE / "models/dnsmos/sig_bak_ovr.onnx"
SO = "/usr/lib/ladspa/libdfn3_ladspa.so"
LABEL = "deep_filter_net3_rs_mono"
BENCH = HERE / "ladspa_bench/target/release/ladspa_bench"

# (name, ffmpeg controls c0..c3, ladspa_bench name=value controls).
# c1 = Min SNR gate (dB), c2 = Skip-all SNR (dB), c3 = Skip-DF SNR (dB).
CONFIGS = [
    ("gate off (full pipeline)", "c1=-40|c2=40|c3=40", "Min SNR gate (dB)=-40|Skip-all SNR (dB)=40|Skip-DF SNR (dB)=40"),
    ("shipped default", "c1=-10|c2=30|c3=20", "Min SNR gate (dB)=-10|Skip-all SNR (dB)=30|Skip-DF SNR (dB)=20"),
    ("gate off, skip default", "c1=-40|c2=30|c3=20", "Min SNR gate (dB)=-40|Skip-all SNR (dB)=30|Skip-DF SNR (dB)=20"),
    ("gate off, skip mild", "c1=-40|c2=35|c3=30", "Min SNR gate (dB)=-40|Skip-all SNR (dB)=35|Skip-DF SNR (dB)=30"),
    ("gate -25, skip default", "c1=-25|c2=30|c3=20", "Min SNR gate (dB)=-25|Skip-all SNR (dB)=30|Skip-DF SNR (dB)=20"),
    # Proposed: keep the noise gate (CPU savings) but disable the harmful skip.
    ("gate -10, skip off", "c1=-10|c2=40|c3=40", "Min SNR gate (dB)=-10|Skip-all SNR (dB)=40|Skip-DF SNR (dB)=40"),
    ("gate -10, skip mild", "c1=-10|c2=35|c3=30", "Min SNR gate (dB)=-10|Skip-all SNR (dB)=35|Skip-DF SNR (dB)=30"),
]


def pairs(limit: int) -> list[tuple[Path, Path]]:
    noisy = sorted((DATASET / "noisy_testset_wav").glob("*.wav"))[:limit]
    return [(n, DATASET / "clean_testset_wav" / n.name) for n in noisy
            if (DATASET / "clean_testset_wav" / n.name).exists()]


def cpu_rtf(bench_controls: str) -> float:
    out = Path("/tmp/gate_cpu.json")
    args = [str(BENCH), SO, LABEL, "--seconds", "4", "--json", str(out)]
    for c in bench_controls.split("|"):
        args += ["--control", c]
    subprocess.run(args, stderr=subprocess.DEVNULL, check=True)
    return json.loads(out.read_text())["cpu_rtf"]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--limit", type=int, default=100)
    ap.add_argument("--json", type=Path)
    ap.add_argument("--match", default="", help="only run configs whose name contains this")
    args = ap.parse_args()

    from lib import shipped

    configs = [c for c in CONFIGS if args.match in c[0]]
    takes = pairs(args.limit)
    if not takes:
        raise SystemExit("no pairs; run ./setup.sh")

    # Raw SI-SDR floor per file, once, so SI-SDRi is a delta.
    raw_si = {}
    for noisy_path, clean_path in takes:
        noisy, sr = sf.read(noisy_path, dtype="float32")
        clean, _ = sf.read(clean_path, dtype="float32")
        raw_si[noisy_path.name] = metrics.si_sdr_db(*metrics.align_by_delay(clean, noisy, sr)[:2])

    rows = []
    for name, ff, bench_controls in configs:
        flt = f"ladspa=f={SO}:p={LABEL}:controls=c0=100|{ff}"
        scores = []
        for noisy_path, clean_path in takes:
            noisy, sr = sf.read(noisy_path, dtype="float32")
            clean, _ = sf.read(clean_path, dtype="float32")
            proc = shipped.process_filter(noisy, sr, 48000, flt, "dfn3")
            m = metrics.score_pair(clean, proc, sr, dnsmos_model=DNSMOS)
            m["si_sdri_db"] = m.get("si_sdr_db", float("nan")) - raw_si[noisy_path.name]
            scores.append(m)
        agg = {k: float(np.nanmean([s.get(k, float("nan")) for s in scores]))
               for k in ("pesq_wb", "estoi", "si_sdri_db", "dnsmos_ovrl")}
        agg["cpu_rtf"] = cpu_rtf(bench_controls)
        agg["config"] = name
        agg["controls"] = ff
        rows.append(agg)
        print(f"{name:26} SI-SDRi {agg['si_sdri_db']:6.2f}  PESQ {agg['pesq_wb']:.2f}  "
              f"DNSMOS {agg['dnsmos_ovrl']:.2f}  CPU {agg['cpu_rtf']:.4f}", flush=True)

    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps({"limit": len(takes), "rows": rows}, indent=2))
        print(f"wrote {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
