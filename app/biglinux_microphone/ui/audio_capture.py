"""Capture only the visible meter, without keeping the audio graph in use."""

from __future__ import annotations

import logging
import shutil
import subprocess
import threading
from collections.abc import Callable
from itertools import pairwise

from gi.repository import GLib

from biglinux_microphone import cli
from biglinux_microphone.ui.pcm_reader import PCMReader

logger = logging.getLogger(__name__)

RATE = 48000
CHUNK = 4096
BANDS = 30
METER_NODE = "biglinux-microphone.meter.spectrum"
_PROPERTIES = (
    "{ media.role=monitor media.category=Monitor "
    "application.id=br.com.biglinux.microphone "
    f'node.name={METER_NODE} node.description="Microphone activity" '
    "node.dont-fallback=true node.dont-reconnect=true node.dont-move=true }"
)


class SpectrumTap:
    """Coalesce frames and discard them when their source or destination changes."""

    def __init__(self, on_frame: Callable[[list[float], float], None]) -> None:
        self._on_frame = on_frame
        self._lock = threading.Lock()
        self._wake = threading.Event()
        self._thread: threading.Thread | None = None
        self._closed = False
        self._request: tuple[str, tuple[str, ...], bool] = ("", (), False)
        self._generation = 0
        self._pending = False
        self._frame: tuple[int, list[float], float] | None = None

    def start(self) -> None:
        if not self._closed and self._thread is None:
            self._thread = threading.Thread(
                target=self._run, name="bigmic-spectrum", daemon=False
            )
            self._thread.start()

    def configure(
        self,
        target: str = "",
        identity: tuple[str, ...] = (),
        *,
        detailed: bool = False,
    ) -> None:
        with self._lock:
            request = (target, identity, detailed)
            if self._closed or request == self._request:
                return
            self._request = request
            self._generation += 1
            self._frame = None
            self._wake.set()

    def stop(self) -> None:
        with self._lock:
            self._closed = True
            self._generation += 1
            self._frame = None
            self._wake.set()

    def _cancelled(self, generation: int) -> bool:
        with self._lock:
            return self._closed or generation != self._generation

    def _run(self) -> None:
        # Import numerical libraries off the GTK thread, and only on demand.
        np = None
        pw_cat = shutil.which("pw-cat")
        if pw_cat is None:
            return
        while True:
            with self._lock:
                if self._closed:
                    return
                target, _identity, detailed = self._request
                generation = self._generation
                self._wake.clear()
            if not target:
                self._wake.wait()
                continue
            if np is None:
                try:
                    import numpy as np
                except ImportError:
                    logger.warning("microphone meter requires numpy")
                    return
            self._capture(pw_cat, np, target, detailed, generation)
            self._publish(generation, [], 0.0)
            self._wake.wait(3)

    def _capture(
        self, pw_cat: str, np, target: str, detailed: bool, generation: int
    ) -> None:
        proc = None
        reader = None
        try:
            proc = subprocess.Popen(
                [
                    pw_cat,
                    "--record",
                    "--raw",
                    f"--rate={RATE}",
                    "--channels=1",
                    "--format=f32",
                    "--target",
                    target,
                    "-P",
                    _PROPERTIES,
                    "-",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                bufsize=0,
            )
            if detailed:
                edges = np.logspace(np.log10(50), np.log10(20000), BANDS + 1)
                indices = np.searchsorted(np.fft.rfftfreq(CHUNK, 1.0 / RATE), edges)
                band_slices = tuple(pairwise(indices))
                window = np.hanning(CHUNK)
                window_gain = 2.0 / window.sum()
            reader = PCMReader(proc.stdout, CHUNK * 4)
            cancelled = lambda: self._cancelled(generation)
            previous_peak = 0.0
            while not cancelled():
                raw = reader.read(cancelled)
                if raw is None:
                    break
                pcm = np.frombuffer(raw, dtype=np.float32)
                if not np.isfinite(pcm).all():
                    raise ValueError("non-finite microphone PCM")
                # OBS's sample-peak detector includes the preceding four
                # samples, so peaks crossing a block boundary are retained.
                magnitude = np.abs(pcm)
                sample_peak = max(float(magnitude.max()), previous_peak)
                previous_peak = float(magnitude[-4:].max())
                peak_db = 20.0 * np.log10(max(sample_peak, 1e-9))
                peak = max(0.0, min(1.0, (peak_db + 60.0) / 60.0))
                bands = []
                if detailed:
                    # Remove DC from the spectrum, but preserve waveform peak
                    # for headroom measurement, as OBS's sample-peak meter does.
                    pcm = pcm - pcm.mean()
                    amp = np.abs(np.fft.rfft(pcm * window)) * window_gain
                    for low, high in band_slices:
                        amplitude = float(amp[low:high].max()) if high > low else 0.0
                        db = 20.0 * np.log10(amplitude + 1e-9)
                        bands.append(max(0.0, min(1.0, (db + 75.0) / 55.0)))
                self._publish(generation, bands, peak)
        except (OSError, ValueError):
            if not self._cancelled(generation):
                logger.exception("microphone capture stopped")
        finally:
            if reader is not None:
                reader.close()
            if proc is not None:
                cli.terminate_process(proc)
                if proc.stdout is not None:
                    proc.stdout.close()

    def _publish(self, generation: int, bands: list[float], peak: float) -> None:
        with self._lock:
            if self._closed or generation != self._generation:
                return
            if self._frame is not None and self._frame[0] == generation:
                peak = max(peak, self._frame[2])
            self._frame = (generation, bands, peak)
            if self._pending:
                return
            self._pending = True
        GLib.idle_add(self._drain)

    def _drain(self) -> bool:
        with self._lock:
            frame, self._frame = self._frame, None
            self._pending = False
            valid = (
                frame is not None and not self._closed and frame[0] == self._generation
            )
        if valid:
            self._on_frame(frame[1], frame[2])
        return False
