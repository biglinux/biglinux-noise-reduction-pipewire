"""Calibration library: signals, chain emulator, metrics, DNSMOS wrapper.

Reproducible offline scoring of the BigLinux noise-reduction filter
chain. The Python emulator is exact for the biquad/EQ stages (same
math as the PipeWire builtins) and calls the GTCRN ONNX model
directly so denoiser scoring matches the live plugin.
"""

from . import chain, dnsmos, metrics, signals

__all__ = ["chain", "dnsmos", "metrics", "signals"]
