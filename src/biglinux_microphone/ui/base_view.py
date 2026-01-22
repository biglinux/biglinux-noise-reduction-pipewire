#!/usr/bin/env python3
"""
Base view classes for BigLinux Microphone Settings UI.

Provides base classes and common functionality for all views.
Note: We cannot use ABC with GObject-based classes due to metaclass conflicts.
Instead, we use NotImplementedError for abstract method enforcement.
"""

from __future__ import annotations

import logging
from collections.abc import Callable
from typing import TYPE_CHECKING

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, GLib, Gtk

if TYPE_CHECKING:
    from biglinux_microphone.services import PipeWireService, SettingsService

logger = logging.getLogger(__name__)


class BaseView(Adw.NavigationPage):
    """
    Base class for all application views.

    Provides common functionality:
    - Service injection
    - Settings management
    - Navigation support
    - Loading state handling

    Note: Subclasses MUST override _setup_ui() and _load_state().
    """

    __gtype_name__ = "BaseView"

    def __init__(
        self,
        title: str,
        tag: str,
        pipewire_service: PipeWireService,
        settings_service: SettingsService,
    ) -> None:
        """
        Initialize the base view.

        Args:
            title: View title for navigation
            tag: Unique tag for navigation
            pipewire_service: PipeWire backend service
            settings_service: Settings persistence service
        """
        super().__init__(title=title, tag=tag)

        self._pipewire = pipewire_service
        self._settings_service = settings_service
        self._settings = settings_service.get()
        self._is_loading = False

        # Setup UI
        self._setup_ui()
        self._load_state()

    def _setup_ui(self) -> None:
        """Set up the view's UI layout. Override in subclasses."""
        raise NotImplementedError("Subclasses must implement _setup_ui()")

    def _load_state(self) -> None:
        """Load current state from settings. Override in subclasses."""
        raise NotImplementedError("Subclasses must implement _load_state()")

    def _save_settings(self) -> None:
        """Save current settings to disk."""
        self._settings_service.save(self._settings)

    def _navigate_to(self, tag: str) -> None:
        """
        Navigate to another view.

        Args:
            tag: Target view tag
        """
        window = self.get_root()
        if hasattr(window, "navigate_to"):
            window.navigate_to(tag)

    def _set_loading(self, loading: bool) -> None:
        """
        Set loading state for the view.

        Args:
            loading: True if loading, False otherwise
        """
        self._is_loading = loading
        self.set_sensitive(not loading)

    def _show_toast(self, message: str, timeout: int = 3) -> None:
        """
        Show a toast notification.

        Args:
            message: Toast message
            timeout: Duration in seconds
        """
        window = self.get_root()
        if hasattr(window, "show_toast"):
            window.show_toast(message, timeout)

    def _schedule_update(
        self, callback: Callable[[], bool], delay_ms: int = 100
    ) -> int:
        """
        Schedule a delayed update.

        Args:
            callback: Function to call
            delay_ms: Delay in milliseconds

        Returns:
            int: Timer ID (can be used to cancel)
        """
        return GLib.timeout_add(delay_ms, callback)


class ScrollableView(BaseView):
    """
    Base view with scrollable content area.

    Provides:
    - Scrolled window container
    - Content clamp for responsive layout
    - Main box for content

    Note: Subclasses must override _build_content() and _load_state().
    """

    __gtype_name__ = "ScrollableView"

    def __init__(
        self,
        title: str,
        tag: str,
        pipewire_service: PipeWireService,
        settings_service: SettingsService,
        max_content_width: int = 600,
    ) -> None:
        """
        Initialize the scrollable view.

        Args:
            title: View title
            tag: View tag
            pipewire_service: PipeWire service
            settings_service: Settings service
            max_content_width: Maximum content width in pixels
        """
        self._max_content_width = max_content_width
        self._content_box: Gtk.Box | None = None
        super().__init__(title, tag, pipewire_service, settings_service)

    def _setup_ui(self) -> None:
        """Set up scrollable content area."""
        # Scrolled window
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)

        # Clamp for content width
        clamp = Adw.Clamp()
        clamp.set_maximum_size(self._max_content_width)
        clamp.set_margin_top(24)
        clamp.set_margin_bottom(24)
        clamp.set_margin_start(12)
        clamp.set_margin_end(12)

        # Main content box
        self._content_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=24)

        # Build view-specific content
        self._build_content()

        clamp.set_child(self._content_box)
        scrolled.set_child(clamp)
        self.set_child(scrolled)

    def _build_content(self) -> None:
        """Build view-specific content in self._content_box. Override in subclasses."""
        raise NotImplementedError("Subclasses must implement _build_content()")

    def _add_group(self, group: Adw.PreferencesGroup) -> None:
        """
        Add a preferences group to the content.

        Args:
            group: PreferencesGroup to add
        """
        if self._content_box is not None:
            self._content_box.append(group)


class PreferencesView(ScrollableView):
    """
    Base view for preferences pages.

    Provides standard preferences page layout with groups.

    Note: Subclasses must override _create_groups() and _load_state().
    """

    __gtype_name__ = "PreferencesView"

    def _build_content(self) -> None:
        """Build preferences content."""
        # Get groups from subclass
        groups = self._create_groups()
        for group in groups:
            if group is not None:
                self._add_group(group)

    def _create_groups(self) -> list[Adw.PreferencesGroup | None]:
        """
        Create preferences groups for the view.

        Returns:
            list: List of PreferencesGroup objects
        """
        raise NotImplementedError("Subclasses must implement _create_groups()")
