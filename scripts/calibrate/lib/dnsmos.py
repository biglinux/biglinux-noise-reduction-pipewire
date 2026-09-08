"""DNSMOS P.835 ONNX wrapper — no-reference speech quality MOS.

DNSMOS predicts three scores from a 9-second 16 kHz mono window:
- SIG  — speech quality   (higher = cleaner voice)
- BAK  — background quality (higher = less residual noise)
- OVRL — overall quality  (combined)

Scores roughly map to the 1–5 MOS scale. We aggregate across windows
(mean) so a one-shot float per metric is returned for any-length input.
"""

from __future__ import annotations

from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path

import numpy as np

_INPUT_WINDOW_SECONDS = 9.01
_TARGET_SAMPLE_RATE_HZ = 16000


@dataclass
class DnsmosScores:
    sig: float
    bak: float
    ovrl: float

    def as_dict(self) -> dict[str, float]:
        return {
            "dnsmos_sig": self.sig,
            "dnsmos_bak": self.bak,
            "dnsmos_ovrl": self.ovrl,
        }


def _polyfit(sig: float, bak: float, ovrl: float) -> DnsmosScores:
    """Apply Microsoft's published linear correction. Numbers come from
    the DNS Challenge `dnsmos_local.py` reference implementation."""
    sig_p = max(1.0, min(5.0, 0.94888 * sig + 0.18217))
    bak_p = max(1.0, min(5.0, 1.00075 * bak + 0.59693))
    ovrl_p = max(1.0, min(5.0, 0.95293 * ovrl + 0.40593))
    return DnsmosScores(sig_p, bak_p, ovrl_p)


@lru_cache(maxsize=4)
def _session(model_path: Path):
    """One session per model file, reused.

    A sweep scores thousands of windows; building an `InferenceSession`
    for each one costs more than the inference does.
    """
    import onnxruntime as ort

    return ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])


#: Level DNSMOS is fed at, in dBFS RMS.
#:
#: The model deliberately has no feature normalization — its authors left
#: it out because people rate quiet clips lower and they wanted that
#: captured — so raw, peak-normalized and loudness-normalized audio all
#: score differently. Any number is defensible; only a fixed one is
#: comparable, and it has to be applied to every system in a comparison
#: including the unprocessed reference.
NORMALIZE_DBFS = -26.0


def normalize(audio: np.ndarray) -> np.ndarray:
    """Bring `audio` to `NORMALIZE_DBFS` RMS. Silence is left alone."""
    rms = float(np.sqrt(np.mean(np.square(audio, dtype=np.float64))))
    if rms < 1e-9:
        return audio
    gain = 10.0 ** (NORMALIZE_DBFS / 20.0) / rms
    return (audio * gain).astype(np.float32)


def score(audio: np.ndarray, sr: int, model_path: Path) -> DnsmosScores:
    """Score one waveform. Resamples to 16 kHz; tiles short clips."""
    from scipy.signal import resample_poly

    if sr != _TARGET_SAMPLE_RATE_HZ:
        audio = resample_poly(audio, _TARGET_SAMPLE_RATE_HZ, sr).astype(np.float32)
    audio = audio.astype(np.float32)
    target_len = int(_INPUT_WINDOW_SECONDS * _TARGET_SAMPLE_RATE_HZ)
    if audio.size < target_len:
        reps = int(np.ceil(target_len / audio.size))
        audio = np.tile(audio, reps)
    audio = audio[:target_len]

    sess = _session(model_path)
    name = sess.get_inputs()[0].name
    out = sess.run(None, {name: audio[np.newaxis, :]})
    raw = out[0][0]  # [sig_raw, bak_raw, ovr_raw]
    return _polyfit(float(raw[0]), float(raw[1]), float(raw[2]))


def score_batch(audio: np.ndarray, sr: int, model_path: Path) -> DnsmosScores:
    """Score by averaging across overlapping 9-second windows.

    Normalizes once, for the whole signal, so every window of one take
    keeps its relative level; normalizing per window would erase exactly
    the loudness differences the model is meant to hear.
    """
    audio = normalize(audio)
    target_len = int(_INPUT_WINDOW_SECONDS * sr)
    hop = target_len // 2
    if audio.size <= target_len:
        return score(audio, sr, model_path)
    sigs, baks, ovrs = [], [], []
    for start in range(0, audio.size - target_len + 1, hop):
        win = audio[start : start + target_len]
        s = score(win, sr, model_path)
        sigs.append(s.sig)
        baks.append(s.bak)
        ovrs.append(s.ovrl)
    return DnsmosScores(
        float(np.mean(sigs)), float(np.mean(baks)), float(np.mean(ovrs))
    )
