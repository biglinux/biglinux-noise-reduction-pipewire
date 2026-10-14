# SPDX-FileCopyrightText: 2026 BigLinux contributors
# SPDX-License-Identifier: MIT
"""One tooltip look for every BigLinux program.

The Python port of big-gtk-kit's ``src/tooltip.rs``; both draw the card from
the same ``tooltip.css``, copied beside this file. Call :func:`install` once,
after GTK is initialised (an application's ``startup``). From then on a
widget's own ``tooltip-text`` or ``tooltip-markup`` shows as the BigLinux card
instead of GTK's tooltip window.
Call sites do not change, and the accessible name and description GTK derives
from the tooltip stay.

Each window and popover gets one card, made the first time something in it is
hovered. A hovered widget answers GTK's ``query-tooltip`` with "none" from then
on, so the two never show together; ``has-tooltip`` stays the program's to turn
off.
"""

import time
import weakref
from pathlib import Path

import gi

gi.require_version("Gdk", "4.0")
gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, GLib, GObject, Gtk

SHOW_DELAY_MS = 150
# Reaching another widget this soon after a card hid shows its card at once.
BROWSE_WINDOW_S = 0.3
MAX_WIDTH_CHARS = 80
CARD_CLASS = "big-tooltip"
CLOSING_EVENTS = (
    Gdk.EventType.BUTTON_PRESS,
    Gdk.EventType.KEY_PRESS,
    Gdk.EventType.SCROLL,
    Gdk.EventType.TOUCH_BEGIN,
)

# Python attributes on the widget wrappers; PyGObject keeps a wrapper that
# carries attributes alive exactly as long as its widget.
_ENGINE = "_big_gtk_kit_tooltip_engine"
_SILENCED = "_big_gtk_kit_tooltip_silenced"

_installed = False


def install():
    """Show every tooltip of this process as the BigLinux card.

    Idempotent. Windows realized before the call are covered too.
    """
    global _installed
    if _installed:
        return
    _installed = True
    display = Gdk.Display.get_default()
    if display is not None:
        provider = Gtk.CssProvider()
        # An application provider answers @media from its own properties, not
        # from the user's settings as the theme does.
        settings = Gtk.Settings.get_for_display(display)
        for setting, preference in (
            ("gtk-interface-color-scheme", "prefers-color-scheme"),
            ("gtk-interface-contrast", "prefers-contrast"),
            ("gtk-interface-reduced-motion", "prefers-reduced-motion"),
        ):
            # GTK 4.20 added them; an older one, as in a CI image, keeps the
            # light card.
            if provider.find_property(preference) is None:
                continue
            settings.bind_property(
                setting, provider, preference, GObject.BindingFlags.SYNC_CREATE
            )
        provider.load_from_path(str(Path(__file__).with_name("tooltip.css")))
        # Process-wide on purpose: the card is the same in every window.
        Gtk.StyleContext.add_provider_for_display(
            display, provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION + 100
        )
    # A class's signals exist once the class is initialised.
    GObject.TypeClass.get(Gtk.Widget.__gtype__)
    GObject.add_emission_hook(Gtk.Widget, "realize", _on_realize)
    for window in Gtk.Window.list_toplevels():
        if window.get_realized():
            _attach(window)


def _on_realize(widget):
    if isinstance(widget, Gtk.Native):
        _attach(widget)
    return True


