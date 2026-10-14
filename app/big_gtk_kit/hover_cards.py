# SPDX-FileCopyrightText: 2026 BigLinux contributors
# SPDX-License-Identifier: MIT
"""Cards that open as the pointer reaches their anchor.

The anchors sit in one line (a list, a bar) and every card opens on the same
side of it, in a layer above the window's content: a little apart from its
anchor with an arrow pointing at it, or joined to it by a bridge. A card opens as soon as the pointer
reaches its anchor, and the pointer can travel from the anchor into the card across the neighboring
anchors: a triangle from where it left the anchor to the card's near edge is
part of the card's region, and an anchor inside that triangle takes over only
when the pointer leaves the triangle or rests there. The geometry is plain functions of
boxes (``card_geometry``); :class:`HoverIntent` (``hover_intent``) decides with an
injected clock, so tests run it without a display; :class:`HoverCards` connects it to GTK.

A click, tap or Enter opens a card for the keyboard (focused, closed by Escape
or a click elsewhere); the pointer never replaces such a card. Escape also
closes a card the pointer opened.
"""

import math
import weakref

import gi

gi.require_version("Adw", "1")
gi.require_version("Gdk", "4.0")
gi.require_version("Graphene", "1.0")
gi.require_version("Gsk", "4.0")
gi.require_version("Gtk", "4.0")
from gi.repository import Adw, Gdk, GLib, GObject, Graphene, Gsk, Gtk

from big_gtk_kit.card_geometry import (
    RING,
    RING_INSET,
    TAB_RADIUS,
    arrow_shape,
    bridge_shape,
    card_box,
    contains,
    shape_bounds,
)
from big_gtk_kit.hover_intent import HoverIntent

# One light above and a little to the left: each layer falls twice as far
# down as right and twice as far as the last, faintly, so together they read
# as one soft shadow, tight near the card and diffuse away from it.
ELEVATION = ((0.5, 1, 1.5, 0.1), (1.5, 3, 6, 0.1), (4, 8, 16, 0.1), (8, 16, 32, 0.1))
BRIDGE_FOOT = 0.25  # the shadow's strength where the bridge leaves the anchor
RISE_MS = 180  # a bridged card rising to its height
RISE_SCALE = 0.97  # how small it starts, growing out of its bridge
OPEN_CLASS = "card-open"


_SIDES = {
    Gtk.PositionType.RIGHT: "right",
    Gtk.PositionType.LEFT: "left",
    Gtk.PositionType.TOP: "top",
    Gtk.PositionType.BOTTOM: "bottom",
}
FADE_MS = 120


def _weak(method):
    """A signal handler that does not keep the method's owner alive: the
    widgets hold their controllers, and the owner holds the widgets."""
    reference = weakref.WeakMethod(method)

    def invoke(*args):
        callback = reference()
        return callback(*args) if callback is not None else None

    return invoke


def _child_popup_visible(widget):
    child = widget.get_first_child()
    while child is not None:
        if isinstance(child, Gtk.Popover) and child.get_visible():
            return True
        if _child_popup_visible(child):
            return True
        child = child.get_next_sibling()
    return False


def _path(polygons):
    builder = Gsk.PathBuilder.new()
    for polygon in polygons:
        for op in polygon:
            if op[0] == "M":
                builder.move_to(op[1], op[2])
            elif op[0] == "L":
                builder.line_to(op[1], op[2])
            elif op[0] == "A":
                builder.arc_to(*op[1:])
            elif op[0] == "C":
                builder.cubic_to(*op[1:])
            else:
                builder.close()
    return builder.to_path()


class CardLayer(Gtk.Fixed):
    """The window's layer for cards, an overlay above all its content. Only
    the cards and their bridges take the pointer; the rest of the layer lets it
    through to the window below."""

    def __init__(self):
        super().__init__()
        self.owners = weakref.WeakSet()  # the HoverCards placing cards here

    def do_contains(self, _x, _y):
        return False


