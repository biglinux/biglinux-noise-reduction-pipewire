"""Offline emulator of the BigLinux mic filter chain.

The biquad/EQ stages mirror the PipeWire builtin math exactly (RBJ
cookbook formulas, identical to `bq_highpass`/`bq_peaking`). GTCRN
delegates to the common streaming ONNX runner in `denoisers.py`.

Inputs/outputs: float32 mono PCM. Sample rate flows through unchanged.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import numpy as np
from scipy.signal import sosfilt

from . import denoisers


# ── Biquad math (RBJ cookbook, matches PipeWire builtins) ────────────


def _hp_sos(freq_hz: float, q: float, sr: int) -> np.ndarray:
    """Single biquad high-pass. Returns SOS row [b0,b1,b2, a0,a1,a2]."""
    w0 = 2 * np.pi * freq_hz / sr
    cosw = np.cos(w0)
    alpha = np.sin(w0) / (2 * q)
    b0 = (1 + cosw) / 2
    b1 = -(1 + cosw)
    b2 = (1 + cosw) / 2
    a0 = 1 + alpha
    a1 = -2 * cosw
    a2 = 1 - alpha
    return np.array([b0, b1, b2, a0, a1, a2]) / a0


def _peaking_sos(freq_hz: float, q: float, gain_db: float, sr: int) -> np.ndarray:
    """Peaking EQ biquad. `gain_db` is the boost/cut at `freq_hz`."""
    a = 10 ** (gain_db / 40)
    w0 = 2 * np.pi * freq_hz / sr
    cosw = np.cos(w0)
    alpha = np.sin(w0) / (2 * q)
    b0 = 1 + alpha * a
    b1 = -2 * cosw
    b2 = 1 - alpha * a
    a0 = 1 + alpha / a
    a1 = -2 * cosw
    a2 = 1 - alpha / a
    return np.array([b0, b1, b2, a0, a1, a2]) / a0


def _apply_sos(x: np.ndarray, sos_rows: list[np.ndarray]) -> np.ndarray:
    if not sos_rows:
        return x
    sos = np.stack(sos_rows)
    # scipy expects (n_sections, 6) — already the right shape.
    return sosfilt(sos, x).astype(np.float32)


# ── Stage signatures ────────────────────────────────────────────────


def apply_hpf(
    x: np.ndarray, sr: int, freq_hz: float, cascaded: bool = True
) -> np.ndarray:
    """High-pass cascade. `cascaded=True` matches the production chain
    (Linkwitz-Riley 4th order, two Q=0.707 biquads). `cascaded=False`
    is a single-biquad legacy mode for A/B comparison."""
    sections = [_hp_sos(freq_hz, 0.707, sr)]
    if cascaded:
        sections.append(_hp_sos(freq_hz, 0.707, sr))
    return _apply_sos(x, sections)


# `EQ_BANDS_HZ` mirrors `src/config/paths.rs` so config and emulator
# stay aligned without an extra build step. Update both together.
EQ_BANDS_HZ: tuple[int, ...] = (31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000)


def apply_eq(
    x: np.ndarray, sr: int, gains_db: list[float], q: float = 1.41
) -> np.ndarray:
    """Ten cascaded `bq_peaking` biquads at `EQ_BANDS_HZ`. `q=1.41`
    matches the production param_eq node."""
    if len(gains_db) != len(EQ_BANDS_HZ):
        raise ValueError(f"gains length {len(gains_db)} != {len(EQ_BANDS_HZ)}")
    sections = []
    for freq, gain in zip(EQ_BANDS_HZ, gains_db):
        if abs(gain) < 1e-3:
            continue  # 0 dB peaking is a no-op; skip for speed
        sections.append(_peaking_sos(float(freq), q, float(gain), sr))
    return _apply_sos(x, sections)


# ── Full chain composition ──────────────────────────────────────────


@dataclass
class ChainSettings:
    """Settings snapshot for one offline run. Mirrors the prod struct
    fields we actually exercise in calibration sweeps."""

    hpf_enabled: bool = False
    hpf_freq_hz: float = 80.0
    hpf_cascaded: bool = True

    gtcrn_enabled: bool = True
    gtcrn_strength: float = 1.0
    gtcrn_model: Path | None = None

    eq_enabled: bool = False
    eq_gains_db: tuple[float, ...] = (0.0,) * len(EQ_BANDS_HZ)


def apply_chain(x: np.ndarray, sr: int, s: ChainSettings) -> np.ndarray:
    """Run `x` through the calibrated production stages: HPF → GTCRN → EQ."""
    y = x
    if s.hpf_enabled:
        y = apply_hpf(y, sr, s.hpf_freq_hz, cascaded=s.hpf_cascaded)
    if s.gtcrn_enabled and s.gtcrn_model is not None:
        enhanced, _ = denoisers.load("gtcrn", s.gtcrn_model).run(y, sr)
        n = min(len(enhanced), len(y))
        wet = float(np.clip(s.gtcrn_strength, 0.0, 1.0))
        y = ((1.0 - wet) * y[:n] + wet * enhanced[:n]).astype(np.float32)
    if s.eq_enabled:
        y = apply_eq(y, sr, list(s.eq_gains_db))
    return y.astype(np.float32)