def _attach(native):
    # The card is a popover too; it has nothing of its own to show.
    if native.has_css_class(CARD_CLASS) or getattr(native, _ENGINE, None):
        return
    engine = _Engine(native)
    # Callbacks hold the engine weakly: the native owns it, through them too.
    ref = weakref.ref(engine)

    def call(method, *args):
        engine = ref()
        if engine is not None:
            getattr(engine, method)(*args)

    motion = Gtk.EventControllerMotion(propagation_phase=Gtk.PropagationPhase.CAPTURE)
    motion.connect("enter", lambda _c, x, y: call("hovered", x, y))
    motion.connect("motion", lambda _c, x, y: call("hovered", x, y))
    motion.connect("leave", lambda _c: call("hide"))

    # A click, a key, a scroll or a touch closes the card, as GTK's does.
    def on_event(controller, _event):
        # PyGObject 3.56 hands this signal's GdkEvent over as None.
        event = controller.get_current_event()
        if event is not None and event.get_event_type() in CLOSING_EVENTS:
            call("dismiss")
        return False

    events = Gtk.EventControllerLegacy(propagation_phase=Gtk.PropagationPhase.CAPTURE)
    events.connect("event", on_event)
    for controller in (motion, events):
        native.add_controller(controller)
        engine.controllers.append(controller)
    engine.handlers.append(native.connect("unrealize", _detach))
    if isinstance(native, Gtk.Window):

        def on_active(window, _pspec):
            if not window.is_active():
                call("hide")

        engine.handlers.append(native.connect("notify::is-active", on_active))
    setattr(native, _ENGINE, engine)


def _detach(native):
    """Undo ``_attach``; the next realize attaches again."""
    engine = getattr(native, _ENGINE, None)
    if engine is None:
        return
    delattr(native, _ENGINE)
    engine.hide()
    if engine.card is not None:
        engine.card.unparent()
        engine.card = None
    for controller in engine.controllers:
        native.remove_controller(controller)
    for handler in engine.handlers:
        native.disconnect(handler)
    engine.controllers.clear()
    engine.handlers.clear()


class _Engine:
    def __init__(self, native):
        self.native = weakref.ref(native)
        self.card = None
        self.label = None
        self.owner = None
        self.owner_handlers = []
        self.timer = 0
        self.shown = False
        self.hidden_at = None
        # Pressed on the current owner: no card until the pointer moves on.
        self.pressed = False
        self.controllers = []
        self.handlers = []

    def owner_widget(self):
        return self.owner() if self.owner is not None else None

    def hovered(self, x, y):
        native = self.native()
        if native is None:
            return
        owner = _tip_owner(native, x, y)
        if owner is not None:
            # Before GTK's own hover timer can fire for it.
            _silence(owner)
        if owner is self.owner_widget():
            return
        browsing = self.shown or (
            self.hidden_at is not None
            and time.monotonic() - self.hidden_at < BROWSE_WINDOW_S
        )
        if owner is None:
            self.hide()
        else:
            # A card on screen moves to the next owner: popping it down and up
            # again would make the compositor a new surface for each one.
            self._release()
        self.pressed = False
        if owner is None:
            return
        self.owner = weakref.ref(owner)
        if browsing:
            self.show(instant=True)
            return
        self.timer = GLib.timeout_add(SHOW_DELAY_MS, self._on_delay)

    def _on_delay(self):
        # Ran: nothing left to remove.
        self.timer = 0
        self.show(instant=False)
        return GLib.SOURCE_REMOVE

    def show(self, instant):
        if not self._present(instant):
            self._popdown()

    def _present(self, instant):
        """Fill and point the card for the owner; False when it shows nothing."""
        native, owner = self.native(), self.owner_widget()
        if native is None or owner is None:
            return False
        if (
            self.pressed
            or not owner.get_mapped()
            or _menu_open(owner)
            or not _active(native)
        ):
            return False
        found, bounds = owner.compute_bounds(native)
        if not found:
            return False
        self._make_card(native)
        if not self._fill(owner):
            return False
        area = Gdk.Rectangle()
        area.x, area.y = int(bounds.get_x()), int(bounds.get_y())
        area.width, area.height = int(bounds.get_width()), int(bounds.get_height())
        self.card.set_pointing_to(area)
        self.card.remove_css_class("visible")
        if instant:
            self.card.add_css_class("instant")
        else:
            self.card.remove_css_class("instant")
        self.card.popup()
        self.card.add_css_class("visible")
        self.shown = True

        # The text may change while the card shows (a play button becoming
        # pause): follow it.
        ref = weakref.ref(self)

        def on_text(owner, _pspec):
            engine = ref()
            if (
                engine is not None
                and engine.card is not None
                and not engine._fill(owner)
            ):
                engine.hide()

        for prop in ("tooltip-text", "tooltip-markup"):
            self.owner_handlers.append(owner.connect(f"notify::{prop}", on_text))
        return True

    def _make_card(self, native):
        if self.card is not None:
            return
        # GTK moves it below when there is no room above.
        self.card = Gtk.Popover(
            has_arrow=False,
            autohide=False,
            can_target=False,
            can_focus=False,
            position=Gtk.PositionType.TOP,
            css_classes=[CARD_CLASS],
        )
        # The owner's own name and description already carry the text. A role,
        # not the hidden state: GTK resets that on every show.
        self.label = Gtk.Label(
            accessible_role=Gtk.AccessibleRole.NONE,
            max_width_chars=MAX_WIDTH_CHARS,
        )
        self.card.set_child(self.label)
        self.card.set_parent(native)

    def _fill(self, owner):
        """Put ``owner``'s tooltip into the card; False when it has none any more."""
        markup = owner.get_tooltip_markup()
        text = markup or owner.get_tooltip_text()
        if not text:
            return False
        if markup:
            self.label.set_markup(markup)
        else:
            self.label.set_text(text)
        # Cards with structure (bold or small lines) read from the left and
        # keep their lines; a plain sentence is centred and wraps. Plain lines
        # of their own (a list of choices) wrap too, but read from the left.
        structured = _is_structured(markup)
        left = structured or "\n" in text
        self.label.set_wrap(not structured)
        self.label.set_xalign(0.0 if left else 0.5)
        self.label.set_justify(
            Gtk.Justification.LEFT if left else Gtk.Justification.CENTER
        )
        return True

    def dismiss(self):
        """Close the card for good on its owner: a press acted on it."""
        self._cancel_timer()
        self.pressed = True
        self._popdown()
        # The next owner waits its delay, as after any action.
        self.hidden_at = None

    def hide(self):
        self._release()
        self._popdown()

    def _release(self):
        """Let go of the owner, leaving the card as it is."""
        self._cancel_timer()
        owner = self.owner_widget()
        if owner is not None:
            for handler in self.owner_handlers:
                owner.disconnect(handler)
        self.owner_handlers.clear()
        self.owner = None

    def _popdown(self):
        if self.shown:
            self.shown = False
            if self.card is not None:
                self.card.popdown()
            self.hidden_at = time.monotonic()

    def _cancel_timer(self):
        if self.timer:
            GLib.source_remove(self.timer)
            self.timer = 0


