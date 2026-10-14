"""Audio loading and canonical test-signal generation for calibration."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import numpy as np
import soundfile as sf


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    """Read first channel only, return float32 + sample rate."""
    x, sr = sf.read(str(path), always_2d=False)
    if x.ndim > 1:
        x = x[:, 0]
    return x.astype(np.float32), int(sr)


def write_wav(path: Path, x: np.ndarray, sr: int) -> None:
    """Write a mono float32 PCM WAV."""
    sf.write(str(path), np.asarray(x, dtype=np.float32), int(sr), subtype="FLOAT")


@dataclass(frozen=True)
class SignalSpec:
    """Geometry shared by every generated test signal."""

    name: str
    sample_rate: int
    duration_s: float

    @property
    def n_samples(self) -> int:
        return round(self.sample_rate * self.duration_s)

    def _t(self) -> np.ndarray:
        return np.arange(self.n_samples, dtype=np.float64) / self.sample_rate


def sine(spec: SignalSpec, freq_hz: float, amp: float) -> np.ndarray:
    """Constant-frequency sine at peak amplitude `amp`."""
    x = amp * np.sin(2.0 * np.pi * freq_hz * spec._t())
    return x.astype(np.float32)


def log_sweep(spec: SignalSpec, f0_hz: float, f1_hz: float, amp: float) -> np.ndarray:
    """Exponential (log) frequency sweep from `f0_hz` to `f1_hz`, peak `amp`."""
    if f0_hz <= 0.0 or f1_hz <= 0.0:
        raise ValueError("sweep endpoints must be positive")
    duration = max(spec.duration_s, 1.0 / spec.sample_rate)
    k = np.log(f1_hz / f0_hz)
    t = spec._t()
    phase = 2.0 * np.pi * f0_hz * duration / k * (np.exp(t / duration * k) - 1.0)
    return (amp * np.sin(phase)).astype(np.float32)


def white_noise(spec: SignalSpec, amp: float, *, seed: int | None = None) -> np.ndarray:
    """Flat-spectrum noise bounded to +/-`amp`."""
    rng = np.random.default_rng(seed)
    return rng.uniform(-amp, amp, spec.n_samples).astype(np.float32)


def pink_noise(spec: SignalSpec, amp: float, *, seed: int | None = None) -> np.ndarray:
    """1/f noise, peak-normalised to `amp` (FFT shaping of white noise)."""
    n = spec.n_samples
    if n == 0:
        return np.zeros(0, dtype=np.float32)
    rng = np.random.default_rng(seed)
    spectrum = np.fft.rfft(rng.standard_normal(n))
    freqs = np.arange(spectrum.size, dtype=np.float64)
    freqs[0] = 1.0  # leave DC untouched instead of dividing by zero
    x = np.fft.irfft(spectrum / np.sqrt(freqs), n)
    peak = float(np.max(np.abs(x)))
    if peak > 0.0:
        x = x / peak * amp
    return x.astype(np.float32)
