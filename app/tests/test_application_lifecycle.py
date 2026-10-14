"""Exercise an actual Gio/GTK quit path without recording or touching audio."""
from unittest.mock import patch

import gi

gi.require_version("Adw", "1")
from gi.repository import Gio, GLib
from biglinux_microphone import cli
from biglinux_microphone.application import MicrophoneApplication
from biglinux_microphone.ui.view import MicrophoneView


def test_application_quit_shuts_down_view_without_window_close():
    settings = {"noise_reduction": {"enabled": False}, "equalizer": {}, "echo_cancel": {}}
    with patch.object(cli, "load_status", return_value={"settings": settings}), \
         patch.object(cli, "list_sources", return_value=[]), \
         patch.object(cli, "get_default_source", return_value=""), \
         patch.object(cli, "get_source_volume", return_value=None), \
         patch.object(cli, "eq_presets", return_value=[]), \
         patch.object(MicrophoneView, "_start_spectrum"):
        app = MicrophoneApplication()
        app.set_flags(Gio.ApplicationFlags.NON_UNIQUE)
        def quit_after_activate(_app):
            GLib.idle_add(lambda: (app.quit(), GLib.SOURCE_REMOVE)[1])
        app.connect("activate", quit_after_activate)
        assert app.run(["biglinux-microphone-lifecycle-test"]) == 0
        assert app._window._view._closed
        assert app._window._view._state._closed
        app._window.destroy()
