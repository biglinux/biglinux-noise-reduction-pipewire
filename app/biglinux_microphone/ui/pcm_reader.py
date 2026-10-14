"""Bounded, cancellable reads from a PCM pipe, independent of GTK/numpy."""

from __future__ import annotations

import os
import selectors
import time
from collections.abc import Callable
from typing import BinaryIO


class PCMReader:
    """One selector per stream; tolerate short reads without retaining a backlog."""

    def __init__(
        self, stream: BinaryIO, frame_bytes: int, timeout: float = 3.0
    ) -> None:
        if frame_bytes <= 0 or not (0 < timeout <= 60):
            raise ValueError("invalid PCM frame size or timeout")
        self._fd = stream.fileno()
        self._frame_bytes = frame_bytes
        self._timeout = timeout
        self._selector = selectors.DefaultSelector()
        try:
            self._selector.register(self._fd, selectors.EVENT_READ)
        except Exception:
            self._selector.close()
            raise

    def read(self, cancelled: Callable[[], bool]) -> bytes | None:
        """None on cancellation/EOF; TimeoutError on an alive but stalled writer."""
        deadline = time.monotonic() + self._timeout
        block = bytearray()
        while len(block) < self._frame_bytes:
            if cancelled():
                return None
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("microphone PCM stalled")
            if not self._selector.select(min(0.1, remaining)):
                continue
            try:
                data = os.read(self._fd, self._frame_bytes - len(block))
            except InterruptedError:
                continue
            if not data:
                return None
            block.extend(data)
        return bytes(block)

    def close(self) -> None:
        self._selector.close()

    def __enter__(self) -> PCMReader:
        return self

    def __exit__(self, *_args: object) -> None:
        self.close()
