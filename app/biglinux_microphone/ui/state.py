"""Shared backend-derived state for every UI page.

Controls request writes through this store. A successful write is followed by a
backend read-back, and pages render that snapshot rather than assuming a click
was applied. This keeps rendering side-effect free and prevents pages from
disagreeing about effective state.
"""

from __future__ import annotations

import logging
import math
from collections.abc import Callable
from dataclasses import dataclass, field, replace
from typing import Any

from biglinux_microphone import cli
from biglinux_microphone.ui.backend import BackendEvents, BackendRunner
from biglinux_microphone.utils.async_utils import Debouncer

logger = logging.getLogger(__name__)

#: How long several writes in a row are allowed to coalesce into one read-back.
#: A slider being dragged produces a write per debounce tick; re-reading the
#: settings file after each one would put the file read in the drag's way.
REREAD_MS = 300

#: The three denoisers: the number the backend stores, the word ``set model``
#: takes and the name people see.
MODELS = {11: ("dfn3", "DFN3"), 12: ("dfn3-ll", "DFN3-LL"), 13: ("dpdfnet", "DPDFNet")}
DEFAULT_MODEL = 11


def calibration_window(entry: dict[str, Any]) -> tuple[float, float] | None:
    """Return a valid user-visible calibrated level range, if one exists.

    Bounds that round to the same percentage are treated as a single level.
    """
    low, high = entry.get("min_volume"), entry.get("max_volume")
    if not (isinstance(low, (int, float)) and isinstance(high, (int, float))):
        return None
    if isinstance(low, bool) or isinstance(high, bool):
        return None
    low, high = float(low), float(high)
    if not (math.isfinite(low) and math.isfinite(high) and 0 <= low <= high <= 3):
        return None
    return (low, high) if round(low * 100) < round(high * 100) else None


@dataclass(frozen=True)
class Snapshot:
    """Everything the window knows, as of one reading. Never mutated in place."""

    settings: dict[str, Any] = field(default_factory=dict)
    sources: list[dict[str, Any]] = field(default_factory=list)
    current_source: str = ""
    volume: int | None = None
    runtime: dict[str, Any] = field(default_factory=dict)
    muted: bool | None = None
    #: Present once something has failed and not yet been superseded by a good
    #: reading. Pages show it; they do not clear it.
    error: str = ""
    #: False until the first successful load. A page must be able to tell
    #: "nothing here yet" from "nothing here".
    loaded: bool = False

    # ── derived readings, so pages do not each re-dig the same dictionary ──

    @property
    def noise_reduction(self) -> dict[str, Any]:
        section = self.settings.get("noise_reduction")
        return section if isinstance(section, dict) else {}

    @property
    def filter_on(self) -> bool:
        """Whether the person requested microphone denoising.

        Both the master bypass and denoiser-specific setting must allow it.
        """
        return not self.settings.get("mic_bypass", False) and bool(
            self.noise_reduction.get("enabled")
        )

    @property
    def echo_mode(self) -> str:
        section = self.settings.get("echo_cancel")
        mode = section.get("mode") if isinstance(section, dict) else None
        return mode if mode in ("auto", "on", "off") else "auto"

    @property
    def equalizer(self) -> dict[str, Any]:
        section = self.settings.get("equalizer")
        return section if isinstance(section, dict) else {}

    @property
    def model(self) -> int | None:
        model = self.runtime.get("effective_model")
        return model if type(model) is int else None

    @property
    def capture_target(self) -> str:
        """Never fall back to raw audio when a requested chain is unavailable."""
        if not self.runtime.get("audio_available") or not self.current_source:
            return ""
        if not any(
            s.get("name") == self.current_source and s.get("available", True)
            for s in self.sources
        ):
            return ""
        if self.runtime.get("mic_running"):
            if self.runtime.get("denoising") != self.filter_on:
                return ""
            return "mic-biglinux"
        return "" if self.runtime.get("mic_enabled") else self.current_source

    @property
    def capture_identity(self) -> tuple[str, ...]:
        if not self.capture_target:
            return ()
        source = next((s for s in self.sources if s["name"] == self.current_source), {})
        return (
            self.current_source,
            str(source.get("serial") or ""),
            str(self.runtime.get("mic_serial") or "")
            if self.capture_target == "mic-biglinux"
            else "",
        )

    @property
    def ready(self) -> bool:
        return bool(self.loaded and not self.error and self.capture_target)

    @property
    def chosen_model(self) -> int:
        """The model the person picked, which may not be running yet."""
        model = self.noise_reduction.get("model")
        return model if type(model) is int and model in MODELS else DEFAULT_MODEL

    @property
    def voice_gate(self) -> bool:
        """Whether the chosen model runs the voice gate; DPDFNet keeps its own
        choice, off until asked for."""
        if MODELS[self.chosen_model][0] == "dpdfnet":
            return self.noise_reduction.get("dpdfnet_voice_gate", False) is True
        return self.noise_reduction.get("voice_gate", True) is True

    @property
    def voice_sensitivity(self) -> int:
        """The voice gate's sensitivity level, 1 (sensitive) to 5 (strict)."""
        level = self.noise_reduction.get("voice_sensitivity")
        return level if type(level) is int and 1 <= level <= 5 else 3

    @property
    def manual_tuning(self) -> bool:
        """Whether a command-line DFN3 tuning overrides the recommended values."""
        nr = self.noise_reduction
        return (
            nr.get("min_snr_override_db") is not None
            or nr.get("post_filter_beta", 0) != 0
            or nr.get("startup_mute_ms", 1000) != 1000
        )

    @property
    def capabilities(self) -> dict[str, Any]:
        """Optional UI feature flags supplied by newer backends.

        Old backends do not expose the section; absence reads
        as false rather than guessing that a feature exists.
        """
        value = self.settings.get("capabilities")
        return dict(value) if isinstance(value, dict) else {}

    @property
    def model_family(self) -> str:
        """Family of the running model, for its conditional advanced controls."""
        return {11: "dfn3", 12: "dfn3", 13: "dpdfnet"}.get(self.model, "unknown")

    @property
    def model_label(self) -> str:
        return MODELS[self.model][1] if self.model in MODELS else ""

    def calibration(self, device: str = "") -> dict[str, Any]:
        """What a calibration pass left for one device, or for the current one."""
        section = self.settings.get("mic_calibration")
        devices = section.get("devices") if isinstance(section, dict) else None
        if not isinstance(devices, dict):
            return {}
        entry = devices.get(device or self.current_source)
        return entry if isinstance(entry, dict) else {}


