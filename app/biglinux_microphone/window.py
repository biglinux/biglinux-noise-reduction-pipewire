"""Main application window.

The view owns native libadwaita header bars and navigation.  The window only
provides the toast overlay and lifecycle boundary.
"""

from __future__ import annotations

import logging

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw

from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.view import MicrophoneView
from biglinux_microphone.utils.i18n import _

logger = logging.getLogger(__name__)


class MicrophoneWindow(Adw.ApplicationWindow):
    """Window whose content is the microphone navigation view."""

    def __init__(self, application: Adw.Application) -> None:
        super().__init__(application=application)
        self.set_title(_("BigLinux Microphone"))
        self.set_default_size(1040, 720)

        self._toast_overlay = Adw.ToastOverlay()
        self._view = MicrophoneView(on_toast=weak_callback(self, type(self).show_toast))
        self._toast_overlay.set_child(self._view)
        self.set_content(self._toast_overlay)

        self.connect("close-request", weak_callback(self, type(self)._on_close_request))

    def show_toast(self, message: str) -> None:
        """Display a transient notification."""
        # Backend/device messages are literal text, never trusted Pango markup.
        toast = Adw.Toast.new(message)
        toast.set_use_markup(False)
        self._toast_overlay.add_toast(toast)

    def _on_close_request(self, _window: Adw.ApplicationWindow) -> bool:
        self._view.shutdown()
        return False