class _TabEdge(Gtk.Widget):
    """The tab's open sides, stroked in their own CSS ``color``."""

    def __init__(self):
        super().__init__(
            css_classes=["card-tab-edge"],
            accessible_role=Gtk.AccessibleRole.PRESENTATION,
            can_target=False,
        )
        self.path = None

    def do_snapshot(self, snapshot):
        if self.path is not None:
            snapshot.append_stroke(self.path, Gsk.Stroke.new(RING), self.get_color())


class _Tab(Gtk.Widget):
    """The shape joining an open card to its anchor, in its CSS ``color``."""

    def __init__(self, css_classes):
        super().__init__(
            css_classes=["card-tab", *css_classes],
            accessible_role=Gtk.AccessibleRole.PRESENTATION,
            focusable=False,
            layout_manager=Gtk.BinLayout(),
        )
        self._path = None
        self._edge = _TabEdge()
        self._edge.set_parent(self)

    def detach_edge(self):
        """GTK may dispose the tab after Python has shut down, when this
        override cannot run: its owner detaches the edge first."""
        if self._edge.get_parent() is not None:
            self._edge.unparent()

    def do_dispose(self):
        self.detach_edge()
        Gtk.Widget.do_dispose(self)

    def set_shape(self, polygons, edges, origin):
        def local(paths):
            return [
                [
                    (op[0], *[v - origin[i % 2] for i, v in enumerate(op[1:])])
                    for op in ops
                ]
                for ops in paths
            ]

        self._path = _path(local(polygons))
        self._edge.path = _path(local(edges)) if edges else None
        self.queue_draw()
        self._edge.queue_draw()

    def do_snapshot(self, snapshot):
        if self._path is not None:
            snapshot.append_fill(self._path, Gsk.FillRule.WINDING, self.get_color())
            self.snapshot_child(self._edge, snapshot)

    def do_contains(self, x, y):
        return self._path is not None and self._path.in_fill(
            Graphene.Point().init(x, y), Gsk.FillRule.WINDING
        )


class _Backdrop(Gtk.Widget):
    """The shadow a bridged card casts with its bridge, beneath both and as
    one shape: a shadow per part would leave a seam where they meet. Its CSS
    ``color`` tints the shadow. It covers the window, in its coordinates."""

    def __init__(self, css_classes):
        super().__init__(
            css_classes=["card-shade", *css_classes],
            accessible_role=Gtk.AccessibleRole.PRESENTATION,
            can_target=False,
        )
        self._shape = None
        self._lift = 1.0  # how far the card has risen

    def set_shape(self, bridge, card, radii, shade, clip):
        x0, y0, x1, y1 = card
        rect = Gsk.RoundedRect()
        rect.init(
            Graphene.Rect().init(x0, y0, x1 - x0, y1 - y0),
            *(Graphene.Size().init(r, r) for r in radii),
        )
        foot, crest = (Graphene.Point().init(*point) for point in shade)
        self._shape = (_path([bridge]), rect, foot, crest, Graphene.Rect().init(*clip))
        self.queue_draw()

    def set_lift(self, lift):
        self._lift = lift
        self.queue_draw()

    def do_snapshot(self, snapshot):
        if self._shape is None:
            return
        bridge, card, foot, crest, clip = self._shape
        tint, lift = self.get_color(), self._lift
        shadows = []
        for dx, dy, blur, alpha in ELEVATION:
            shadow = Gsk.Shadow()
            shadow.color = _tint(tint, alpha * lift)
            shadow.dx, shadow.dy, shadow.radius = dx * lift, dy * lift, blur * lift
            shadows.append(shadow)
        stops = []
        for offset, alpha in ((0, BRIDGE_FOOT), (1, 1)):
            stop = Gsk.ColorStop()
            stop.offset, stop.color = offset, _tint(tint, alpha)
            stops.append(stop)
        # Never on the anchors, which lie under the card; and the bridge
        # rises from their plane, so its shadow grows from faint where it
        # leaves the anchor to the card's own.
        snapshot.push_clip(clip)
        snapshot.push_mask(Gsk.MaskMode.ALPHA)
        snapshot.append_linear_gradient(clip, foot, crest, stops)
        snapshot.pop()
        # Only the shadow, outside the shape: the shape would show through
        # the card while it fades in.
        snapshot.push_mask(Gsk.MaskMode.INVERTED_ALPHA)
        self._snapshot_shape(snapshot, bridge, card, tint)
        snapshot.pop()
        snapshot.push_shadow(shadows)
        self._snapshot_shape(snapshot, bridge, card, tint)
        snapshot.pop()
        snapshot.pop()
        snapshot.pop()
        snapshot.pop()

    @staticmethod
    def _snapshot_shape(snapshot, bridge, card, color):
        snapshot.append_fill(bridge, Gsk.FillRule.WINDING, color)
        snapshot.push_rounded_clip(card)
        snapshot.append_color(color, card.bounds)
        snapshot.pop()