class AudioState:
    """The store: reads the backend, writes the backend, tells everybody."""

    def __init__(self, runner: BackendRunner) -> None:
        self._runner = runner
        # Recording/calibration must not hold up hotplug and state reads.
        self._tasks = BackendRunner(on_error=self._write_failed)
        self._events: BackendEvents | None = None
        self._subscribers: dict[int, Callable[[Snapshot], None]] = {}
        self._next_token = 1
        self._debouncers: dict[str, Debouncer] = {}
        self._reread_pending = False
        self._reading = False
        self._reload_again = False
        self._closed = False
        self._writing: set[str] = set()
        self._queued_values: dict[str, str] = {}
        #: Why the last action failed, kept until an action succeeds. A reading
        #: must not clear it: the re-read that follows a failed write would
        #: otherwise erase the only explanation the person was going to get.
        self._action_error = ""
        self.snapshot = Snapshot()
        #: True while a broadcast is running. Handlers check it and stand down,
        #: so a widget being updated to match the truth never reports that as a
        #: change the person made.
        self.syncing = False

    # ── subscription ──────────────────────────────────────────────────────

    def watch(self) -> None:
        if not self._closed and self._events is None:
            self._events = BackendEvents(self.reload)

    def subscribe(self, callback: Callable[[Snapshot], None]) -> int:
        token = self._next_token
        self._next_token += 1
        self._subscribers[token] = callback
        return token

    def _broadcast(self) -> None:
        self.syncing = True
        try:
            for callback in list(self._subscribers.values()):
                try:
                    callback(self.snapshot)
                except Exception:  # one page's bug must not blank the others
                    logger.exception("a page failed to render the snapshot")
        finally:
            self.syncing = False

    # ── reading ───────────────────────────────────────────────────────────

    def reload(self) -> None:
        """Read everything the window shows, then tell every page."""
        if self._closed:
            return
        if self._reading:
            self._reload_again = True
            return
        self._reading = True
        self._runner.call(self._read, self._apply_read, self._read_failed)

    @staticmethod
    def _read() -> Snapshot:
        status = dict(cli.load_status())
        settings = status.pop("settings")
        sources = cli.list_sources()
        current = cli.get_default_source()
        names = {source["name"] for source in sources}
        if current not in names:
            current = ""  # Never present another device as the missing default.
        source = next((source for source in sources if source["name"] == current), {})
        return Snapshot(
            settings=settings,
            sources=sources,
            current_source=current,
            volume=source.get("volume"),
            muted=source.get("muted"),
            runtime=status,
            loaded=True,
        )

    def _apply_read(self, snapshot: Snapshot) -> None:
        if self._closed:
            return
        self.snapshot = replace(snapshot, error=self._action_error)
        self._broadcast()
        self._finish_read()

    def _read_failed(self, message: str) -> None:
        if self._closed:
            return
        # Keep the last good reading on screen and say what went wrong beside
        # it. Blanking the window on a transient failure teaches people that
        # the app forgets their settings.
        self.snapshot = replace(self.snapshot, error=message)
        self._broadcast()
        self._finish_read()

    def _finish_read(self) -> None:
        self._reading = False
        again, self._reload_again = self._reload_again, False
        if again:
            self.reload()

    # ── writing ───────────────────────────────────────────────────────────

    def set(self, key: str, value: str) -> None:
        """Change one setting, then re-read. Never assume the write landed."""
        if self._closed:
            return
        if key in self._writing:
            self._queued_values[key] = value
            return
        self._writing.add(key)
        if key == "denoiser":
            self._broadcast()
        self._runner.call(
            lambda: cli.set_value(key, value),
            lambda result: self._finish_setting(key, result, None),
            lambda message: self._finish_setting(key, None, message),
        )

    def _finish_setting(self, key: str, result: object, error: str | None) -> None:
        self._writing.discard(key)
        pending = self._queued_values.pop(key, None)
        if self._closed:
            return
        if error is None:
            self._after_write(result)
        else:
            self._write_failed(error)
        if pending is not None:
            self.set(key, pending)

    def set_debounced(self, key: str, value: str, ms: int = 250) -> None:
        """For controls that produce a value per pixel of travel."""
        if self._closed:
            return
        debouncer = self._debouncers.get(key)
        if debouncer is None:
            debouncer = Debouncer(ms)
            self._debouncers[key] = debouncer
        debouncer.call(lambda: self.set(key, value))

    def cancel_pending(self, key: str) -> None:
        """Discard unsent slider values superseded by a preset/reset action.

        An in-flight command is not interrupted. The serialized runner finishes
        it before the new action; only delayed or coalesced values are removed.
        """
        debouncer = self._debouncers.get(key)
        if debouncer is not None:
            debouncer.cancel()
        self._queued_values.pop(key, None)

    def is_pending(self, key: str) -> bool:
        debouncer = self._debouncers.get(key)
        return (
            key in self._writing
            or key in self._queued_values
            or bool(debouncer and debouncer.pending)
        )

    def call(
        self,
        action: Callable[[], object],
        on_done: Callable | None = None,
        on_error: Callable | None = None,
    ) -> None:
        """Run something on the backend that is not a settings write."""
        self._tasks.call(action, on_done, on_error)

    def repair(self) -> None:
        if self._closed or self.is_pending("__repair__"):
            return
        self._writing.add("__repair__")
        self._broadcast()
        self._runner.call(
            cli.repair,
            lambda result: self._finish_setting("__repair__", result, None),
            lambda message: self._finish_setting("__repair__", None, message),
        )

    def unmute(self) -> None:
        source = self.snapshot.current_source
        if source:
            self._runner.call(
                lambda: cli.set_source_muted(source, False),
                self._after_write,
                self._write_failed,
            )

    def set_volume(self, source: str, percent: int) -> None:
        if (
            self._closed
            or not source
            or source != self.snapshot.current_source
            or calibration_window(self.snapshot.calibration()) is not None
        ):
            return
        self._runner.call(
            lambda: cli.set_source_volume(source, percent),
            self._after_write,
            self._write_failed,
        )

    def set_level_range(self, source: str, bounds: tuple[int, int] | None) -> None:
        if self._closed or not source or source != self.snapshot.current_source:
            return
        self._runner.call(
            lambda: cli.set_level_range(source, bounds),
            self._after_write,
            self._write_failed,
        )

    def _after_write(self, _result: object) -> None:
        # A write that worked is the only thing that clears a write that did not.
        self._action_error = ""
        # Coalesce: a burst of writes deserves one read-back, not one each.
        if self._reread_pending:
            return
        self._reread_pending = True
        reread = self._debouncers.setdefault("__reread__", Debouncer(REREAD_MS))
        reread.call(self._do_reread)

    def _do_reread(self) -> None:
        self._reread_pending = False
        self.reload()

    def _write_failed(self, message: str) -> None:
        logger.warning("settings write failed: %s", message)
        self._action_error = message
        self.snapshot = replace(self.snapshot, error=message)
        self._broadcast()
        # Re-read anyway: the backend may have applied part of the change, and
        # the screen must show what is true rather than what was attempted.
        self.reload()

    def shutdown(self) -> None:
        self._closed = True
        self._reload_again = False
        self._queued_values.clear()
        for debouncer in self._debouncers.values():
            debouncer.cancel()
        self._subscribers.clear()
        if self._events is not None:
            self._events.shutdown()
        self._tasks.shutdown()
