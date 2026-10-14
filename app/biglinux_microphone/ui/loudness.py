"""Loudness, the way broadcast and streaming platforms measure it.

A peak meter answers "am I about to clip". It does not answer "am I as loud as
everybody else in this call": a voice with sharp consonants peaks high and sits
quiet, and a compressed one does the opposite. That second question is what
LUFS answers, and it is the number a streamer is given a target for.

This is an approximation using the ITU-R BS.1770-4 weighting — a high shelf standing in for the shadow
of a head, then a gentle high-pass discounting what the ear barely weighs —
applied to the spectrum the meter already computes. It is not a conformant
BS.1770 sample-domain implementation or a certified loudness meter. The standard describes two
biquads over samples; running those in Python, per sample, would cost more CPU
than the whole spectrum does. Multiplying the analysis bins by the same filters'
magnitude response costs one array multiply and lands within a few tenths of a
decibel of the standard's own reference tone, which is checked in the tests.

Integrated loudness is left out. It is defined over a
programme with a beginning and an end; a number integrating since the window
opened stops meaning anything within minutes. A measured span needs a start and
a stop somebody pressed, and that belongs to the A/B capture, not to a meter.
"""

from __future__ import annotations

import math
from collections import deque

#: Momentary and short-term, as the standard names them.
MOMENTARY_S = 0.400
SHORT_TERM_S = 3.0
#: Everything quieter than this is a floor, not a loudness. The standard's own
#: absolute gate sits here.
SILENCE_LUFS = -70.0
#: The standard's calibration constant, so a 997 Hz tone at -23 dBFS reads -23.
_OFFSET_DB = -0.691


def _shelf(rate: int) -> tuple[list[float], list[float]]:
    """Stage 1: the high shelf standing in for a head in the way. +4 dB up top."""
    db, f0, q = 3.999_843_853_973_347, 1_681.974_450_955_533, 0.707_175_228_004_951_8
    k = math.tan(math.pi * f0 / rate)
    vh = 10.0 ** (db / 20.0)
    vb = vh**0.499_666_774_155_600_1
    denominator = 1.0 + k / q + k * k
    b = [
        (vh + vb * k / q + k * k) / denominator,
        2.0 * (k * k - vh) / denominator,
        (vh - vb * k / q + k * k) / denominator,
    ]
    a = [1.0, 2.0 * (k * k - 1.0) / denominator, (1.0 - k / q + k * k) / denominator]
    return b, a


def _highpass(rate: int) -> tuple[list[float], list[float]]:
    """Stage 2: the RLB curve, a gentle high-pass near 38 Hz."""
    f0, q = 38.135_470_876_002_35, 0.500_327_040_690_190_5
    k = math.tan(math.pi * f0 / rate)
    denominator = 1.0 + k / q + k * k
    a = [1.0, 2.0 * (k * k - 1.0) / denominator, (1.0 - k / q + k * k) / denominator]
    return [1.0, -2.0, 1.0], a


def k_weighting_db(freqs, rate: int = 48000):
    """The BS.1770 weighting, in dB, at each frequency.

    The response of the standard's own two biquads, evaluated on the unit
    circle. An analogue prototype was tried first and produced a shelf the
    wrong way up — +4 dB in the bass rather than the treble — which the shape
    test caught; reading the real coefficients removes the chance of guessing
    that algebra wrong a second time.
    """
    import numpy as np

    omega = 2.0 * np.pi * np.asarray(freqs, dtype=np.float64) / rate
    z = np.exp(-1j * omega)

    def response(coefficients):
        b, a = coefficients
        numerator = b[0] + b[1] * z + b[2] * z * z
        denominator = a[0] + a[1] * z + a[2] * z * z
        return np.abs(numerator) / np.maximum(np.abs(denominator), 1e-30)

    magnitude = response(_shelf(rate)) * response(_highpass(rate))
    return 20.0 * np.log10(np.maximum(magnitude, 1e-12))


class LoudnessMeter:
    """Feed it one analysis block's spectrum; ask how loud that was."""

    def __init__(self, rate: int = 48000, block_s: float = 0.1) -> None:
        import numpy as np

        self._np = np
        self._rate = rate
        if rate <= 0 or not math.isfinite(block_s) or block_s <= 0.0:
            raise ValueError("rate and block duration must be positive")
        self._weights = None
        # Keep the actual object alive: comparing `id()` integers with `is`
        # recalculates the response every frame and permits recycled identities.
        self._freqs = None
        self._blocks: deque[tuple[float, float]] = deque(maxlen=4096)

    def push_block(self, mean_square: float, amplitudes, freqs, seconds: float):
        """Add weighted energy and the *actual* duration of this audio block.

        The spectral ratio approximates K weighting; the input mean square is
        measured from samples. Durations are never rounded to 100 ms: a 4096
        sample block at 48 kHz represents 85.333 ms, not 100 ms.
        """
        np = self._np
        if not math.isfinite(seconds) or seconds <= 0.0:
            raise ValueError("block duration must be finite and positive")
        if not math.isfinite(mean_square) or mean_square < 0.0:
            raise ValueError("mean square must be finite and nonnegative")
        amplitudes = np.asarray(amplitudes, dtype=np.float64)
        if self._weights is None or self._freqs is not freqs:
            self._weights = 10.0 ** (k_weighting_db(freqs, self._rate) / 10.0)
            self._freqs = freqs
        if amplitudes.ndim != 1 or amplitudes.shape != self._weights.shape:
            raise ValueError("spectrum and frequency bins must have matching shapes")
        power = amplitudes * amplitudes
        energy = float(np.sum(power))
        weighted_energy = float(np.dot(power, self._weights))
        if not math.isfinite(energy) or not math.isfinite(weighted_energy):
            raise ValueError("spectrum energy must be finite")
        ratio = weighted_energy / energy if energy > 0.0 else 1.0
        weighted_square = mean_square * ratio
        if not math.isfinite(weighted_square):
            raise ValueError("weighted mean square must be finite")
        self._blocks.append((min(seconds, SHORT_TERM_S), weighted_square))
        duration = sum(block[0] for block in self._blocks)
        while self._blocks and duration - self._blocks[0][0] >= SHORT_TERM_S:
            duration -= self._blocks.popleft()[0]
        return self.window(MOMENTARY_S), self.window(SHORT_TERM_S)

    def window(self, seconds: float) -> float | None:
        """Integrate the latest exact time window, including partial blocks."""
        if not math.isfinite(seconds) or not 0.0 < seconds <= SHORT_TERM_S:
            raise ValueError("window must be positive and at most three seconds")
        remaining, energy = seconds, 0.0
        for duration, mean_square in reversed(self._blocks):
            overlap = min(duration, remaining)
            energy += overlap * mean_square
            remaining -= overlap
            if remaining <= 1e-9:
                mean_square = energy / seconds
                if mean_square <= 0.0:
                    return SILENCE_LUFS
                return max(SILENCE_LUFS, _OFFSET_DB + 10.0 * math.log10(mean_square))
        return None

    def reset(self) -> None:
        self._blocks.clear()
