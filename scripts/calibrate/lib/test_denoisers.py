"""Unit tests for the OLA framing pad added for review MODEL-03.

Numpy-only: exercises the padding arithmetic in isolation so it runs without
onnxruntime/openvino or any model file.
"""

from __future__ import annotations

import unittest

import numpy as np

from lib.denoisers import _pad_to_full_frames


class PadToFullFrames(unittest.TestCase):
    def test_short_clip_is_padded_up_to_one_full_frame(self) -> None:
        nfft, hop = 512, 256
        x = np.ones(100, dtype=np.float32)  # shorter than nfft: used to crash
        padded = _pad_to_full_frames(x, nfft, hop)
        self.assertEqual(len(padded), nfft)
        self.assertEqual(padded.dtype, np.float32)
        # A single full frame is now valid: (len - nfft) is a multiple of hop.
        self.assertEqual((len(padded) - nfft) % hop, 0)

    def test_exact_frame_length_is_unchanged(self) -> None:
        nfft, hop = 512, 256
        x = np.ones(nfft, dtype=np.float32)
        self.assertEqual(len(_pad_to_full_frames(x, nfft, hop)), nfft)

    def test_partial_tail_is_padded_so_it_is_processed(self) -> None:
        nfft, hop = 512, 256
        x = np.ones(nfft + hop + 5, dtype=np.float32)  # 5-sample tail past 2 frames
        padded = _pad_to_full_frames(x, nfft, hop)
        self.assertGreaterEqual(len(padded), len(x))
        self.assertEqual((len(padded) - nfft) % hop, 0)
        # The real samples survive; only zeros were appended.
        self.assertTrue(np.array_equal(padded[: len(x)], x))
        self.assertTrue(np.all(padded[len(x) :] == 0.0))

    def test_frame_count_covers_every_sample(self) -> None:
        nfft, hop = 320, 160
        for length in (1, 159, 160, 321, 1000):
            padded = _pad_to_full_frames(np.ones(length, dtype=np.float32), nfft, hop)
            n_frames = 1 + (len(padded) - nfft) // hop
            last_covered = (n_frames - 1) * hop + nfft
            self.assertGreaterEqual(last_covered, length, f"tail dropped for len={length}")


if __name__ == "__main__":
    unittest.main()
