"""Does the loudness meter agree with the standard it claims to implement?

BS.1770 defines its own calibration: a 997 Hz sine at -23 dBFS must read
-23 LUFS. Everything else here is downstream of that one number being right.
"""

from __future__ import annotations

import math
import unittest
from unittest import mock

import numpy as np

from biglinux_microphone.ui.loudness import SILENCE_LUFS, LoudnessMeter, k_weighting_db

RATE = 48000
CHUNK = 4096


def _spectrum(signal: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """The same analysis the meter's capture does: Hann, coherent-gain fixed."""
    window = np.hanning(len(signal))
    amplitude = np.abs(np.fft.rfft(signal * window)) * (2.0 / window.sum())
    return amplitude, np.fft.rfftfreq(len(signal), 1.0 / RATE)


def _sine(dbfs: float, hz: float = 997.0, n: int = CHUNK) -> np.ndarray:
    """A tone at `dbfs` RMS — which is what the standard's reference means.

    Getting this wrong reads three decibels low and looks like a bug in the
    meter: a sine's RMS is its peak over root two.
    """
    amplitude = (10.0 ** (dbfs / 20.0)) * math.sqrt(2.0)
    t = np.arange(n) / RATE
    return amplitude * np.sin(2.0 * np.pi * hz * t)


def _feed(meter: LoudnessMeter, signal: np.ndarray, seconds: float = 0.1) -> None:
    amplitude, freqs = _spectrum(signal)
    mean_square = float(np.mean(signal.astype(np.float64) ** 2))
    for _ in range(35):  # past the 3 s window
        meter.push_block(mean_square, amplitude, freqs, seconds)


class LoudnessContract(unittest.TestCase):
    def test_the_standards_reference_tone_reads_its_own_level(self) -> None:
        meter = LoudnessMeter(RATE)
        _feed(meter, _sine(-23.0))
        short_term = meter.window(3.0)
        self.assertIsNotNone(short_term)
        self.assertAlmostEqual(short_term, -23.0, delta=0.5)

    def test_ten_decibels_quieter_reads_ten_units_quieter(self) -> None:
        loud, quiet = LoudnessMeter(RATE), LoudnessMeter(RATE)
        _feed(loud, _sine(-23.0))
        _feed(quiet, _sine(-33.0))
        self.assertAlmostEqual(loud.window(3.0) - quiet.window(3.0), 10.0, delta=0.3)

    def test_silence_is_a_floor_rather_than_minus_infinity(self) -> None:
        meter = LoudnessMeter(RATE)
        _feed(meter, np.zeros(CHUNK))
        self.assertEqual(meter.window(3.0), SILENCE_LUFS)

    def test_a_window_answers_nothing_until_it_is_filled(self) -> None:
        meter = LoudnessMeter(RATE)
        amplitude, freqs = _spectrum(_sine(-23.0))
        meter.push_block(float(np.mean(_sine(-23.0) ** 2)), amplitude, freqs, 0.1)
        self.assertIsNone(meter.window(3.0), "three seconds cannot be known from one tenth")
        self.assertIsNone(meter.window(0.4))

    def test_the_weighting_is_the_curve_the_standard_describes(self) -> None:
        curve = k_weighting_db(np.array([10.0, 100.0, 1000.0, 4000.0, 10000.0]))
        # Rumble is discounted, the presence region is lifted about 4 dB, and
        # 1 kHz sits near unity — that shape is the whole point of the filter.
        self.assertLess(curve[0], -19.0)
        self.assertLess(curve[1], 0.5)
        # +0.69 dB at 1 kHz is not a rounding error: the standard's -0.691
        # offset exists precisely to cancel it, so a 1 kHz tone reads its own
        # level. A curve that is flat here would be the wrong curve.
        self.assertAlmostEqual(curve[2], 0.69, delta=0.1)
        self.assertGreater(curve[3], 3.0)
        self.assertGreater(curve[4], 3.0)


class LoudnessTimingRegression(unittest.TestCase):
    def setUp(self):
        self.amplitude, self.freqs = _spectrum(_sine(-23.0))
        self.mean_square = float(np.mean(_sine(-23.0) ** 2))

    def test_real_capture_requires_36_blocks_for_three_seconds(self):
        meter = LoudnessMeter(RATE)
        for _ in range(35):
            momentary, short = meter.push_block(
                self.mean_square, self.amplitude, self.freqs, CHUNK / RATE)
        self.assertIsNotNone(momentary)
        self.assertIsNone(short)  # 35 * 4096 / 48000 = 2.9867 s
        self.assertIsNotNone(meter.push_block(
            self.mean_square, self.amplitude, self.freqs, CHUNK / RATE)[1])

    def test_response_is_cached_for_the_same_frequency_array(self):
        with mock.patch("biglinux_microphone.ui.loudness.k_weighting_db",
                        wraps=k_weighting_db) as weighting:
            meter = LoudnessMeter(RATE)
            for _ in range(20):
                meter.push_block(self.mean_square, self.amplitude, self.freqs, CHUNK / RATE)
            self.assertEqual(weighting.call_count, 1)
            meter.push_block(self.mean_square, self.amplitude, self.freqs.copy(), CHUNK / RATE)
            self.assertEqual(weighting.call_count, 2)

    def test_partial_oldest_block_is_time_weighted(self):
        meter = LoudnessMeter(RATE)
        meter.push_block(self.mean_square, self.amplitude, self.freqs, .3)
        reference = meter.push_block(self.mean_square, self.amplitude, self.freqs, .1)[0]
        value = meter.push_block(self.mean_square / 10, self.amplitude, self.freqs, .1)[0]
        self.assertAlmostEqual(value - reference, 10 * math.log10(.775), places=8)

    def test_reset_discards_time_history(self):
        meter = LoudnessMeter(RATE)
        _feed(meter, _sine(-23.0))
        meter.reset()
        self.assertIsNone(meter.window(.4))

    def test_invalid_duration_or_energy_is_rejected(self):
        meter = LoudnessMeter(RATE)
        for duration in (0, -1, math.nan, math.inf):
            with self.assertRaises(ValueError):
                meter.push_block(self.mean_square, self.amplitude, self.freqs, duration)
        for energy in (-1, math.nan, math.inf):
            with self.assertRaises(ValueError):
                meter.push_block(energy, self.amplitude, self.freqs, .1)

    def test_long_stream_keeps_a_bounded_history(self):
        meter = LoudnessMeter(RATE)
        for _ in range(1000):
            meter.push_block(self.mean_square, self.amplitude, self.freqs, CHUNK / RATE)
        self.assertLessEqual(len(meter._blocks), 36)


if __name__ == "__main__":
    unittest.main()
