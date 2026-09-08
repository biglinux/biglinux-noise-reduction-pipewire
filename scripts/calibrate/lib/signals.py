"""Audio input loading for calibration commands."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import soundfile as sf


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    """Read first channel only, return float32 + sample rate."""
    x, sr = sf.read(str(path), always_2d=False)
    if x.ndim > 1:
        x = x[:, 0]
    return x.astype(np.float32), int(sr)
