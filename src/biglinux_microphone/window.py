#!/usr/bin/env python3
"""
Main application window for BigLinux Microphone Settings.

Implements Adw.ApplicationWindow with navigation and view management.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gdk, Gio, GLib, Gtk

from biglinux_microphone.config import (
    APP_NAME,
    ICON_APP,
    WINDOW_HEIGHT_MIN,
    WINDOW_WIDTH_MIN,
    save_settings,
)
from biglinux_microphone.ui.main_view import MainView

if TYPE_CHECKING:
    from biglinux_microphone.application import MicrophoneApplication

from biglinux_microphone.utils.i18n import _

logger = logging.getLogger(__name__)


class MicrophoneWindow(Adw.ApplicationWindow):
    """
    Main application window with navigation stack.

    Implements a modern GNOME-style interface with:
    - Header bar with navigation
    - Main content area with view stack
    - Responsive layout using Adw.Breakpoint
    """

    def __init__(self, application: MicrophoneApplication) -> None:
        """
        Initialize the main window.

        Args:
            application: Parent application instance
        """
        super().__init__(application=application)

        self._app = application
        self._settings = application.settings

        # Window state tracking
        self._size_change_timer: int = 0

        # Initialize UI
        self._setup_window()
        self._setup_actions()
        self._setup_content()
        self._setup_window_tracking()

        logger.debug("Window initialized")

    def _setup_window(self) -> None:
        """Configure window properties."""
        # Set window size from settings
        self.set_default_size(
            self._settings.window.width,
            self._settings.window.height,
        )
        self.set_size_request(WINDOW_WIDTH_MIN, WINDOW_HEIGHT_MIN)

        # Apply maximized state
        if self._settings.window.maximized:
            self.maximize()

        # Set window title
        self.set_title(_(APP_NAME))

    def _setup_content(self) -> None:
        """Create main window content with navigation."""
        # Toast overlay for notifications
        self._toast_overlay = Adw.ToastOverlay()

        # Main container using ToolbarView for proper header integration
        toolbar_view = Adw.ToolbarView()

        # Create header bar
        header_bar = self._create_header_bar()
        toolbar_view.add_top_bar(header_bar)

        # Create navigation view
        self._navigation_view = Adw.NavigationView()

        # Create main view (unified view with all settings)
        self._main_view = MainView(
            pipewire_service=self._app.pipewire_service,
            settings_service=self._app.settings_service,
            monitor_service=self._app.monitor_service,
            on_toast=self.show_toast,
            audio_monitor=getattr(self._app, "audio_monitor", None),
            tooltip_helper=self._app.tooltip_helper,
        )

        self._navigation_view.add(self._main_view)

        # Set content
        toolbar_view.set_content(self._navigation_view)
        self._toast_overlay.set_child(toolbar_view)
        self.set_content(self._toast_overlay)

    def show_toast(self, message: str, timeout: int = 3) -> None:
        """
        Show a toast notification.

        Args:
            message: Message to display
            timeout: Duration in seconds
        """
        toast = Adw.Toast.new(message)
        toast.set_timeout(timeout)
        self._toast_overlay.add_toast(toast)
        logger.debug("Toast shown: %s", message)

    def _create_header_bar(self) -> Adw.HeaderBar:
        """Create the header bar with app icon and menu."""
        header = Adw.HeaderBar()
        header.set_show_end_title_buttons(True)

        # Add app icon to header
        if ICON_APP.exists():
            try:
                file = Gio.File.new_for_path(str(ICON_APP))
                texture = Gdk.Texture.new_from_file(file)
                icon_image = Gtk.Image.new_from_paintable(texture)
                icon_image.set_pixel_size(24)
                icon_image.set_margin_start(8)
                icon_image.set_margin_end(4)
                icon_image.set_margin_end(4)
                header.pack_start(icon_image)
            except Exception:
                logger.exception("Error loading header icon")

        # Title widget
        title = Adw.WindowTitle.new(_(APP_NAME), "")
        header.set_title_widget(title)

        # Menu button
        menu_button = self._create_menu_button()
        header.pack_end(menu_button)

        return header

    def _setup_actions(self) -> None:
        """Setup window-scoped actions."""
        # Restore defaults action
        action = Gio.SimpleAction.new("restore-defaults", None)
        action.connect("activate", self._on_restore_defaults)
        self.add_action(action)

    def _on_restore_defaults(self, action=None, param=None) -> None:
        """Handle reset defaults action."""
        # Confirm dialog
        dialog = Adw.MessageDialog(
            transient_for=self,
            heading=_("Restore settings?"),
            body=_("This will return all adjustments to their original defaults."),
        )
        dialog.add_response("cancel", _("Cancel"))
        dialog.add_response("restore", _("Restore"))
        dialog.set_response_appearance("restore", Adw.ResponseAppearance.DESTRUCTIVE)
        dialog.set_default_response("cancel")
        dialog.set_close_response("cancel")

        dialog.connect("response", self._on_restore_confirmed)
        dialog.present()

    def _on_restore_confirmed(self, dialog: Adw.MessageDialog, response: str) -> None:
        """Handle restore confirmation."""
        if response == "restore" and hasattr(self, "_main_view"):
            self._main_view.restore_defaults()

    def _create_menu_button(self) -> Gtk.MenuButton:
        """Create the application menu button."""
        menu = Gio.Menu.new()
        # Add Restore Defaults to menu
        menu.append(_("Restore Defaults"), "win.restore-defaults")
        section = Gio.Menu.new()
        section.append(_("About"), "app.about")
        section.append(_("Quit"), "app.quit")
        menu.append_section(None, section)

        menu_button = Gtk.MenuButton()
        menu_button.set_icon_name("open-menu-symbolic")
        menu_button.set_menu_model(menu)
        menu_button.set_tooltip_text(_("Main menu"))

        return menu_button

    def _setup_window_tracking(self) -> None:
        """Setup tracking of window size and state."""
        self.connect("notify::default-width", self._on_window_size_changed)
        self.connect("notify::default-height", self._on_window_size_changed)
        self.connect("notify::maximized", self._on_window_maximized_changed)

    def _on_window_size_changed(self, *args: object) -> None:
        """Handle window size changes with debounce."""
        if self._size_change_timer:
            GLib.source_remove(self._size_change_timer)

        if not self.is_maximized():
            self._size_change_timer = GLib.timeout_add(500, self._save_window_size)

    def _save_window_size(self) -> bool:
        """Save current window size to settings."""
        width = self.get_width()
        height = self.get_height()

        if width > 0 and height > 0:
            self._settings.window.width = width
            self._settings.window.height = height
            save_settings(self._settings)

        self._size_change_timer = 0
        return False  # Don't repeat

    def _on_window_maximized_changed(self, *args: object) -> None:
        """Handle window maximized state change."""
        self._settings.window.maximized = self.is_maximized()
        save_settings(self._settings)

    def update_audio_level(self, level: float) -> None:
        """
        Update the spectrum analyzer with audio level.

        Args:
            level: Audio level (0.0 to 1.0)
        """
        if hasattr(self, "_main_view"):
            self._main_view.update_audio_level(level)

    def update_spectrum_bands(self, bands: list[float]) -> None:
        """
        Update the spectrum analyzer with frequency band data.

        Args:
            bands: List of frequency band levels (0.0 to 1.0)
        """
        if hasattr(self, "_main_view"):
            self._main_view.update_spectrum_bands(bands)
