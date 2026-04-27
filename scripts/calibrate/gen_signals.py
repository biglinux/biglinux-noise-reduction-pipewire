#!/usr/bin/env python3
"""Generate canonical test signals for HPF / EQ / compressor calibration.

Produces:
  log_sweep_20_20k.wav   — magnitude-response probe (HPF & EQ)
  sine_1000.wav          — compressor threshold/makeup probe
  white_noise.wav        — broadband floor for SNR mixing
  pink_noise.wav         — 1/f ambient surrogate

All files are 30 s mono, 48 kHz, float32 PCM. Default output dir is
`<cache>/calibration/signals/` so they aren't committed.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import signals as sig  # noqa: E402


def _default_outdir() -> Path:
    cache = Path(os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache")))
    return cache / "biglinux-noise-reduction-pipewire/calibration/signals"


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--outdir", type=Path, default=_default_outdir())
    p.add_argument("--sr", type=int, default=48000)
    p.add_argument("--duration", type=float, default=30.0)
    args = p.parse_args()

    spec = sig.SignalSpec(name="probe", sample_rate=args.sr, duration_s=args.duration)
    args.outdir.mkdir(parents=True, exist_ok=True)

    items = [
        ("log_sweep_20_20k.wav", sig.log_sweep(spec, 20.0, 20000.0, 0.5)),
        ("sine_1000.wav", sig.sine(spec, 1000.0, 0.5)),
        ("white_noise.wav", sig.white_noise(spec, 0.1, seed=1)),
        ("pink_noise.wav", sig.pink_noise(spec, 0.1, seed=1)),
    ]
    for name, x in items:
        sig.write_wav(args.outdir / name, x, args.sr)
        print(f"wrote {args.outdir / name}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
