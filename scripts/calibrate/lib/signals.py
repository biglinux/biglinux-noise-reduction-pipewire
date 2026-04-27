"""Test-signal generation for HPF / EQ / compressor calibration."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import numpy as np
import soundfile as sf


@dataclass(frozen=True)
class SignalSpec:
    """Description of a synthetic test signal."""

    name: str
    sample_rate: int
    duration_s: float


def sine(spec: SignalSpec, freq_hz: float, amplitude: float = 0.5) -> np.ndarray:
    """Pure sine — used to validate compressor threshold/makeup."""
    t = np.arange(int(spec.sample_rate * spec.duration_s)) / spec.sample_rate
    return (amplitude * np.sin(2 * np.pi * freq_hz * t)).astype(np.float32)


def log_sweep(
    spec: SignalSpec,
    f0_hz: float = 20.0,
    f1_hz: float = 20000.0,
    amplitude: float = 0.5,
) -> np.ndarray:
    """Exponential frequency sweep — measures HPF/EQ magnitude response."""
    t = np.arange(int(spec.sample_rate * spec.duration_s)) / spec.sample_rate
    if f0_hz <= 0 or f1_hz <= f0_hz:
        raise ValueError("sweep needs 0 < f0 < f1")
    k = np.log(f1_hz / f0_hz) / spec.duration_s
    phase = 2 * np.pi * f0_hz * (np.expm1(k * t) / k)
    return (amplitude * np.sin(phase)).astype(np.float32)


def white_noise(spec: SignalSpec, amplitude: float = 0.1, seed: int = 0) -> np.ndarray:
    """Gaussian white noise — broadband floor for SNR mixing."""
    rng = np.random.default_rng(seed)
    n = int(spec.sample_rate * spec.duration_s)
    return (amplitude * rng.standard_normal(n)).astype(np.float32)


def pink_noise(spec: SignalSpec, amplitude: float = 0.1, seed: int = 0) -> np.ndarray:
    """1/f-shaped noise — closer to real-world ambient than white noise."""
    rng = np.random.default_rng(seed)
    n = int(spec.sample_rate * spec.duration_s)
    # Voss-McCartney: cumulative sum of white noise, deconvolved with
    # a first-order pole. Cheap, accurate enough for calibration.
    white = rng.standard_normal(n).astype(np.float32)
    pink = np.empty_like(white)
    acc = 0.0
    for i, w in enumerate(white):
        acc = 0.99 * acc + 0.05 * w
        pink[i] = acc
    pink /= np.max(np.abs(pink)) + 1e-9
    return amplitude * pink


def mix_at_snr(speech: np.ndarray, noise: np.ndarray, snr_db: float) -> np.ndarray:
    """Mix `speech` with `noise` at the requested SNR. Returns clipped float32.

    Uses RMS energy of both signals. Trims/loops `noise` to match
    speech length so SNR is stationary across the file.
    """
    speech = speech.astype(np.float32)
    if noise.size < speech.size:
        reps = int(np.ceil(speech.size / noise.size))
        noise = np.tile(noise, reps)
    noise = noise[: speech.size].astype(np.float32)

    rms_s = float(np.sqrt(np.mean(speech**2)) + 1e-12)
    rms_n = float(np.sqrt(np.mean(noise**2)) + 1e-12)
    target_n = rms_s * (10.0 ** (-snr_db / 20.0))
    noise = noise * (target_n / rms_n)
    mixed = speech + noise
    peak = float(np.max(np.abs(mixed)) + 1e-12)
    if peak > 0.99:
        mixed = mixed * (0.99 / peak)
    return mixed.astype(np.float32)


def write_wav(path: Path, x: np.ndarray, sr: int) -> None:
    """Write float32 PCM, mono. Creates parent dirs as needed."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    sf.write(str(path), x, sr, subtype="FLOAT")


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    """Read first channel only, return float32 + sample rate."""
    x, sr = sf.read(str(path), always_2d=False)
    if x.ndim > 1:
        x = x[:, 0]
    return x.astype(np.float32), int(sr)
