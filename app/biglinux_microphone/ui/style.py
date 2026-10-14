"""Small CSS layer for custom visualisations only.

Ordinary controls intentionally use libadwaita defaults so the application
inherits light/dark/high-contrast themes without a parallel design system.
"""

from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, Gtk

CSS = """
.activity-card { padding: 12px 14px 8px; border-radius: 12px; }
.activity-meter { color: @success_color; }
.threshold-card { padding: 12px; border-radius: 12px; }
.eq-bands { padding: 14px 10px; }
.numeric { font-variant-numeric: tabular-nums; }
.pass-progress trough { min-height: 10px; border-radius: 6px; }
.pass-progress progress { min-height: 10px; border-radius: 6px; }
.preset-button { padding: 4px 8px; }
"""


# A view can be recreated; installing one provider per view accumulates CSS
# providers on the display for the whole lifetime of the process.
_providers: dict[Gdk.Display, Gtk.CssProvider] = {}


def install(display: Gdk.Display | None = None) -> None:
    display = display or Gdk.Display.get_default()
    if display is None or display in _providers:
        return
    provider = Gtk.CssProvider()
    provider.load_from_string(CSS)
    Gtk.StyleContext.add_provider_for_display(
        display, provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
    )
    _providers[display] = provider
