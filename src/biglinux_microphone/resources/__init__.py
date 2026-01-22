"""
Resource management for BigLinux Microphone Settings.

Handles loading CSS, icons, and other resources.
"""

from __future__ import annotations

import logging
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gio, Gtk

logger = logging.getLogger(__name__)

# Resource path
RESOURCES_DIR = Path(__file__).parent


def load_css() -> None:
    """Load application CSS styles."""
    css_file = RESOURCES_DIR / "style.css"

    if not css_file.exists():
        logger.warning("CSS file not found: %s", css_file)
        return

    try:
        css_provider = Gtk.CssProvider()
        css_provider.load_from_path(str(css_file))

        display = Gdk.Display.get_default()
        if display:
            Gtk.StyleContext.add_provider_for_display(
                display,
                css_provider,
                Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
            )
            logger.debug("CSS styles loaded successfully")
    except Exception:
        logger.exception("Error loading CSS styles")


def get_icon_path(name: str) -> str | None:
    """
    Get the path to an icon file.

    Args:
        name: Icon name without extension

    Returns:
        str or None: Full path to icon file
    """
    icon_dirs = [
        Path("/usr/share/icons/hicolor/scalable/apps"),
        Path("/usr/share/icons/hicolor/scalable/status"),
        Path.home() / ".local" / "share" / "icons" / "hicolor" / "scalable" / "apps",
    ]

    for icon_dir in icon_dirs:
        for ext in (".svg", ".png"):
            icon_path = icon_dir / f"{name}{ext}"
            if icon_path.exists():
                return str(icon_path)

    return None
