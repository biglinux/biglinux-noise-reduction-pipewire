#!/usr/bin/env python3
"""Score one (reference, processed) pair or one no-reference clip.

Examples:
    python score_pair.py --processed out.wav
    python score_pair.py --reference clean.wav --processed out.wav
    python score_pair.py --processed out.wav --dnsmos ~/.cache/.../sig_bak_ovr.onnx
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

# Allow running as a script without installing the package.
sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import metrics, signals  # noqa: E402


def _default_dnsmos() -> Path | None:
    cache = Path(
        os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache"))
    ) / "biglinux-noise-reduction-pipewire/calibration/models/dnsmos/sig_bak_ovr.onnx"
    return cache if cache.exists() else None


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--reference", type=Path, help="Clean reference WAV")
    p.add_argument("--processed", type=Path, required=True, help="Processed WAV to score")
    p.add_argument(
        "--dnsmos",
        type=Path,
        default=_default_dnsmos(),
        help="DNSMOS ONNX model path (default: cache)",
    )
    p.add_argument("--json", action="store_true", help="Emit JSON instead of a table")
    args = p.parse_args()

    proc, sr = signals.read_wav(args.processed)
    ref = None
    if args.reference:
        ref, ref_sr = signals.read_wav(args.reference)
        if ref_sr != sr:
            from scipy.signal import resample_poly

            ref = resample_poly(ref, sr, ref_sr).astype("float32")

    scores = metrics.score_pair(ref, proc, sr, dnsmos_model=args.dnsmos)

    if args.json:
        print(json.dumps(scores, indent=2))
    else:
        width = max(len(k) for k in scores)
        for k, v in scores.items():
            print(f"  {k:<{width}}  {v:>10.3f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