def _tint(color, alpha):
    return Gdk.RGBA(red=color.red, green=color.green, blue=color.blue, alpha=alpha)


class HoverCards:
    """Cards in the window's :class:`CardLayer` that open as the pointer
    reaches their anchors, each a little apart from its anchor with an arrow
    pointing at it, or with ``bridge`` joined to the ring its anchor draws by
    a bridge.

    ``side`` is where every card opens relative to the line of anchors. The
    consumer styles ``.hover-card`` (the card), the anchor of the open card
    (``.card-open``) and ``.card-tab`` (its ``color`` fills the arrow or
    bridge: the card's own). A bridged card and its open anchor draw an outline
    ``RING`` wide inside their edges, the anchor's ``RING_INSET`` in;
    ``.card-tab > .card-tab-edge`` gives the bridge's in its ``color``. A
    bridged card and its bridge cast one shadow, tinted by the ``color`` of
    ``.card-shade``; the card itself casts none. It rises as it opens,
    growing out of its bridge as its shadow deepens. A scrolled view holding an open card's anchor is
    ``.card-open`` too. Hooks: ``can_open(name)`` may refuse to open;
    ``edge(name)`` gives the card's near edge in window coordinates (by
    default 12 px past the anchor). ``later`` and ``cancel`` default to GLib
    timeouts.
    """

    def __init__(
        self,
        layer,
        side=Gtk.PositionType.RIGHT,
        later=None,
        cancel=None,
        bridge=False,
    ):
        self.layer = layer
        layer.owners.add(self)
        self.side = side
        self.bridge = bridge
        self.panels = {}  # name -> the card's frame in the layer
        self._tabs = {}  # name -> its arrow or bridge in the layer
        self._shades = {}  # name -> a bridged card's backdrop in the layer
        self._anchors = {}  # name -> anchor
        self._scrollers = {}  # name -> the scroller that caps the card's height
        self._focus = {}  # name -> widget focused when a click opens the card
        self._points = {}  # name -> widget its arrow points at
        self._inside = {}  # name -> the card's motion controller
        self._held = set()  # names with a button down inside
        self._sources = set()
        self._root = None  # the window whose frames move the open card
        self._watched = None  # (window, its controllers)
        self._after_paint = None  # (frame clock, handler) while a card is open
        self._placed = {}  # name -> (anchor box, window size) it was placed at
        self._spots = {}  # name -> (where it grows from, its parts' places) in the layer
        self._fade = None
        self.can_open = None
        self.edge = None
        self._intent = HoverIntent(
            _SIDES[side],
            self._boxes,
            self._show,
            self.close,
            later or self._later,
            cancel or self._cancel,
            can_open=self._allowed,
            is_held=self._is_held,
        )

    # Public API.

    def attach(
        self, name, anchor, content, title, css_classes=(), focus=None, point_at=None
    ):
        """``content`` becomes ``name``'s card beside ``anchor``; ``title``
        names it to screen readers. Its arrow points at ``point_at``, by
        default the anchor."""
        side = _SIDES[self.side]
        scroller = Gtk.ScrolledWindow(
            child=content,
            hscrollbar_policy=Gtk.PolicyType.NEVER,
            propagate_natural_height=True,
            propagate_natural_width=True,
        )
        panel = Gtk.Box(
            css_classes=["hover-card", f"side-{side}", *css_classes],
            overflow=Gtk.Overflow.HIDDEN,
            visible=False,
            accessible_role=Gtk.AccessibleRole.GROUP,
        )
        panel.update_property([Gtk.AccessibleProperty.LABEL], [title])
        panel.append(scroller)
        # The card lives outside the anchor's tree, so it takes its state here.
        anchor.bind_property(
            "sensitive", panel, "sensitive", GObject.BindingFlags.SYNC_CREATE
        )
        if self.bridge:
            # Beneath the card and its bridge.
            self._shades[name] = _Backdrop(css_classes)
            self._shades[name].set_visible(False)
            self.layer.put(self._shades[name], 0, 0)
        self.layer.put(panel, 0, 0)
        # Above the card, so the card's outline stops at it. An arrow lets the
        # pointer through, as the gap it crosses.
        self._tabs[name] = _Tab(css_classes)
        self._tabs[name].set_visible(False)
        self._tabs[name].set_can_target(self.bridge)
        self.layer.put(self._tabs[name], 0, 0)
        self.panels[name] = panel
        self._anchors[name] = anchor
        self._scrollers[name] = scroller
        self._focus[name] = focus
        self._points[name] = point_at or anchor

        hover = Gtk.EventControllerMotion()
        hover.connect("enter", _weak(self._on_anchor_pointer), name)
        hover.connect("motion", _weak(self._on_anchor_pointer), name)
        hover.connect("leave", _weak(self._on_anchor_leave), name)
        anchor.add_controller(hover)
        inside = Gtk.EventControllerMotion()
        inside.connect("enter", _weak(self._on_card_enter), name)
        inside.connect("leave", _weak(self._on_card_leave), name)
        panel.add_controller(inside)
        self._inside[name] = inside
        # Sees presses and releases without claiming them from the controls.
        buttons = Gtk.EventControllerLegacy(
            propagation_phase=Gtk.PropagationPhase.CAPTURE
        )
        buttons.connect("event", _weak(self._on_card_event), name)
        panel.add_controller(buttons)
        keys = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
        keys.connect("key-pressed", _weak(self._on_card_key), name)
        panel.add_controller(keys)
        # The window's controllers must be in place before the pointer enters:
        # GTK tells a controller added later nothing when the pointer leaves.
        self._watch(anchor.get_root())
        anchor.connect("notify::root", _weak(self._on_anchor_rooted))

    @property
    def pinned(self):
        """The card a click or key opened, or None."""
        return self._intent.pinned

    def is_open(self, name):
        return self.panels[name].get_visible()

    def open(self, name):
        """Open ``name``'s card as the pointer reaching its anchor would."""
        box = self._anchor_box(name)
        apex = ((box[0] + box[2]) / 2, (box[1] + box[3]) / 2) if box else None
        self._intent.open(name, apex)

    def open_for_keyboard(self, name):
        self._intent.pin(name)
        self._present(name)
        focus = self._focus[name]
        if callable(focus):
            focus = focus()  # the control to focus can change between openings
        # A container takes no focus itself: its first control does.
        if focus is None or not focus.grab_focus():
            self.panels[name].child_focus(Gtk.DirectionType.TAB_FORWARD)

    def close(self, name):
        panel = self.panels[name]
        if not panel.get_visible():
            return
        root = panel.get_root()
        focused = root.get_focus() if root is not None else None
        had_focus = focused is not None and focused.is_ancestor(panel)
        if self._fade is not None and self._fade[0] == name:
            self._fade[1].skip()
        for part in self._parts(name):
            part.set_visible(False)
        self._mark_open(name, False)
        self._held.discard(name)
        self._intent.closed(name)
        if had_focus:
            anchor = self._anchors[name]
            # A bar's anchor is a box: its button takes the focus.
            if not anchor.grab_focus():
                anchor.child_focus(Gtk.DirectionType.TAB_FORWARD)
        if not any(p.get_visible() for p in self.panels.values()):
            self._unfollow()

    def close_all(self):
        for name in self.panels:
            self.close(name)

    def close_hover(self):
        """Close the card the pointer opened, as when its anchors scroll."""
        self._intent.close_now()

    def reposition(self, name):
        """Place an open card again if its anchor or the window moved."""
        if self.is_open(name) and self._placed.get(name) != self._placement(name):
            self._place(name)

    def refit(self, name):
        """Place an open card again after its content changed size."""
        if self.is_open(name):
            self._place(name)

    def unparent_all(self):
        self._intent.cancel_all()
        self._cancel_after_paint()
        if self._fade is not None:
            self._fade[1].skip()
        for source in list(self._sources):
            GLib.source_remove(source)
        self._sources.clear()
        self._unfollow()
        if self._watched is not None:
            root, controllers = self._watched
            self._watched = None
            for controller in controllers:
                root.remove_controller(controller)
        for name in list(self.panels):
            for part in self._parts(name):
                self.layer.remove(part)
            self._tabs[name].detach_edge()

    # Placement and geometry.

    def _show(self, name):
        self._present(name)

    def _present(self, name):
        # One card at a time in the window: the previous one goes at once,
        # never fading out beside the next.
        for owner in list(self.layer.owners):
            for other in owner.panels:
                if owner is not self or other != name:
                    owner.close(other)
        was_open = self.panels[name].get_visible()
        self._mark_open(name, True)
        # Shown before it is placed: a hidden widget measures nothing.
        for part in self._parts(name):
            if not was_open:
                part.set_opacity(0)
            part.set_visible(True)
        self._place(name)
        self._follow(self._anchors[name].get_root())
        if not was_open:
            self._fade_in(name)

    def _parts(self, name):
        """What shows a card: its backdrop if bridged, the card, its arrow or
        bridge."""
        shade = self._shades.get(name)
        return [*([shade] if shade else []), self.panels[name], self._tabs[name]]

    def _fade_in(self, name):
        if self._fade is not None:
            self._fade[1].skip()
        parts = self._parts(name)
        shade = self._shades.get(name)

        def step(value):
            for part in parts:
                part.set_opacity(min(value * 2, 1))
            if shade is None or name not in self._spots:
                return
            shade.set_lift(value)
            # Coming nearer: it grows out of its bridge to its full size.
            (px, py), places = self._spots[name]
            scale = RISE_SCALE + (1 - RISE_SCALE) * value
            for part, (x, y) in zip(parts, places):
                transform = Gsk.Transform.new().translate(Graphene.Point().init(px, py))
                transform = transform.scale(scale, scale).translate(
                    Graphene.Point().init(x - px, y - py)
                )
                self.layer.set_child_transform(part, transform)

        # Adw skips to the end when the desktop asks for no animations.
        animation = Adw.TimedAnimation(
            widget=self.panels[name],
            value_from=0.0,
            value_to=1.0,
            duration=RISE_MS if shade else FADE_MS,
            target=Adw.CallbackAnimationTarget.new(step),
        )
        self._fade = (name, animation)
        animation.connect("done", _weak(self._on_faded))
        animation.play()

    def _on_faded(self, _animation):
        self._fade = None

    def _mark_open(self, name, is_open):
        # The scrolled view holding the anchor is marked too, so its overlay
        # scrollbar can stay out of the bridge's way.
        anchor = self._anchors[name]
        for widget in (anchor, anchor.get_ancestor(Gtk.ScrolledWindow)):
            if widget is None:
                continue
            if is_open:
                widget.add_css_class(OPEN_CLASS)
            else:
                widget.remove_css_class(OPEN_CLASS)

    def _view_box(self, name):
        """The scrolled view the anchor sits in, or None."""
        view = self._anchors[name].get_ancestor(Gtk.ScrolledWindow)
        return self._box_of(view) if view is not None else None

    def _edge(self, name, box):
        if self.edge is not None:
            return self.edge(name)
        side = _SIDES[self.side]
        return {
            "right": box[2] + 12,
            "left": box[0] - 12,
            "top": box[1] - 12,
            "bottom": box[3] + 12,
        }[side]

    def _place(self, name):
        anchor = self._anchors[name]
        root = anchor.get_root()
        box = self._anchor_box(name)
        if root is None or box is None:
            return
        side = _SIDES[self.side]
        edge = self._edge(name, box)
        panel, scroller = self.panels[name], self._scrollers[name]
        scroller.set_max_content_height(-1)
        panel.set_size_request(-1, -1)
        _min, width, _mb, _nb = panel.measure(Gtk.Orientation.HORIZONTAL, -1)
        _min, height, _mb, _nb = panel.measure(Gtk.Orientation.VERTICAL, width)
        window = (root.get_width(), root.get_height())
        if self.bridge:
            # The card lines up with the anchor's ring, not its box.
            x0, y0, x1, y1 = box
            inset = RING_INSET
            box = (
                (x0, y0 + inset, x1, y1 - inset)
                if side in ("right", "left")
                else (x0 + inset, y0, x1 - inset, y1)
            )
        card = card_box(side, box, edge, (width, height), window)
        if card[3] - card[1] < height:
            scroller.set_max_content_height(max(round(card[3] - card[1]), 1))
        # The layer gives a child its minimum size; a card wants its natural one.
        panel.set_size_request(round(card[2] - card[0]), round(card[3] - card[1]))
        found, origin = root.compute_point(self.layer, Graphene.Point().init(0, 0))
        ox, oy = (origin.x, origin.y) if found else (0, 0)
        self.layer.move(panel, card[0] + ox, card[1] + oy)
        if self.bridge:
            polygons, edges, joined_start, joined_end, shade = bridge_shape(
                side, box, card
            )
            for css, on in (("joined-start", joined_start), ("joined-end", joined_end)):
                (panel.add_css_class if on else panel.remove_css_class)(css)
        else:
            target = self._box_of(self._points[name]) or box
            polygons, edges = [arrow_shape(side, target, card)], []
        bounds = shape_bounds(polygons + edges)
        tab = self._tabs[name]
        tab.set_size_request(
            math.ceil(bounds[2] - bounds[0]), math.ceil(bounds[3] - bounds[1])
        )
        tab.set_shape(polygons, edges, (bounds[0], bounds[1]))
        self.layer.move(tab, bounds[0] + ox, bounds[1] + oy)
        if self.bridge:
            joined = (joined_start, joined_end)
            self._place_shade(name, side, polygons[0], card, joined, shade)
            (cx, cy) = shade[1]
            places = [
                (ox, oy),
                (card[0] + ox, card[1] + oy),
                (bounds[0] + ox, bounds[1] + oy),
            ]
            self._spots[name] = ((cx + ox, cy + oy), places)
        self._placed[name] = self._placement(name)

    def _place_shade(self, name, side, bridge, card, joined, shade):
        root = self._anchors[name].get_root()
        width, height = root.get_width(), root.get_height()
        (fx, fy) = shade[0]
        # Beyond the anchors' edge only, on the card's side.
        clip = {
            "right": (fx, 0, width - fx, height),
            "left": (0, 0, fx, height),
            "top": (0, 0, width, fy),
            "bottom": (0, fy, width, height - fy),
        }[side]
        radii = [TAB_RADIUS] * 4  # top-left, top-right, bottom-right, bottom-left
        start, end = {"right": (0, 3), "left": (1, 2), "top": (3, 2), "bottom": (0, 1)}[
            side
        ]
        for corner, on in zip((start, end), joined):
            if on:
                radii[corner] = 0
        backdrop = self._shades[name]
        backdrop.set_size_request(width, height)
        backdrop.set_shape(bridge, card, radii, shade, clip)
        found, origin = root.compute_point(self.layer, Graphene.Point().init(0, 0))
        self.layer.move(backdrop, *((origin.x, origin.y) if found else (0, 0)))

    def _placement(self, name):
        root = self._anchors[name].get_root()
        size = (root.get_width(), root.get_height()) if root is not None else None
        return self._anchor_box(name), size

    def _box_of(self, widget):
        root = widget.get_root()
        if root is None:
            return None
        found, bounds = widget.compute_bounds(root)
        if not found:
            return None
        x, y = bounds.get_x(), bounds.get_y()
        return (x, y, x + bounds.get_width(), y + bounds.get_height())

    def _anchor_box(self, name):
        """The anchor as shown: the part its scrolled view does not hide."""
        box = self._box_of(self._anchors[name])
        view = self._view_box(name)
        if box is None or view is None:
            return box
        x0, y0 = max(box[0], view[0]), max(box[1], view[1])
        x1, y1 = min(box[2], view[2]), min(box[3], view[3])
        return (x0, y0, x1, y1) if x0 < x1 and y0 < y1 else None

    def _card_box(self, name):
        """The card as drawn, in the window's coordinates, or None."""
        panel = self.panels[name]
        if not panel.get_visible() or panel.get_root() is None:
            return None
        return self._box_of(panel)

    def _boxes(self, name):
        """The anchor reaching over the gap to its card, and the card."""
        anchor, card = self._anchor_box(name), self._card_box(name)
        if anchor is None or card is None:
            return anchor, card
        side = _SIDES[self.side]
        x0, y0, x1, y1 = anchor
        if side == "right":
            x1 = card[0]
        elif side == "left":
            x0 = card[2]
        elif side == "top":
            y0 = card[3]
        else:
            y1 = card[1]
        return (x0, y0, x1, y1), card

    def _on_tab(self, name, x, y):
        tab = self._tabs[name]
        root = tab.get_root()
        if root is None or not tab.get_visible():
            return False
        found, point = root.compute_point(tab, Graphene.Point().init(x, y))
        return found and tab.contains(point.x, point.y)

    def _anchor_at(self, point):
        for name, anchor in self._anchors.items():
            if not anchor.get_mapped():
                continue
            box = self._anchor_box(name)
            if box is not None and contains(box, point):
                return name
        return None

    def _allowed(self, name):
        anchor = self._anchors[name]
        # The pointer never replaces a card a click opened, whoever owns it.
        return (
            anchor.get_mapped()
            and anchor.is_sensitive()
            and all(owner.pinned is None for owner in self.layer.owners)
            and (self.can_open is None or self.can_open(name))
        )

    def _is_held(self, name):
        return name in self._held or _child_popup_visible(self.panels[name])

    # Events.

    def _on_anchor_pointer(self, controller, x, y, name):
        if self._intent.owner is not None:
            return  # the window's controller follows the pointer meanwhile
        anchor = self._anchors[name]
        root = anchor.get_root()
        if root is None:
            return
        found, point = anchor.compute_point(root, Graphene.Point().init(x, y))
        if found:
            self._intent.pointer(name, (point.x, point.y))

    def _on_anchor_leave(self, _controller, name):
        self._intent.left_anchor(name)

    def _on_card_enter(self, _controller, _x, _y, name):
        self._intent.entered_card(name)

    def _on_card_leave(self, _controller, name):
        self._intent.left_card(name)

    def _on_window_leave(self, _controller):
        if self._intent.owner is not None:
            self._intent.left_window()

    def _on_window_motion(self, _controller, x, y):
        # Only the way to an open card needs the whole window.
        if self._intent.owner is not None:
            self._intent.pointer(self._anchor_at((x, y)), (x, y))

    def _on_window_press(self, _gesture, _n_press, x, y):
        # A press outside a pinned card closes it and still does its own work.
        name = self._intent.pinned
        if name is None:
            return
        card, anchor = self._card_box(name), self._anchor_box(name)
        if card is not None and contains(card, (x, y)):
            return
        if anchor is not None and contains(anchor, (x, y)):
            return
        if not self._on_tab(name, x, y):
            self.close(name)

    def _on_anchor_rooted(self, anchor, _pspec):
        self._watch(anchor.get_root())

    def _on_window_key(self, _controller, keyval, _keycode, _state):
        if keyval != Gdk.KEY_Escape:
            return False
        if self._intent.pinned is not None:
            self.close(self._intent.pinned)
            return True
        return bool(self._intent.escape())

    def _on_card_key(self, _controller, keyval, _keycode, state, name):
        # Tab cycles within a pinned card, as within a dialog.
        if keyval not in (Gdk.KEY_Tab, Gdk.KEY_ISO_Left_Tab, Gdk.KEY_KP_Tab):
            return False
        backward = keyval == Gdk.KEY_ISO_Left_Tab or (
            state & Gdk.ModifierType.SHIFT_MASK
        )
        direction = (
            Gtk.DirectionType.TAB_BACKWARD
            if backward
            else Gtk.DirectionType.TAB_FORWARD
        )
        panel = self.panels[name]
        if not panel.child_focus(direction):
            root = panel.get_root()
            if root is not None:
                root.set_focus(None)
            panel.child_focus(direction)
        return True

    def _on_card_event(self, controller, _event, name):
        # PyGObject 3.56 hands this signal's GdkEvent over as None.
        event = controller.get_current_event()
        kind = event.get_event_type() if event is not None else None
        if kind in (Gdk.EventType.BUTTON_PRESS, Gdk.EventType.TOUCH_BEGIN):
            self._held.add(name)
        elif kind in (
            Gdk.EventType.BUTTON_RELEASE,
            Gdk.EventType.TOUCH_END,
            Gdk.EventType.TOUCH_CANCEL,
        ):
            self._held.discard(name)
            if not self._inside[name].contains_pointer():
                self._intent.left_card(name)
        return False

    def _watch(self, root):
        """Follow the pointer over the whole window, where the way to a card
        lies; take Escape; see presses outside a pinned card."""
        if root is None or (self._watched is not None and self._watched[0] is root):
            return
        motion = Gtk.EventControllerMotion()
        motion.connect("motion", _weak(self._on_window_motion))
        motion.connect("leave", _weak(self._on_window_leave))
        keys = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
        keys.connect("key-pressed", _weak(self._on_window_key))
        press = Gtk.GestureClick(propagation_phase=Gtk.PropagationPhase.CAPTURE)
        press.connect("pressed", _weak(self._on_window_press))
        for controller in (motion, keys, press):
            root.add_controller(controller)
        self._watched = (root, (motion, keys, press))

    def _follow(self, root):
        """While a card is open it follows its anchor whenever a frame moved
        it: a resize, a reflow, a scroll. Frames are drawn only when something
        changes."""
        if root is None or self._after_paint is not None:
            return
        self._watch(root)
        self._root = root
        clock = root.get_frame_clock()
        if clock is not None:
            handler = clock.connect("after-paint", _weak(self._on_after_paint))
            self._after_paint = (clock, handler)

    def _unfollow(self):
        self._cancel_after_paint()
        self._root = None

    def _on_after_paint(self, _clock):
        for name in self.panels:
            self.reposition(name)

    def _cancel_after_paint(self):
        if self._after_paint is not None:
            clock, handler = self._after_paint
            self._after_paint = None
            clock.disconnect(handler)

    # Default scheduling.

    def _later(self, milliseconds, callback):
        holder = []

        def fire():
            self._sources.discard(holder[0])
            callback()
            return GLib.SOURCE_REMOVE

        holder.append(GLib.timeout_add(milliseconds, fire))
        self._sources.add(holder[0])
        return holder[0]

    def _cancel(self, source):
        if source in self._sources:
            self._sources.discard(source)
            GLib.source_remove(source)
