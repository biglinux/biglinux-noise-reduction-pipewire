"""
Audio processing package for BigLinux Microphone Settings.

Contains modules for PipeWire filter-chain generation and audio processing.
"""

from biglinux_microphone.audio.filter_chain import (
    FilterChainConfig,
    FilterChainGenerator,
)

__all__ = ["FilterChainConfig", "FilterChainGenerator"]