def _tip_owner(native, x, y):
    """The widget under ``(x, y)`` whose tooltip shows there: the nearest with a
    tooltip text, as GTK picks it (insensitive widgets explain themselves too)."""
    widget = native.pick(x, y, Gtk.PickFlags.INSENSITIVE)
    while widget is not None and widget is not native:
        if _has_tip(widget):
            return widget
        widget = widget.get_parent()
    return None


def _has_tip(widget):
    # A widget its program turned has-tooltip off on keeps it off.
    return widget.get_has_tooltip() and bool(
        widget.get_tooltip_markup() or widget.get_tooltip_text()
    )


def _silence(widget):
    """Answer GTK's tooltip query for ``widget`` with "no tooltip", once per
    widget: the card shows instead. Stopping the emission skips GTK's default
    handler, which would answer with the text."""
    if getattr(widget, _SILENCED, False):
        return
    setattr(widget, _SILENCED, True)

    def on_query(widget, *_args):
        if _has_tip(widget):
            widget.stop_emission_by_name("query-tooltip")
        return False

    widget.connect("query-tooltip", on_query)


def _is_structured(markup):
    """GTK keeps an escaped copy of plain text as the markup too; only real
    markup has a tag."""
    return markup is not None and "<" in markup


def _active(native):
    root = native.get_root()
    return not isinstance(root, Gtk.Window) or root.is_active()


def _menu_open(owner):
    """A control whose own menu or popover is open shows no card over it."""
    if isinstance(owner, Gtk.MenuButton):
        return owner.get_active()
    child = owner.get_first_child()
    while child is not None:
        if isinstance(child, Gtk.Popover) and child.get_visible():
            return True
        child = child.get_next_sibling()
    return False
