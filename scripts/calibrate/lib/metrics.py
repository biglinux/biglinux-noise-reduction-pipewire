"""Objective metrics for filter-chain calibration.

All metrics return a flat dict so they can be aggregated into a CSV
or markdown table. Each function tolerates length mismatches and
silently truncates to the shorter pair.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
from scipy.signal import resample_poly


def _align(reference: np.ndarray, processed: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    n = min(reference.size, processed.size)
    return reference[:n].astype(np.float32), processed[:n].astype(np.float32)


def _resample(x: np.ndarray, src_sr: int, dst_sr: int) -> np.ndarray:
    if src_sr == dst_sr:
        return x
    return resample_poly(x, dst_sr, src_sr).astype(np.float32)


# ── No-reference metrics ────────────────────────────────────────────


def lufs(x: np.ndarray, sr: int) -> float:
    """Integrated loudness (ITU-R BS.1770-4)."""
    import pyloudnorm as pyln

    meter = pyln.Meter(sr)
    return float(meter.integrated_loudness(x))


def rms_db(x: np.ndarray) -> float:
    return 20.0 * float(np.log10(np.sqrt(np.mean(x.astype(np.float64) ** 2)) + 1e-12))


def peak_db(x: np.ndarray) -> float:
    return 20.0 * float(np.log10(np.max(np.abs(x)) + 1e-12))


def crest_factor_db(x: np.ndarray) -> float:
    return peak_db(x) - rms_db(x)


def spectrum_band_energy_db(x: np.ndarray, sr: int, lo_hz: float, hi_hz: float) -> float:
    """Energy in a band — used to verify HPF rolloff and EQ band lift."""
    n = max(2048, 1 << (int(np.ceil(np.log2(x.size))) - 1))
    fft = np.fft.rfft(x[:n])
    freqs = np.fft.rfftfreq(n, 1.0 / sr)
    mask = (freqs >= lo_hz) & (freqs < hi_hz)
    energy = float(np.sum(np.abs(fft[mask]) ** 2)) + 1e-12
    return 10.0 * float(np.log10(energy))


# ── Reference-based metrics ─────────────────────────────────────────


def pesq_wb(reference: np.ndarray, processed: np.ndarray, sr: int) -> float:
    """Wideband PESQ at 16 kHz. Returns NaN if PESQ refuses (e.g. silence)."""
    from pesq import pesq as pesq_fn

    r16 = _resample(reference, sr, 16000)
    p16 = _resample(processed, sr, 16000)
    r16, p16 = _align(r16, p16)
    try:
        return float(pesq_fn(16000, r16, p16, "wb"))
    except Exception:
        return float("nan")


def stoi(reference: np.ndarray, processed: np.ndarray, sr: int, extended: bool = True) -> float:
    """Short-Time Objective Intelligibility (0..1, higher = better)."""
    from pystoi import stoi as stoi_fn

    r, p = _align(reference, processed)
    return float(stoi_fn(r, p, sr, extended=extended))


def si_sdr_db(reference: np.ndarray, processed: np.ndarray) -> float:
    """Scale-invariant SDR — robust to gain mismatch."""
    r, p = _align(reference, processed)
    r = r - r.mean()
    p = p - p.mean()
    alpha = float(np.dot(p, r) / (np.dot(r, r) + 1e-12))
    target = alpha * r
    noise = p - target
    return 10.0 * float(np.log10((np.sum(target**2) + 1e-12) / (np.sum(noise**2) + 1e-12)))


# ── DNSMOS hook ─────────────────────────────────────────────────────


def dnsmos_scores(processed: np.ndarray, sr: int, model_path: Path) -> dict[str, float]:
    """Run DNSMOS on processed audio. Returns dict with sig/bak/ovrl
    keys (always present, may be NaN if the model fails to load)."""
    try:
        from . import dnsmos as dm

        s = dm.score_batch(processed, sr, model_path)
        return s.as_dict()
    except Exception:
        return {"dnsmos_sig": float("nan"), "dnsmos_bak": float("nan"), "dnsmos_ovrl": float("nan")}


# ── Aggregator ──────────────────────────────────────────────────────


def score_pair(
    reference: np.ndarray | None,
    processed: np.ndarray,
    sr: int,
    dnsmos_model: Path | None = None,
) -> dict[str, float]:
    """Compute every available metric. `reference=None` skips PESQ/STOI/SDR."""
    out: dict[str, float] = {
        "lufs": lufs(processed, sr),
        "rms_db": rms_db(processed),
        "peak_db": peak_db(processed),
        "crest_db": crest_factor_db(processed),
        "energy_sub80_db": spectrum_band_energy_db(processed, sr, 0.0, 80.0),
        "energy_80_300_db": spectrum_band_energy_db(processed, sr, 80.0, 300.0),
        "energy_300_2k_db": spectrum_band_energy_db(processed, sr, 300.0, 2000.0),
        "energy_2k_4k_db": spectrum_band_energy_db(processed, sr, 2000.0, 4000.0),
        "energy_4k_8k_db": spectrum_band_energy_db(processed, sr, 4000.0, 8000.0),
        "energy_8k_plus_db": spectrum_band_energy_db(processed, sr, 8000.0, sr / 2),
    }
    if reference is not None:
        out["pesq_wb"] = pesq_wb(reference, processed, sr)
        out["stoi"] = stoi(reference, processed, sr, extended=False)
        out["estoi"] = stoi(reference, processed, sr, extended=True)
        out["si_sdr_db"] = si_sdr_db(reference, processed)
    if dnsmos_model is not None and Path(dnsmos_model).exists():
        out.update(dnsmos_scores(processed, sr, Path(dnsmos_model)))
    return out
