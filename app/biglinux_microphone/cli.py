"""Thin access layer over the Rust backend (``biglinux-microphone-cli``).

The whole config/pipeline/PipeWire policy lives in the Rust binary; this module
only shells out to it and parses what it prints. Nothing here reimplements chain
logic — a slider moving is one ``set`` call, reading state is one ``settings``
or ``status`` call.

Hardware source enumeration and the "test my voice" round trip talk to the
standard PipeWire tools (``pactl``/``pw-record``/``pw-play``) directly, because
those are the platform's own commands and the UI is a thin layer over them.
"""

from __future__ import annotations

import contextlib
import json
import logging
import math
import shutil
import subprocess
import sys
from pathlib import Path

log = logging.getLogger(__name__)

#: The loudest capture level this application will set, as a percentage.
#: It is not a preference: it is what the backend's own level control tops out
#: at (`agc::control::MAX_VOL_DB`, 28.6 dB = 300%), and a calibration on a quiet
#: capsule routinely lands there. Refusing to write a number the calibration
#: itself produced is how the volume slider came to say "Invalid capture
#: volume" for a device that was already set that way.
MAX_CAPTURE_PERCENT = 300

#: The Rust backend binary. Installed on PATH by the package.
BACKEND = "biglinux-microphone-cli"

#: How long a backend call may run before we give up. ``apply``/``reload``
#: reconcile the graph and can legitimately take a few seconds; the model
#: measurement path is invoked by the backend itself, never from here.
_TIMEOUT = 30


class BackendError(RuntimeError):
    """A ``biglinux-microphone-cli`` invocation failed."""


