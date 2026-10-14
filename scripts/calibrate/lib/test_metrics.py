"""Unit tests for the gain-sensitive metrics added for review METRIC-02.

Numpy-only: they must not pull pesq/pystoi/onnxruntime so they run in any
checkout. Run with `python -m pytest lib/test_metrics.py` from scripts/calibrate,
or `python -m unittest lib.test_metrics`.
"""

from __future__ import annotations

import unittest

import numpy as np

from lib import metrics


def _sine(n: int, freq: float = 220.0, sr: int = 48000, amp: float = 0.5) -> np.ndarray:
    t = np.arange(n) / sr
    return (amp * np.sin(2 * np.pi * freq * t)).astype(np.float32)


class GainSensitiveMetrics(unittest.TestCase):
    def test_sdr_penalises_a_level_drop_that_si_sdr_ignores(self) -> None:
        ref = _sine(48000)
        half = (0.5 * ref).astype(np.float32)  # −6 dB, no distortion
        # SI-SDR rescales the drop away → near-perfect; plain SDR sees 4:1 → ~6 dB.
        self.assertGreater(metrics.si_sdr_db(ref, half), 50.0)
        self.assertAlmostEqual(metrics.sdr_db(ref, half), 6.02, delta=0.2)

    def test_clip_fraction_counts_full_scale_samples(self) -> None:
        x = np.array([0.0, 0.5, 1.0, -1.0, 0.2], dtype=np.float32)
        self.assertAlmostEqual(metrics.clip_fraction(x), 2 / 5)
        self.assertEqual(metrics.clip_fraction(0.5 * np.ones(10, dtype=np.float32)), 0.0)

    def test_speech_pause_levels_report_drop_and_residual(self) -> None:
        sr = 48000
        speech = _sine(sr, amp=0.5)  # 1 s of speech-like tone
        silence = np.zeros(sr, dtype=np.float32)  # 1 s pause
        ref = np.concatenate([speech, silence])
        # −6 dB on speech, a faint −60 dBFS residual left in the pause.
        proc = np.concatenate([0.5 * speech, 1e-3 * _sine(sr, freq=90.0, amp=1.0)]).astype(
            np.float32
        )
        gain, pause = metrics.speech_pause_levels(ref, proc, sr)
        self.assertAlmostEqual(gain, -6.02, delta=0.3)
        # A full-scale sine of amplitude 1e-3 has RMS 1e-3/√2 → −63 dBFS.
        self.assertAlmostEqual(pause, -63.0, delta=1.0)

    def test_score_pair_exposes_the_new_keys(self) -> None:
        try:
            import pyloudnorm  # noqa: F401
        except ImportError:
            self.skipTest("pyloudnorm not installed; score_pair needs the full metric stack")
        sr = 48000
        ref = np.concatenate([_sine(sr), np.zeros(sr, dtype=np.float32)])
        proc = (0.5 * ref).astype(np.float32)
        out = metrics.score_pair(ref, proc, sr)
        for key in ("clip_pct", "sdr_db", "level_delta_db", "active_speech_gain_db", "pause_residual_db"):
            self.assertIn(key, out)
        self.assertAlmostEqual(out["level_delta_db"], -6.02, delta=0.2)


if __name__ == "__main__":
    unittest.main()
