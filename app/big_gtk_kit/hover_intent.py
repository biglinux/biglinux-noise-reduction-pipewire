# SPDX-FileCopyrightText: 2026 BigLinux contributors
# SPDX-License-Identifier: MIT
"""Which hover card the pointer means, decided with an injected clock."""

import math

from big_gtk_kit.card_geometry import contains, in_triangle, safe_triangle

TRIANGLE_REST_MS = 400  # resting on an anchor that lies on the way to the card
CLOSE_MS = 400
REST_RADIUS = 4  # a hand's tremor still counts as resting


class HoverIntent:
    """Which card the pointer means, decided from where it rests and goes.

    ``boxes(name)`` gives the anchor's box and its card's box (``None`` while
    closed) in one coordinate space; ``show``/``hide`` open and close a card;
    ``later(ms, callback)``/``cancel(id)`` schedule.
    """

    def __init__(self, side, boxes, show, hide, later, cancel, **hooks):
        self.side = side
        self._boxes = boxes
        self._show = show
        self._hide = hide
        self._later = later
        self._cancel = cancel
        self._can_open = hooks.get("can_open") or (lambda _name: True)
        self._is_held = hooks.get("is_held") or (lambda _name: False)
        self.owner = None  # the card the pointer opened
        self.pinned = None  # the card a click or key opened
        self.apex = None  # where the pointer last was on the owner's anchor
        self.suppressed = None  # closed by Escape; reopens once the pointer leaves
        self._rest = None  # (name, point, source)
        self._closing = None

    def pointer(self, name, point):
        """The pointer is at ``point``, over anchor ``name`` or none."""
        if self.pinned is not None:
            return
        if self.suppressed is not None and name != self.suppressed:
            self.suppressed = None
        if self.owner is None:
            if name is None or name == self.suppressed:
                self._cancel_rest()
                return
            self._reach(name, point)
            return
        if name == self.owner:
            self.apex = point
            self._cancel_close()
            self._cancel_rest()
            return
        anchor, card = self._boxes(self.owner)
        on_the_way = (
            card is not None
            and self.apex is not None
            and in_triangle(point, *safe_triangle(self.apex, card, self.side))
        )
        if on_the_way or contains(anchor, point) or (card and contains(card, point)):
            self._cancel_close()
        else:
            self._schedule_close()
        if name is None or name == self.suppressed:
            self._cancel_rest()
            return
        if on_the_way:
            self._rest_on(name, point)
        else:
            self._reach(name, point)

    def left_anchor(self, name):
        if self._rest is not None and self._rest[0] == name:
            self._cancel_rest()
        if self.suppressed == name:
            self.suppressed = None

    def left_window(self):
        self._cancel_rest()
        self._schedule_close()

    def entered_card(self, name):
        if name == self.owner:
            self._cancel_close()
            self._cancel_rest()

    def left_card(self, name):
        if name == self.owner:
            self._schedule_close()

    def open(self, name, apex):
        """Open ``name``'s card as the pointer would, the pointer at ``apex``."""
        self._cancel_rest()
        self._cancel_close()
        if self.owner is not None and self.owner != name:
            self._hide(self.owner)
        self.owner, self.apex = name, apex
        self._show(name)

    def pin(self, name):
        """A click or key opens ``name``'s card; the caller shows it."""
        self._cancel_rest()
        self._cancel_close()
        if self.owner is not None:
            owner, self.owner, self.apex = self.owner, None, None
            self._hide(owner)
        self.pinned = name

    def closed(self, name):
        """The card closed, for whatever reason."""
        if name == self.owner:
            self.owner = self.apex = None
            self._cancel_close()
        if name == self.pinned:
            self.pinned = None

    def escape(self):
        """Close the pointer's card; True when there was one."""
        if self.owner is None:
            return False
        name, self.owner, self.apex = self.owner, None, None
        self._cancel_close()
        self._cancel_rest()
        self.suppressed = name
        self._hide(name)
        return True

    def close_now(self):
        """Close the pointer's card at once, as when the anchors scroll."""
        self._cancel_rest()
        if self.owner is not None:
            name, self.owner, self.apex = self.owner, None, None
            self._cancel_close()
            self._hide(name)

    def cancel_all(self):
        self._cancel_rest()
        self._cancel_close()

    def _reach(self, name, point):
        self._cancel_rest()
        if self._can_open(name):
            self.open(name, point)

    def _rest_on(self, name, point):
        if self._rest is not None:
            rest_name, rest_point, _source = self._rest
            if rest_name == name and math.dist(point, rest_point) < REST_RADIUS:
                return
        self._cancel_rest()
        source = self._later(TRIANGLE_REST_MS, self._rested)
        self._rest = (name, point, source)

    def _rested(self):
        name, point, _source = self._rest
        self._rest = None
        if self.pinned is None and self._can_open(name):
            self.open(name, point)

    def _cancel_rest(self):
        if self._rest is not None:
            self._cancel(self._rest[2])
            self._rest = None

    def _schedule_close(self):
        if self.owner is not None and self._closing is None:
            self._closing = self._later(CLOSE_MS, self._close_by_pointer)

    def _cancel_close(self):
        if self._closing is not None:
            self._cancel(self._closing)
            self._closing = None

    def _close_by_pointer(self):
        self._closing = None
        # A slider dragged outside, or a dropdown open in the card, is in use;
        # the next pointer move outside closes it.
        if self.owner is None or self._is_held(self.owner):
            return
        name, self.owner, self.apex = self.owner, None, None
        self._hide(name)