class CalibrationSession:
    """Cancelable handle for one backend calibration pass."""

    def __init__(self, pass_name: str = "quick") -> None:
        if pass_name not in {"quick", "position"}:
            raise BackendError("Unknown calibration pass")
        binary = shutil.which(BACKEND)
        if binary is None:
            raise BackendError(f"{BACKEND} is not installed on PATH")
        try:
            self._proc = subprocess.Popen(
                [binary, "calibrate", pass_name],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
        except (OSError, subprocess.SubprocessError) as exc:
            raise BackendError(f"{BACKEND}: {exc}") from exc

    @property
    def process(self) -> subprocess.Popen:
        return self._proc

    def cancel(self) -> None:
        terminate_process(self._proc)

    def wait(self, timeout: int = 60) -> dict:
        try:
            stdout, stderr = self._proc.communicate(timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            terminate_process(self._proc)
            raise BackendError("Calibration timed out") from exc
        return _parse_calibration_result(stdout, stderr, self._proc.returncode)


def start_calibration(pass_name: str = "quick") -> CalibrationSession:
    return CalibrationSession(pass_name)


MIC_TEST_SECONDS = 5
MIC_TEST_RATE = 48_000


def _parse_calibration_result(stdout: str, stderr: str, returncode: int) -> dict:
    if not stdout.strip():
        raise BackendError(stderr.strip()[:2000] or "Calibration produced no result")
    result = _json(stdout, dict, "calibration")
    heard = result.get("heard_speech")
    volume = result.get("volume")
    if (
        not isinstance(heard, bool)
        or isinstance(volume, bool)
        or not isinstance(volume, (int, float))
        or not 0 <= volume <= 3
        or not isinstance(result.get("device"), str)
        or not result["device"]
    ):
        raise BackendError("Invalid calibration result")
    if returncode != 0 and heard:
        raise BackendError(stderr.strip()[:2000] or "Calibration could not be saved")
    return result


def _command(
    argv: list[str], *, timeout: int = _TIMEOUT, check: bool = True
) -> subprocess.CompletedProcess:
    """One error contract for GUI workers; never invoke a shell."""
    try:
        done = subprocess.run(
            argv, capture_output=True, text=True, timeout=timeout, check=False
        )
    except (OSError, subprocess.SubprocessError, UnicodeError) as exc:
        raise BackendError(f"{Path(argv[0]).name}: {exc}") from exc
    if check and done.returncode != 0:
        detail = done.stderr.strip()[:2000]
        raise BackendError(detail or f"{Path(argv[0]).name} failed ({done.returncode})")
    return done


def _run(args: list[str], *, timeout: int = _TIMEOUT) -> str:
    binary = shutil.which(BACKEND)
    if binary is None:
        raise BackendError(f"{BACKEND} is not installed on PATH")
    return _command([binary, *args], timeout=timeout).stdout


def _json(raw: str, expected: type, label: str):
    try:
        if len(raw) > 4 * 1024 * 1024:
            raise ValueError("response too large")
        value = json.loads(raw)
        if not isinstance(value, expected):
            raise TypeError(f"expected {expected.__name__}")
        pending = [value]
        while pending:
            item = pending.pop()
            if isinstance(item, float) and not math.isfinite(item):
                raise ValueError("non-finite JSON number")
            if isinstance(item, dict):
                pending.extend(item.values())
            elif isinstance(item, list):
                pending.extend(item)
        return value
    except (ValueError, TypeError, RecursionError) as exc:
        raise BackendError(f"Invalid {label} response: {exc}") from exc


def load_settings() -> dict:
    return _validate_settings(_json(_run(["settings"]), dict, "settings"))


def load_status() -> dict:
    status = _json(_run(["status"]), dict, "audio status")
    if not isinstance(status.get("settings"), dict):
        raise BackendError(
            "The audio backend must be updated together with the application"
        )
    _validate_settings(status["settings"])
    for key in ("audio_available", "mic_enabled", "mic_running", "aec_running"):
        if type(status.get(key)) is not bool:
            raise BackendError(f"Invalid audio status: {key}")
    if status.get("effective_model") is not None and (
        type(status["effective_model"]) is not int
        or status["effective_model"] not in (11, 12, 13)
    ):
        raise BackendError("Invalid effective microphone model")
    if status.get("denoising") is not None and type(status["denoising"]) is not bool:
        raise BackendError("Invalid denoising status")
    if status.get("mic_state") is not None and not isinstance(status["mic_state"], str):
        raise BackendError("Invalid microphone state")
    if status.get("mic_serial") is not None and not isinstance(
        status["mic_serial"], str
    ):
        raise BackendError("Invalid microphone identity")
    return status


def _validate_settings(settings: dict) -> dict:
    for key in ("noise_reduction", "equalizer", "echo_cancel"):
        if not isinstance(settings.get(key, {}), dict):
            raise BackendError(f"Invalid settings section: {key}")
    nr = settings.get("noise_reduction", {})
    for key in ("strength", "silence_depth"):
        if key in nr and (
            isinstance(nr[key], bool) or not isinstance(nr[key], (int, float))
        ):
            raise BackendError(f"Invalid noise reduction value: {key}")
    bands = settings.get("equalizer", {}).get("bands", [])
    if not isinstance(bands, list) or any(
        isinstance(v, bool) or not isinstance(v, (int, float)) for v in bands
    ):
        raise BackendError("Invalid equalizer bands")
    return settings


def eq_presets() -> list[str]:
    presets = _json(_run(["eq-presets"]), list, "presets")
    if not all(isinstance(p, str) for p in presets):
        raise BackendError("Invalid equalizer preset list")
    return presets


def set_value(key: str, value: str) -> None:
    """Set one named setting and let the backend reconcile the graph."""
    _run(["set", key, value])


# ── Hardware sources and the A/B "test my voice" round trip ────────────────
# These use the platform's own PipeWire tools rather than the backend: the UI
# is a thin layer and a mic list or a record-and-play is exactly what pactl and
# pw-record/pw-play already do.


def list_sources() -> list[dict]:
    """Capture devices for the picker, including explicitly marked virtual inputs.

    Monitors (loopbacks of a sink) and our own filter nodes are dropped: they
    are not microphones a person records from.
    """
    sources = []
    for src in _source_rows():
        name = src["name"]
        if _is_virtual_source(name) or src.get("monitor_of_sink") not in (
            None,
            4294967295,
            "n/a",
        ):
            continue
        props = src.get("properties") or {}
        if (
            props.get("device.class") == "monitor"
            or props.get("media.class") == "Audio/Sink"
        ):
            continue
        label = next(
            (
                value
                for value in (
                    src.get("description"),
                    props.get("node.description"),
                    props.get("device.description"),
                    name,
                )
                if isinstance(value, str) and value and value != "(null)"
            ),
            name,
        )
        ports = src.get("ports")
        port = (
            next(
                (
                    p
                    for p in ports
                    if isinstance(p, dict) and p.get("name") == src.get("active_port")
                ),
                {},
            )
            if isinstance(ports, list)
            else {}
        )
        sources.append(
            {
                "name": name,
                "description": label if isinstance(label, str) else name,
                "volume": _source_volume(src),
                "muted": src.get("mute") if type(src.get("mute")) is bool else None,
                "serial": str(props.get("object.serial", "")),
                "virtual": props.get("node.virtual") in (True, "true")
                or not props.get("device.api"),
                "available": port.get("availability")
                not in ("not available", "no", "unavailable"),
            }
        )
    return sources


def latency(model: str = "") -> dict:
    """What the microphone path costs, stage by stage, with provenance."""
    args = ["latency", model] if model else ["latency"]
    return _json(_run(args), dict, "latency")


def _pactl() -> str:
    binary = shutil.which("pactl")
    if binary is None:
        raise BackendError("pactl is not installed")
    return binary


def _source_rows() -> list[dict]:
    raw = _command([_pactl(), "-f", "json", "list", "sources"], timeout=10).stdout
    rows = _json(raw, list, "sources")
    for row in rows:
        if (
            not isinstance(row, dict)
            or not isinstance(row.get("name"), str)
            or not row["name"]
        ):
            raise BackendError("Invalid source identity")
        if not isinstance(row.get("properties") or {}, dict):
            raise BackendError("Invalid source properties")
    return rows


# Keep in lockstep with services::pipewire::sources::is_virtual_source in the
# Rust backend: the picker must hide monitors and every one of our own virtual
# endpoints, not just some, or a person can select a node that loops back.
_HIDDEN_SOURCE_NAMES = frozenset(
    {"mic-biglinux", "mic-biglinux-capture", "echo-cancel-source", "output-biglinux"}
)


def _is_virtual_source(name: str) -> bool:
    return (
        name.endswith(".monitor")
        or name.startswith(("big.filter-", "input.pw-loopback", "output.pw-loopback"))
        or name in _HIDDEN_SOURCE_NAMES
    )


def get_default_source() -> str:
    return _command([_pactl(), "get-default-source"], timeout=10).stdout.strip()


def get_source_volume(name: str) -> int:
    if not name:
        raise BackendError("No capture device selected")
    for src in _source_rows():
        if src["name"] != name:
            continue
        volume = _source_volume(src)
        if volume is not None:
            return volume
    raise BackendError("Capture device volume is unavailable")


def _source_volume(source: dict) -> int | None:
    channels = source.get("volume") or {}
    if isinstance(channels, dict):
        for channel in channels.values():
            if isinstance(channel, dict):
                percent = str(channel.get("value_percent", "")).rstrip("%")
                if percent.isdecimal() and len(percent) < 6:
                    return int(percent)
    return None


def repair() -> None:
    _run(["repair"])


def set_source_muted(name: str, muted: bool) -> None:
    if not name:
        raise BackendError("No capture device selected")
    _command([_pactl(), "set-source-mute", name, "1" if muted else "0"], timeout=10)


def set_source_volume(name: str, percent: int) -> None:
    if not name or type(percent) is not int or not 0 <= percent <= MAX_CAPTURE_PERCENT:
        raise BackendError("Invalid capture volume")
    _command([_pactl(), "set-source-volume", name, f"{percent}%"], timeout=10)


def set_level_range(source: str, bounds: tuple[int, int] | None) -> None:
    if bounds is not None and (
        len(bounds) != 2
        or any(type(value) is not int for value in bounds)
        or not 0 <= bounds[0] < bounds[1] <= MAX_CAPTURE_PERCENT
    ):
        raise BackendError("Invalid automatic volume limits")
    if not source or get_default_source() != source:
        raise BackendError("The microphone changed. Select its volume limits again.")
    _run(["level-range", *([str(v) for v in bounds] if bounds else ["off"])])


def monitor_argv() -> list[str]:
    """Command that runs the delayed microphone monitor as a subprocess."""
    # The production launcher adds a private root to its own sys.path, not to
    # subprocesses. The monitor is stdlib-only, so execute its package-owned
    # file directly in isolated mode instead of depending on site-packages.
    return [sys.executable, "-I", str(Path(__file__).resolve().with_name("monitor.py"))]


def set_default_source(name: str) -> None:
    """Pick the microphone by making it the system default source.

    The smart-filter follows whichever source is default rather than pinning a
    target, so this is how the picker selects a mic.
    """
    if not name:
        raise BackendError("No capture device selected")
    _command([_pactl(), "set-default-source", name], timeout=10)


def terminate_process(proc: subprocess.Popen) -> None:
    """Reap outside the GTK thread. Escalate TERM to KILL with bounded waits."""
    try:
        if proc.poll() is None:
            with contextlib.suppress(ProcessLookupError):
                proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            with contextlib.suppress(ProcessLookupError):
                proc.kill()
            proc.wait(timeout=2)
    except (OSError, subprocess.SubprocessError):
        log.exception("Cannot reap child pid=%s", proc.pid)
