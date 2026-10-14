"""Closed windows must finalize while the GTK process remains alive."""

import ctypes
import gc
import time
from collections import Counter
from types import SimpleNamespace
from unittest.mock import patch

import gi

gi.require_version("Adw", "1")
from biglinux_microphone import cli
from biglinux_microphone.application import MicrophoneApplication
from biglinux_microphone.ui.state import AudioState, Snapshot
from biglinux_microphone.ui.view import MicrophoneView
from biglinux_microphone.window import MicrophoneWindow
from gi.repository import Adw, GLib, Gtk

# Unlike weak notifications (dispose), qdata destruction proves finalization.
_gobject = ctypes.CDLL("libgobject-2.0.so.0")
_gobject.g_quark_from_string.argtypes = [ctypes.c_char_p]
_gobject.g_quark_from_string.restype = ctypes.c_uint
_gobject.g_object_set_qdata_full.argtypes = [
    ctypes.c_void_p,
    ctypes.c_uint,
    ctypes.c_void_p,
    ctypes.c_void_p,
]
_key = _gobject.g_quark_from_string(b"bigmic-finalization-test")
_tracked = {}


@ctypes.CFUNCTYPE(None, ctypes.c_void_p)
def _finalized(pointer):
    label, finalized = _tracked.pop(pointer)
    finalized[label] += 1


def _settle(context, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        context.iteration(False)
        time.sleep(0.01)


def test_closed_windows_finalize_with_all_pages():
    Adw.init()
    settings = {
        "noise_reduction": {"enabled": False},
        "equalizer": {},
        "echo_cancel": {},
    }
    created, finalized = Counter(), Counter()
    app = Adw.Application()
    app.register(None)
    context = GLib.MainContext.default()

    with (
        patch.object(AudioState, "_read", return_value=Snapshot(settings=settings)),
        patch.object(cli, "eq_presets", return_value=[]),
        patch.object(MicrophoneView, "_start_spectrum"),
    ):
        for _ in range(3):
            window = MicrophoneWindow(app)
            window.present()
            for page_id in (
                "noise_reduction",
                "voice_volume",
                "voice_sound",
                "echo_routing",
                "mic_test",
                "diagnostics",
                "source_picker",
                "calibrate",
            ):
                window._view.navigate(page_id)
            window._view._by_id["noise_reduction"]._ensure_editor()
            window._view._by_id["voice_sound"]._ensure_equalizer()
            window._view.navigate("home")
            window._view._cli_worker.submit(lambda: None).result(timeout=3)
            objects = {
                "window": window,
                "view": window._view,
                "meter": window._view._home._meter,
                **{page.id: page.widget for page in window._view._pages},
                "threshold": window._view._by_id["noise_reduction"]._editor.canvas,
            }
            _settle(context, 0.2)
            MicrophoneApplication._on_about(SimpleNamespace(_window=window), None, None)
            about = window.get_visible_dialog() or next(
                dialog
                for dialog in Gtk.Window.get_toplevels()
                if dialog.get_transient_for() == window
            )
            _settle(context, 0.4)
            assert about.get_mapped(), "About must render before testing its teardown"

            def track_about(widget):
                # GTK parents the display's single tooltip window to whichever
                # window was hovered last and keeps it until the tooltip
                # expires: not the dialog's content. Xvfb, with no window
                # manager, leaves the pointer resting on the dialog.
                if widget.__gtype__.name == "GtkTooltipWindow":
                    return
                pointer = hash(widget)
                _tracked[pointer] = ("about-content", finalized)
                created["about-content"] += 1
                _gobject.g_object_set_qdata_full(pointer, _key, pointer, _finalized)
                child = widget.get_first_child()
                while child is not None:
                    track_about(child)
                    child = child.get_next_sibling()

            track_about(about)
            for label, widget in objects.items():
                pointer = hash(widget)
                _tracked[pointer] = (label, finalized)
                created[label] += 1
                _gobject.g_object_set_qdata_full(pointer, _key, pointer, _finalized)
            del objects, widget
            about.close()
            del about
            window.close()
            del window
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline and created != finalized:
                while context.pending():
                    context.iteration(False)
                gc.collect()
                time.sleep(0.01)
            assert created == finalized, f"survivors: {created - finalized}"
