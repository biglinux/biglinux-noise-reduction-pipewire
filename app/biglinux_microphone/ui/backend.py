"""Serialize blocking backend calls off the GTK main thread.

One worker preserves settings-write order; results are delivered back through
the GLib main loop and discarded after shutdown.
"""

from __future__ import annotations

import logging
import os
import subprocess
import threading
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor

import gi

gi.require_version("Gtk", "4.0")

from gi.repository import Gio, GLib

from biglinux_microphone import cli

logger = logging.getLogger(__name__)


class BackendRunner:
    """A serialized worker for CLI calls, with results delivered on the UI thread."""

    def __init__(self, on_error: Callable[[str], None]) -> None:
        self._default_error = on_error
        self._closed = False
        self.executor = ThreadPoolExecutor(
            max_workers=1, thread_name_prefix="biglinux-cli-"
        )

    @property
    def closed(self) -> bool:
        return self._closed

    def dispatch(self, callback: Callable, *args) -> None:
        """Run `callback` on the GTK main loop unless this runner is shut down."""

        def deliver() -> bool:
            if not self._closed:
                callback(*args)
            return False  # never repeat an idle source if a callback returns True

        GLib.idle_add(deliver)

    def call(
        self,
        action: Callable[[], object],
        on_done: Callable | None = None,
        on_error: Callable | None = None,
    ) -> None:
        """Run `action` on the worker; deliver its result or its failure on the UI thread."""
        if self._closed:
            return

        def task() -> None:
            try:
                result = action()
            except Exception as exc:  # every backend failure is a message, not a crash
                logger.exception("backend call failed")
                self.dispatch(on_error or self._default_error, str(exc))
                return
            if on_done is not None:
                self.dispatch(on_done, result)

        self.executor.submit(task)

    def shutdown(self) -> None:
        """Stop accepting work and drop what has not started. Idempotent."""
        self._closed = True
        self.executor.shutdown(wait=False, cancel_futures=True)


class BackendEvents:
    """Watch audio and atomic settings replacements only while the window exists."""

    def __init__(self, on_change: Callable[[], None]) -> None:
        self._on_change = on_change
        self._stop = threading.Event()
        self._lock = threading.Lock()
        self._proc: subprocess.Popen | None = None
        self._timer = 0
        path = os.path.join(
            GLib.get_user_config_dir(), "biglinux-microphone", "settings.json"
        )
        self._file_monitor = Gio.File.new_for_path(path).monitor_file(
            Gio.FileMonitorFlags.WATCH_MOVES, None
        )
        self._file_monitor.connect("changed", lambda *_args: self._changed())
        self._thread = threading.Thread(
            target=self._listen, name="bigmic-events", daemon=True
        )
        self._thread.start()

    def _changed(self) -> bool:
        if not self._stop.is_set() and not self._timer:
            self._timer = GLib.timeout_add(200, self._deliver)
        return False

    def _deliver(self) -> bool:
        self._timer = 0
        if not self._stop.is_set():
            self._on_change()
        return False

    def _listen(self) -> None:
        while not self._stop.is_set():
            proc = None
            try:
                proc = subprocess.Popen(
                    [cli._pactl(), "subscribe"],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.DEVNULL,
                    text=True,
                    encoding="utf-8",
                    env={**os.environ, "LC_ALL": "C"},
                )
                with self._lock:
                    self._proc = proc
                if self._stop.is_set():
                    break
                GLib.idle_add(self._changed)
                for line in proc.stdout:
                    # Client and source-output events include our own read-only
                    # queries. Listening to them would make refresh feed itself.
                    if any(
                        f" on {kind} #" in line
                        for kind in ("source", "sink", "server", "card")
                    ):
                        GLib.idle_add(self._changed)
                    if self._stop.is_set():
                        break
            except (OSError, UnicodeError, cli.BackendError):
                logger.debug("audio event connection unavailable", exc_info=True)
            finally:
                if proc is not None:
                    cli.terminate_process(proc)
                    if proc.stdout is not None:
                        proc.stdout.close()
                with self._lock:
                    self._proc = None
            GLib.idle_add(self._changed)
            # Reconnect after the audio server returns; no queries while idle.
            self._stop.wait(3)

    def shutdown(self) -> None:
        self._stop.set()
        self._file_monitor.cancel()
        if self._timer:
            GLib.source_remove(self._timer)
            self._timer = 0
        with self._lock:
            proc = self._proc
        if proc is not None:
            threading.Thread(
                target=cli.terminate_process,
                args=(proc,),
                name="bigmic-events-stop",
                daemon=True,
            ).start()
