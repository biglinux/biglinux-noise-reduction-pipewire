"""Adw.Application for the BigLinux Microphone UI.

No Python services live here: the window talks straight to :mod:`cli`.
"""

from __future__ import annotations

import logging

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

import big_gtk_kit
from gi.repository import Adw, Gio, GLib, Gtk

from biglinux_microphone import __version__
from biglinux_microphone.utils.i18n import _
from biglinux_microphone.window import MicrophoneWindow

logger = logging.getLogger(__name__)

APP_ID = "br.com.biglinux.microphone"


class MicrophoneApplication(Adw.Application):
    """Application lifecycle: create and present the single window."""

    def __init__(self) -> None:
        super().__init__(
            application_id=APP_ID, flags=Gio.ApplicationFlags.DEFAULT_FLAGS
        )
        self._window: MicrophoneWindow | None = None
        GLib.set_application_name(_("BigLinux Microphone"))
        self.connect("activate", self._on_activate)
        self.connect("startup", self._on_startup)
        self.connect("shutdown", self._on_shutdown)

    def _on_startup(self, _app: Adw.Application) -> None:
        big_gtk_kit.install()
        about = Gio.SimpleAction.new("about", None)
        about.connect("activate", self._on_about)
        self.add_action(about)

        quit_action = Gio.SimpleAction.new("quit", None)
        quit_action.connect("activate", lambda *_a: self.quit())
        self.add_action(quit_action)
        self.set_accels_for_action("app.quit", ["<Control>q"])

    def _on_shutdown(self, _app: Adw.Application) -> None:
        # Gio.Application.quit() (including Ctrl+Q) need not emit the window's
        # close-request. Always stop monitor/capture and finish owned rollback.
        if self._window is not None:
            self._window._view.shutdown()

    def _on_activate(self, _app: Adw.Application) -> None:
        if self._window is None:
            self._window = MicrophoneWindow(application=self)
        self._window.present()

    def _on_about(self, _action: Gio.SimpleAction, _param: object) -> None:
        about = Gtk.AboutDialog(transient_for=self._window, modal=True)
        about.set_program_name(_("BigLinux Microphone"))
        about.set_version(__version__)
        about.set_authors(["BigLinux"])
        about.set_license_type(Gtk.License.GPL_3_0)
        about.set_logo_icon_name("br.com.biglinux.microphone")
        about.present()
