"""Calibration library: audio loading, chain emulator, metrics, DNSMOS wrapper.

Reproducible offline scoring of the BigLinux noise-reduction filter
chain. The Python emulator is exact for the biquad/EQ stages (same
math as the PipeWire builtins) and calls the GTCRN ONNX model
directly so denoiser scoring matches the live plugin.
"""

import os
from pathlib import Path

from . import chain, dnsmos, metrics, signals

__all__ = ["cache_root", "chain", "dnsmos", "metrics", "signals"]


def cache_root() -> Path:
    """Where `setup.sh` puts datasets and models.

    Every script needs this and each carried its own copy, all of which had
    to be edited together for the last rename. One of them read `~/.cache`
    unconditionally, so with `XDG_CACHE_HOME` set it reported the dataset
    missing right after `setup.sh` had downloaded it.
    """
    return (
        Path(os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache")))
        / "biglinux-microphone/calibration"
    )
