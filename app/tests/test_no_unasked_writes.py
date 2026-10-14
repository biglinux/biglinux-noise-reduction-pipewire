"""Opening the window, and looking at it, must change nothing.

Every control here renders a reading, and rendering a reading is one
`set_active` away from looking exactly like a person's click. That mistake is
silent — the screen keeps showing the truth, because the value it wrote is the
value it was rendering — and it only surfaces later, as a setting the person
never chose. It has happened once already: the home page's quality choice wrote
`model standard` over a saved DPDFNet while merely drawing itself.

So: build the window, feed it a settings file, walk every page, and assert the
backend was never asked to write anything.
"""

from __future__ import annotations

import unittest
from unittest.mock import patch

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gdk, GLib, Gtk

from biglinux_microphone import cli
from biglinux_microphone.ui.view import _TASKS, MicrophoneView

#: A saved state where every control has something non-default to render: the
#: filter on, a model other than the default, an equalizer that is off with a preset
#: named, and a calibrated device.
SETTINGS = {
    "noise_reduction": {
        "enabled": True,
        "strength": 0.9,
        "model": 13,
        "silence_depth": 20,
        "post_filter_beta": 0.02,
    },
    "equalizer": {
        "preset": "voice_boost",
        "bands": [3, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        "enabled": False,
    },
    "echo_cancel": {"mode": "off"},
    "mic_calibration": {"devices": {"mic": {"volume": 0.62}}},
}


class NoUnaskedWrites(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        Adw.init()
        if Gdk.Display.get_default() is None:
            raise unittest.SkipTest("no display for GTK")

    def test_rendering_the_saved_state_writes_nothing(self) -> None:
        writes: list[tuple] = []
        stubs = {
            "load_status": lambda: {
                "settings": SETTINGS,
                "audio_available": True,
                "mic_running": True,
                "effective_model": 13,
            },
            "list_sources": lambda: [{"name": "mic", "description": "Test Mic"}],
            "get_default_source": lambda: "mic",
            "get_source_volume": lambda _name: 62,
            "eq_presets": lambda: ["flat", "voice_boost", "custom"],
            "latency": lambda *_a: {"total_ms": 130.0, "stages": []},
            "set_value": lambda key, value: writes.append(("set", key, value)),
            "set_default_source": lambda name: writes.append(("source", name)),
            "set_source_volume": lambda name, pct: writes.append(("volume", name, pct)),
            "start_calibration": lambda *_a: writes.append(("start_calibration",)),
        }
        with patch.multiple(cli, **{k: v for k, v in stubs.items()}):
            with patch.object(MicrophoneView, "_start_spectrum"):
                view = MicrophoneView(on_toast=lambda _m: None)
            self.addCleanup(view.shutdown)
            # In a real window, because the panels this test opens need a
            # surface to sit on.
            window = Gtk.Window(child=view)
            window.present()
            self.addCleanup(window.destroy)
            self._settle(view)
            # Every card opens as for the keyboard: a card that writes as it
            # draws its choices is the same bug one level down.
            cards = view._controls.cards
            for rows, _count in cards.groups:
                for row in rows:
                    if row.card is not None and row.get_sensitive():
                        cards.hover_cards.open_for_keyboard(row)
                        self._settle(view)
                        cards.close()
            for factory in _TASKS:
                # Calibration is included: it asks a question on arrival and
                # listens only once somebody has answered it, so merely opening
                # it must be as harmless as opening any other page.
                view.navigate(factory.id)
                self._settle(view)
            view.navigate("home")
            self._settle(view)
        self.assertEqual(writes, [], f"the window wrote without being asked: {writes}")

    def test_closing_the_window_writes_nothing(self) -> None:
        """Teardown is a write path, and the least expected one.

        A grouped toggle that is taken apart hands its active state to whatever
        is left in the group, and that looks exactly like somebody clicking the
        other option — one second before the process exits, with nobody there to
        see it. This is how a saved DPDFNet became "standard" on a run where the
        window was only ever looked at.
        """
        writes: list[tuple] = []
        with patch.multiple(
            cli,
            load_status=lambda: {
                "settings": SETTINGS,
                "audio_available": True,
                "mic_running": True,
                "effective_model": 13,
            },
            list_sources=lambda: [{"name": "mic", "description": "Test Mic"}],
            get_default_source=lambda: "mic",
            get_source_volume=lambda _name: 62,
            eq_presets=lambda: ["flat", "voice_boost", "custom"],
            set_value=lambda key, value: writes.append(("set", key, value)),
        ):
            with patch.object(MicrophoneView, "_start_spectrum"):
                view = MicrophoneView(on_toast=lambda _m: None)
            window = Gtk.Window()
            window.set_child(view)
            window.present()
            self._settle(view)
            writes.clear()  # whatever start-up did is the other test's business
            view.shutdown()
            window.destroy()
            context = GLib.MainContext.default()
            while context.pending():
                context.iteration(False)
        self.assertEqual(writes, [], f"closing the window wrote: {writes}")

    def _settle(self, view: MicrophoneView) -> None:
        view._cli_worker.submit(lambda: None).result(timeout=5)
        context = GLib.MainContext.default()
        for _ in range(3):
            while context.pending():
                context.iteration(False)
            view._cli_worker.submit(lambda: None).result(timeout=5)


if __name__ == "__main__":
    unittest.main()
