"""Objective metrics for filter-chain calibration.

All metrics return a flat dict so they can be aggregated into a CSV
or markdown table.

The intrusive metrics compare a clean reference against a processed
signal sample by sample, so they are only meaningful once the two are
aligned in time. Every denoiser here adds latency — 20 to 50 ms of STFT
framing, model group delay and worker handoff — and comparing without
compensating for it does not produce a slightly worse score, it produces
a meaningless one. `align_by_delay` estimates the shift by cross
correlation and reports it, so a wrong estimate is visible in the output
rather than silently folded into the score.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
from scipy.signal import resample_poly


#: Widest delay searched for, in seconds. Comfortably past the ~50 ms the
#: heaviest chain adds; searching further only invites a spurious peak.
MAX_DELAY_S = 0.25


def estimate_delay(reference: np.ndarray, processed: np.ndarray, sr: int) -> int:
    """Samples `processed` lags `reference` by, from cross correlation.

    Positive means the processed signal is late, which is the only
    direction a causal filter can produce. Returns 0 when neither signal
    carries enough energy to correlate.
    """
    max_lag = int(MAX_DELAY_S * sr)
    n = min(reference.size, processed.size)
    if n <= max_lag * 2:
        return 0
    a = reference[:n].astype(np.float64)
    b = processed[:n].astype(np.float64)
    a -= a.mean()
    b -= b.mean()
    if a.std() < 1e-9 or b.std() < 1e-9:
        return 0
    # Correlate over a window in the middle, away from the fade-in the
    # first frames of any overlap-add produce.
    lo = min(n // 4, max_lag * 2)
    seg = slice(lo, min(n, lo + 10 * sr))
    corr = np.correlate(b[seg], a[seg], mode="full")
    centre = corr.size // 2
    window = corr[centre : centre + max_lag + 1]
    return int(np.argmax(window))


def align_by_delay(
    reference: np.ndarray, processed: np.ndarray, sr: int
) -> tuple[np.ndarray, np.ndarray, int]:
    """Shift `processed` back onto `reference` and trim both to one length.

    Returns the aligned pair and the delay applied, so the caller can
    publish it: a delay near zero on a chain known to add 30 ms is the
    signal that the estimate, not the chain, is wrong.
    """
    delay = estimate_delay(reference, processed, sr)
    shifted = processed[delay:] if delay else processed
    n = min(reference.size, shifted.size)
    return (
        reference[:n].astype(np.float32),
        shifted[:n].astype(np.float32),
        delay,
    )


def _align(reference: np.ndarray, processed: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Trim to a common length. Callers that compare sample by sample must
    use `align_by_delay` instead; this is for metrics that do not."""
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
    """Run DNSMOS on processed audio, as `dnsmos_sig` / `_bak` / `_ovrl`.

    A missing runtime or an unreadable model gives NaN, because a machine
    without onnxruntime should still get the intrusive metrics. Anything
    else is allowed to raise: an earlier version caught every exception
    here, and a whole comparison run came back with NaN in the column
    that was supposed to answer the question.
    """
    try:
        from . import dnsmos as dm
    except ImportError:
        return {"dnsmos_sig": float("nan"), "dnsmos_bak": float("nan"), "dnsmos_ovrl": float("nan")}
    return dm.score_batch(processed, sr, model_path).as_dict()


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
        # Align once, here, so every intrusive metric below sees the same
        # pair and the delay is reported beside the scores it produced.
        aligned_ref, aligned_proc, delay = align_by_delay(reference, processed, sr)
        out["delay_ms"] = delay / sr * 1000.0
        out["pesq_wb"] = pesq_wb(aligned_ref, aligned_proc, sr)
        out["stoi"] = stoi(aligned_ref, aligned_proc, sr, extended=False)
        out["estoi"] = stoi(aligned_ref, aligned_proc, sr, extended=True)
        out["si_sdr_db"] = si_sdr_db(aligned_ref, aligned_proc)
    if dnsmos_model is not None and Path(dnsmos_model).exists():
        out.update(dnsmos_scores(processed, sr, Path(dnsmos_model)))
    return out
