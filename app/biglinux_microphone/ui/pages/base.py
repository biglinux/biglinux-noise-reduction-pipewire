"""What every page is, and what the shell may ask of one.

A page owns a slice of the window and nothing else: it builds its widgets once
and re-renders them whenever the store hands it a new reading. It never reads
another page's widgets and never writes the backend directly — that rule is the
whole reason six pages can show the same microphone without disagreeing.
"""

from __future__ import annotations

from dataclasses import dataclass

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gtk


@dataclass
class PageContext:
    """What the shell lends every page."""

    #: The single source of settings truth.
    state: object
    #: The one spectrum/meter instance, which lives in the shell, not in a page.
    meter: object
    #: Show a transient message.
    toast: object
    #: Switch to another page by id, for the "more options" links.
    navigate: object
    #: Shell-owned runtime actions that are not persistent settings (for example
    #: delayed live monitoring). Keep backend/settings writes in ``state``.
    shell: object = None


class Page:
    """Base class: identity for the sidebar, a widget for the stack."""

    #: Stack name and sidebar row id.
    id: str = ""
    #: Sidebar label; a page sets this with `_()` at build time.
    title: str = ""
    #: Symbolic icon name for the sidebar row.
    icon: str = "preferences-system-symbolic"
    #: False for a page you arrive at from somewhere else and leave when it is
    #: finished. Calibration is one: a sidebar row for it would offer a task as
    #: if it were a place.
    in_sidebar: bool = True

    def __init__(self, context: PageContext) -> None:
        self.ctx = context
        self.widget = self.build()

    def build(self) -> Gtk.Widget:  # pragma: no cover — every page overrides
        raise NotImplementedError

    def sync(self, snapshot: object) -> None:
        """Render the reading. Called on every broadcast, including the first."""

    def on_peak(self, peak: float) -> None:
        """The newest sample peak, 0..1, at the meter's rate. For the pages
        that show how loud the microphone is *right now*."""

    def on_bands(self, bands: list[float]) -> None:
        """Newest normalised spectrum frame for deliberately visual pages."""

    def on_monitor(self, active: bool) -> None:
        """The delayed live return started or stopped. Every control that
        offers it renders this rather than what its own click implied."""

    def on_shown(self) -> None:
        """This page is now the one on screen. Start anything that should only
        run while somebody can see it — a poll, a meter, a preview."""

    def on_hidden(self) -> None:
        """The person navigated away. Release anything that only makes sense
        while this page is on screen — an open editor, a poll, a preview."""

    def shutdown(self) -> None:
        """The window is closing."""


def scrolled(child: Gtk.Widget, max_width: int = 980) -> Gtk.Widget:
    """The standard page frame: clamped width, scrolls only when it must."""
    clamp = Adw.Clamp(maximum_size=max_width, child=child)
    scroller = Gtk.ScrolledWindow(vexpand=True)
    scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
    scroller.set_child(clamp)
    return scroller


def slider_row(
    title: str, scale: Gtk.Scale, value: Gtk.Label, subtitle: str = ""
) -> Adw.PreferencesRow:
    """Give the explanation and slider independent space on narrow displays."""
    row = Adw.PreferencesRow()
    row.set_focusable(False)
    body = Gtk.Box(
        orientation=Gtk.Orientation.VERTICAL,
        spacing=6,
        margin_top=12,
        margin_bottom=12,
        margin_start=12,
        margin_end=12,
    )
    body.append(Gtk.Label(label=title, xalign=0, wrap=True))
    if subtitle:
        body.append(
            Gtk.Label(
                label=subtitle,
                xalign=0,
                wrap=True,
                css_classes=["dim-label", "caption"],
            )
        )
    controls = Gtk.Box(spacing=12)
    scale.set_draw_value(False)
    scale.set_hexpand(True)
    scale.set_size_request(120, -1)
    scale.update_property([Gtk.AccessibleProperty.LABEL], [title])
    scale.set_valign(Gtk.Align.CENTER)
    value.set_valign(Gtk.Align.CENTER)
    value.add_css_class("numeric")
    value.add_css_class("dim-label")
    controls.append(scale)
    controls.append(value)
    body.append(controls)
    row.set_child(body)
    return row


def column(spacing: int = 16) -> Gtk.Box:
    """A page's vertical body, with the margins every page shares."""
    return Gtk.Box(
        orientation=Gtk.Orientation.VERTICAL,
        spacing=spacing,
        margin_top=18,
        margin_bottom=24,
        margin_start=18,
        margin_end=18,
    )
