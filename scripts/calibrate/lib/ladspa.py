"""Drive a LADSPA plugin directly, one fixed block at a time.

ffmpeg's `ladspa` filter is convenient but it is also a second buffering
scheme between the measurement and the thing being measured: it picks its
own block size, ignores `nb_samples` in some builds, and adds its own
delay. Every latency figure taken through it is the plugin plus ffmpeg,
and the two cannot be separated after the fact.

This calls the plugin the way PipeWire does — instantiate at a rate,
activate, then `run()` with exactly `quantum` frames, over and over — so
the numbers describe the plugin alone at the block size we actually ship.
"""

from __future__ import annotations

import ctypes
from dataclasses import dataclass
from pathlib import Path

import numpy as np

# LADSPA port descriptor bits, from ladspa.h.
PORT_INPUT = 0x1
PORT_OUTPUT = 0x2
PORT_CONTROL = 0x4
PORT_AUDIO = 0x8

# Hint bits, enough to pick a sane default for a control we are not setting.
HINT_DEFAULT_MASK = 0x3C0
HINT_DEFAULT_MINIMUM = 0x40
HINT_DEFAULT_LOW = 0x80
HINT_DEFAULT_MIDDLE = 0xC0
HINT_DEFAULT_HIGH = 0x100
HINT_DEFAULT_MAXIMUM = 0x140
HINT_DEFAULT_0 = 0x200
HINT_DEFAULT_1 = 0x240
HINT_DEFAULT_100 = 0x280
HINT_DEFAULT_440 = 0x2C0
HINT_SAMPLE_RATE = 0x8


class PortRangeHint(ctypes.Structure):
    _fields_ = [
        ("HintDescriptor", ctypes.c_int),
        ("LowerBound", ctypes.c_float),
        ("UpperBound", ctypes.c_float),
    ]


class Descriptor(ctypes.Structure):
    _fields_ = [
        ("UniqueID", ctypes.c_ulong),
        ("Label", ctypes.c_char_p),
        ("Properties", ctypes.c_int),
        ("Name", ctypes.c_char_p),
        ("Maker", ctypes.c_char_p),
        ("Copyright", ctypes.c_char_p),
        ("PortCount", ctypes.c_ulong),
        ("PortDescriptors", ctypes.POINTER(ctypes.c_int)),
        ("PortNames", ctypes.POINTER(ctypes.c_char_p)),
        ("PortRangeHints", ctypes.POINTER(PortRangeHint)),
        ("ImplementationData", ctypes.c_void_p),
        ("instantiate", ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong)),
        (
            "connect_port",
            ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_float)),
        ),
        ("activate", ctypes.CFUNCTYPE(None, ctypes.c_void_p)),
        ("run", ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_ulong)),
        ("run_adding", ctypes.c_void_p),
        ("set_run_adding_gain", ctypes.c_void_p),
        ("deactivate", ctypes.CFUNCTYPE(None, ctypes.c_void_p)),
        ("cleanup", ctypes.CFUNCTYPE(None, ctypes.c_void_p)),
    ]


def _default(hint: PortRangeHint, rate: int) -> float:
    """The value a host would use for a control nobody set."""
    which = hint.HintDescriptor & HINT_DEFAULT_MASK
    scale = rate if hint.HintDescriptor & HINT_SAMPLE_RATE else 1
    low = hint.LowerBound * scale
    high = hint.UpperBound * scale
    return {
        HINT_DEFAULT_MINIMUM: low,
        HINT_DEFAULT_LOW: low * 0.75 + high * 0.25,
        HINT_DEFAULT_MIDDLE: (low + high) / 2.0,
        HINT_DEFAULT_HIGH: low * 0.25 + high * 0.75,
        HINT_DEFAULT_MAXIMUM: high,
        HINT_DEFAULT_0: 0.0,
        HINT_DEFAULT_1: 1.0,
        HINT_DEFAULT_100: 100.0,
        HINT_DEFAULT_440: 440.0,
    }.get(which, 0.0)


@dataclass
class Plugin:
    """One instantiated plugin, ready to be driven block by block."""

    library: ctypes.CDLL
    descriptor: Descriptor
    handle: ctypes.c_void_p
    rate: int
    quantum: int
    _in: np.ndarray
    _out: np.ndarray
    _controls: np.ndarray

    def set_control(self, index: int, value: float) -> None:
        """Write one control port, by its index among the control ports."""
        seen = -1
        for port in range(self.descriptor.PortCount):
            kind = self.descriptor.PortDescriptors[port]
            if kind & PORT_CONTROL and kind & PORT_INPUT:
                seen += 1
                if seen == index:
                    self._controls[port] = value
                    return
        raise IndexError(f"no control port {index}")

    def process(self, audio: np.ndarray) -> np.ndarray:
        """Push `audio` through in whole blocks and return what came back.

        Trailing samples that do not fill a block are dropped, so the
        output is always a whole number of blocks.
        """
        blocks = audio.size // self.quantum
        out = np.empty(blocks * self.quantum, dtype=np.float32)
        for b in range(blocks):
            start = b * self.quantum
            self._in[:] = audio[start : start + self.quantum]
            self.descriptor.run(self.handle, self.quantum)
            out[start : start + self.quantum] = self._out
        return out

    def close(self) -> None:
        if self.handle:
            if self.descriptor.deactivate:
                self.descriptor.deactivate(self.handle)
            self.descriptor.cleanup(self.handle)
            self.handle = None


def open_plugin(path: Path, label: str, rate: int, quantum: int) -> Plugin:
    """Load `label` from `path` and instantiate it at `rate`."""
    library = ctypes.CDLL(str(path), mode=ctypes.RTLD_LOCAL)
    library.ladspa_descriptor.restype = ctypes.POINTER(Descriptor)
    library.ladspa_descriptor.argtypes = [ctypes.c_ulong]

    index = 0
    while True:
        found = library.ladspa_descriptor(index)
        # Some plugins end the list with a descriptor whose fields are zeroed
        # rather than with a null pointer, so both count as the end.
        if not found or not found.contents.Label:
            raise LookupError(f"{label} not in {path}")
        if found.contents.Label.decode() == label:
            descriptor = found.contents
            break
        index += 1

    handle = descriptor.instantiate(ctypes.byref(descriptor), rate)
    if not handle:
        raise RuntimeError(f"{label} refused to instantiate at {rate} Hz")

    buffers_in = np.zeros(quantum, dtype=np.float32)
    buffers_out = np.zeros(quantum, dtype=np.float32)
    controls = np.zeros(descriptor.PortCount, dtype=np.float32)
    float_p = ctypes.POINTER(ctypes.c_float)

    for port in range(descriptor.PortCount):
        kind = descriptor.PortDescriptors[port]
        if kind & PORT_AUDIO:
            buffer = buffers_in if kind & PORT_INPUT else buffers_out
            descriptor.connect_port(handle, port, buffer.ctypes.data_as(float_p))
        else:
            controls[port] = _default(descriptor.PortRangeHints[port], rate)
            descriptor.connect_port(
                handle, port, controls[port : port + 1].ctypes.data_as(float_p)
            )

    if descriptor.activate:
        descriptor.activate(handle)

    return Plugin(
        library, descriptor, handle, rate, quantum, buffers_in, buffers_out, controls
    )
