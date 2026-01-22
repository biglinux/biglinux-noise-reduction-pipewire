#!/usr/bin/env python3
"""
Main Adw.Application class for BigLinux Microphone Settings.

Handles application lifecycle, activation, and global application state.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gio, GLib, Gtk

from biglinux_microphone.config import (
    APP_DEVELOPER,
    APP_ID,
    APP_ISSUE_URL,
    APP_NAME,
    APP_VERSION,
    APP_WEBSITE,
)
from biglinux_microphone.resources import load_css
from biglinux_microphone.services.audio_monitor import AudioLevels, AudioMonitor
from biglinux_microphone.services.monitor_service import MonitorService
from biglinux_microphone.services.pipewire_service import PipeWireService
from biglinux_microphone.services.settings_service import SettingsService
from biglinux_microphone.utils.tooltip_helper import TooltipHelper
from biglinux_microphone.window import MicrophoneWindow

if TYPE_CHECKING:
    from biglinux_microphone.config import AppSettings

from biglinux_microphone.utils.i18n import _

logger = logging.getLogger(__name__)


class MicrophoneApplication(Adw.Application):
    """
    Main application class for BigLinux Microphone Settings.

    Manages application lifecycle, services, and global state.
    Following GNOME HIG and Libadwaita patterns.
    """

    def __init__(self) -> None:
        """Initialize the application."""
        super().__init__(
            application_id=APP_ID,
            flags=Gio.ApplicationFlags.DEFAULT_FLAGS,
        )

        # Initialize services (lazy loading)
        self._pipewire_service: PipeWireService | None = None
        self._settings_service: SettingsService | None = None
        self._audio_monitor: AudioMonitor | None = None
        self._monitor_service: MonitorService | None = None
        self._tooltip_helper: TooltipHelper | None = None

        # Window reference
        self._window: MicrophoneWindow | None = None

        # Set color scheme
        style_manager = Adw.StyleManager.get_default()
        style_manager.set_color_scheme(Adw.ColorScheme.PREFER_DARK)

        # Connect signals
        self.connect("activate", self._on_activate)
        self.connect("startup", self._on_startup)
        self.connect("shutdown", self._on_shutdown)

        logger.debug("Application initialized")

    @property
    def pipewire_service(self) -> PipeWireService:
        """Get PipeWire service (lazy initialization)."""
        if self._pipewire_service is None:
            self._pipewire_service = PipeWireService()
        return self._pipewire_service

    @property
    def settings_service(self) -> SettingsService:
        """Get settings service (lazy initialization)."""
        if self._settings_service is None:
            self._settings_service = SettingsService()
        return self._settings_service

    @property
    def settings(self) -> AppSettings:
        """Get application settings via settings_service (single source of truth)."""
        return self.settings_service.get()

    @property
    def audio_monitor(self) -> AudioMonitor:
        """Get audio monitor (lazy initialization)."""
        if self._audio_monitor is None:
            self._audio_monitor = AudioMonitor()
        return self._audio_monitor

    @property
    def monitor_service(self) -> MonitorService:
        """Get monitor service (lazy initialization)."""
        if self._monitor_service is None:
            self._monitor_service = MonitorService()
        return self._monitor_service

    @property
    def tooltip_helper(self) -> TooltipHelper:
        """Get tooltip helper (lazy initialization)."""
        if self._tooltip_helper is None:
            self._tooltip_helper = TooltipHelper()
        return self._tooltip_helper

    def _on_startup(self, app: Adw.Application) -> None:
        """Handle application startup - setup actions and shortcuts."""
        logger.debug("Application startup")

        # Load CSS styles
        load_css()

        # Create application actions
        self._create_actions()

        # Set application name for about dialog
        GLib.set_application_name(_(APP_NAME))

    def _on_activate(self, app: Adw.Application) -> None:
        """Handle application activation - create or present window."""
        logger.debug("Application activated")

        if self._window is None:
            self._window = MicrophoneWindow(application=app)

            # Start audio monitoring and connect to spectrum analyzer
            self._start_audio_monitoring()

        self._window.present()

    def _start_audio_monitoring(self) -> None:
        """Start audio monitoring for spectrum analyzer."""

        def on_audio_levels(levels: AudioLevels) -> None:
            """Callback to update spectrum with audio levels."""
            if self._window is not None:
                # Use spectrum bands if available, otherwise use level
                if levels.spectrum_bands:
                    GLib.idle_add(
                        self._window.update_spectrum_bands, levels.spectrum_bands
                    )
                else:
                    level = max(levels.input_level, levels.output_level)
                    GLib.idle_add(self._window.update_audio_level, level)

        self.audio_monitor.start(on_audio_levels)
        logger.info("Audio monitoring started for spectrum analyzer")

    def _on_shutdown(self, app: Adw.Application) -> None:
        """Handle application shutdown - cleanup resources."""
        logger.debug("Application shutdown")

        # Stop audio monitoring
        if self._audio_monitor is not None:
            self._audio_monitor.stop()

        # Stop headphone monitoring
        if self._monitor_service is not None:
            self._monitor_service.stop_monitor()

        # Cleanup tooltip helper
        if self._tooltip_helper is not None:
            self._tooltip_helper.cleanup()

        # Save settings on exit (via settings_service)
        if self._settings_service is not None:
            self._settings_service.save(self.settings)

    def _create_actions(self) -> None:
        """Create application-level actions."""
        # About action
        about_action = Gio.SimpleAction.new("about", None)
        about_action.connect("activate", self._on_about_action)
        self.add_action(about_action)

        # Quit action
        quit_action = Gio.SimpleAction.new("quit", None)
        quit_action.connect("activate", self._on_quit_action)
        self.add_action(quit_action)
        self.set_accels_for_action("app.quit", ["<Control>q"])

        # Preferences action (keyboard shortcut) - opens advanced expander
        prefs_action = Gio.SimpleAction.new("preferences", None)
        prefs_action.connect("activate", self._on_preferences_action)
        self.add_action(prefs_action)
        self.set_accels_for_action("app.preferences", ["<Control>comma"])

    def _on_about_action(
        self, action: Gio.SimpleAction, param: GLib.Variant | None
    ) -> None:
        """Show about dialog."""
        about = Adw.AboutDialog.new()
        about.set_application_name(_(APP_NAME))
        about.set_version(APP_VERSION)
        about.set_developer_name(APP_DEVELOPER)
        about.set_license_type(Gtk.License.GPL_3_0)
        about.set_website(APP_WEBSITE)
        about.set_issue_url(APP_ISSUE_URL)
        about.set_application_icon("biglinux-noise-reduction-pipewire")
        about.set_copyright("© 2025-2026 BigLinux Team")
        about.set_developers(["BigLinux Team"])
        about.set_comments(
            _("AI-powered noise reduction with stereo enhancement and equalization")
        )

        # Add credits
        about.add_credit_section(
            _("Powered by"),
            ["GTCRN Neural Network", "PipeWire Audio Server"],
        )

        # Add link to GTCRN project
        about.add_link(_("GTCRN Project"), "https://github.com/Xiaobin-Rong/gtcrn")

        about.present(self._window)
        logger.debug("About dialog shown")

    def _on_quit_action(
        self, action: Gio.SimpleAction, param: GLib.Variant | None
    ) -> None:
        """Handle quit action."""
        logger.info("Quit action triggered")
        self.quit()

    def _on_preferences_action(
        self, action: Gio.SimpleAction, param: GLib.Variant | None
    ) -> None:
        """Handle preferences action - show toast with hint."""
        if self._window is not None:
            self._window.show_toast("Use expanders to access settings", 2)
