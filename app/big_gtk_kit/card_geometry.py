# SPDX-FileCopyrightText: 2026 BigLinux contributors
# SPDX-License-Identifier: MIT
"""Boxes, triangles and outlines of hover cards."""

import math

APEX_BACKOFF = 4  # one jittery sample where the pointer left the anchor
BASE_BUFFER = 8
MARGIN = 8  # between a card and the window's edge
TAB_RADIUS = 12  # a boxed list's corners; the card and the bridge's curves match it
ARROW_WIDTH, ARROW_HEIGHT = 24, 10  # GtkPopover's tail, short of the gap
OVERLAP = 1  # the arrow reaches over the card's outline
INTO_ANCHOR = 4  # the bridge reaches over the container's outline and edge shading
RING = 2  # the outline a bridged card and its anchor draw inside their edges
RING_INSET = 3  # how far inside its edge the anchor's ring lies


# Geometry, in the window's coordinates. A box is (x0, y0, x1, y1).


def contains(box, point):
    x, y = point
    return box[0] <= x <= box[2] and box[1] <= y <= box[3]


def in_triangle(point, a, b, c):
    """Whether ``point`` lies in the triangle ``a b c``; a degenerate one holds
    nothing."""

    def cross(o, p, q):
        return (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0])

    if abs(cross(a, b, c)) < 1e-9:
        return False
    signs = (cross(a, b, point), cross(b, c, point), cross(c, a, point))
    return not (any(s < 0 for s in signs) and any(s > 0 for s in signs))


def safe_triangle(apex, card, side):
    """The triangle from ``apex``, backed off toward the anchor, to the near
    edge of ``card``, which lies on ``side`` ("right", "left", "top" or
    "bottom") of the anchors, widened by a buffer at both ends."""
    x, y = apex
    x0, y0, x1, y1 = card
    b = BASE_BUFFER
    if side == "right":
        return (x - APEX_BACKOFF, y), (x0, y0 - b), (x0, y1 + b)
    if side == "left":
        return (x + APEX_BACKOFF, y), (x1, y0 - b), (x1, y1 + b)
    if side == "top":
        return (x, y + APEX_BACKOFF), (x0 - b, y1), (x1 + b, y1)
    return (x, y - APEX_BACKOFF), (x0 - b, y0), (x1 + b, y0)


def card_box(side, anchor, edge, size, window, margin=MARGIN):
    """The card's box beside ``anchor``, its near edge on ``edge`` (an x for
    "right"/"left", a y for "top"/"bottom"), inside ``window`` by ``margin``.
    Beside the anchors its top lines up with the anchor's, rising as far as
    it must to be seen whole; above or below them it is centered on the
    anchor where it fits. A card taller (or, above a bar, higher) than the
    room is cut to it; the caller scrolls the rest. A card wider than the room
    beside the anchors is narrowed to it."""
    ax0, ay0, ax1, _ay1 = anchor
    width, height = size
    window_width, window_height = window
    if side in ("right", "left"):
        beside = window_width - margin - edge if side == "right" else edge - margin
        width = min(width, max(beside, 0))
        left = edge if side == "right" else edge - width
        height = min(height, max(window_height - 2 * margin, 0))
        top = max(margin, min(ay0, window_height - margin - height))
        return (left, top, left + width, top + height)
    left = (ax0 + ax1) / 2 - width / 2
    left = max(margin, min(left, window_width - margin - width))
    if side == "top":
        height = min(height, max(edge - margin, 0))
        return (left, edge - height, left + width, edge)
    height = min(height, max(window_height - margin - edge, 0))
    return (left, edge, left + width, edge + height)


def _frame(side, anchor, card):
    """Edges and a mapping as if the card were to the right of the anchor: u
    runs from the anchor (its edge ``e``) to the card (its edge ``p``), v
    across (the anchor spans ``t`` to ``b``, the card ``ct`` to ``cb``);
    ``to_xy`` turns (u, v) back."""
    if side == "right":
        e, p = anchor[2], card[0]
        t, b, ct, cb = anchor[1], anchor[3], card[1], card[3]

        def to_xy(u, v):
            return (u, v)
    elif side == "left":
        e, p = -anchor[0], -card[2]
        t, b, ct, cb = anchor[1], anchor[3], card[1], card[3]

        def to_xy(u, v):
            return (-u, v)
    elif side == "top":
        e, p = -anchor[1], -card[3]
        t, b, ct, cb = anchor[0], anchor[2], card[0], card[2]

        def to_xy(u, v):
            return (v, -u)
    else:
        e, p = anchor[3], card[1]
        t, b, ct, cb = anchor[0], anchor[2], card[0], card[2]

        def to_xy(u, v):
            return (v, u)

    return e, p, t, b, ct, cb, to_xy


def arrow_shape(side, anchor, card):
    """The arrow from an open card's near edge toward its anchor's middle,
    kept on the card's straight edge; a polygon as :func:`bridge_shape`
    gives them."""
    _e, p, t, b, ct, cb, to_xy = _frame(side, anchor, card)
    half = ARROW_WIDTH / 2
    mid = max(ct + TAB_RADIUS + half, min((t + b) / 2, cb - TAB_RADIUS - half))
    points = [
        (p + OVERLAP, mid - half),
        (p, mid - half),
        (p - ARROW_HEIGHT, mid),
        (p, mid + half),
        (p + OVERLAP, mid + half),
    ]
    return [("M", *to_xy(*points[0])), *[("L", *to_xy(*q)) for q in points[1:]], ("Z",)]


def bridge_shape(side, ring, card, width=RING):
    """The bridge that joins an open card to the ring its anchor draws:
    ``ring`` is that ring's box, ``RING_INSET`` inside the anchor across it
    and level with the anchor's outer edge. The bridge spans the ring across
    and runs from inside it to the card's near edge; where the card goes on
    beyond the ring it meets it through a concave corner, where its edge is
    level the card's corner is joined (drawn square). Returns ``(polygons,
    edges, joined_start, joined_end, shade)``; each polygon is a list of
    ``("M", x, y)``, ``("L", x, y)``, ``("A", corner_x, corner_y, x, y)`` (a
    quarter circle tangent to the corner) and ``("Z",)``; ``edges`` are the center lines of a ``width`` wide stroke that
    carries the ring along both sides into the card's own, and ``shade`` is
    ``(foot, crest)``: where the bridge leaves the anchor and where it meets
    the card, along its middle."""
    e, p, t, b, ct, cb, to_xy = _frame(side, ring, card)
    gap = p - e + RING_INSET
    top = _corner(gap, t - ct)
    bottom = _corner(gap, cb - b)
    left, right = e - RING_INSET - INTO_ANCHOR, p + width
    o = width / 2

    def side_ops(v, inward, corner, inset):
        """One side, from inside the ring to the card, ``inset`` inside the
        bridge."""
        w = v + inward * inset
        ops = [("M", *to_xy(left, w))]
        if corner:
            end = v - math.copysign(corner, inward)
            ops.append(("L", *to_xy(p - corner, w)))
            ops.append(("A", *to_xy(p + inset, w), *to_xy(p + inset, end)))
        else:
            ops.append(("L", *to_xy(right, w)))
        return ops

    fill = side_ops(t, 1, top, 0)
    if top:
        fill.append(("L", *to_xy(right, t - top)))
    fill.append(("L", *to_xy(right, b + bottom)))
    # The lower side backwards, from the card to inside the ring.
    if bottom:
        fill.append(("L", *to_xy(p, b + bottom)))
        fill.append(("A", *to_xy(p, b), *to_xy(p - bottom, b)))
    fill += [("L", *to_xy(left, b)), ("Z",)]
    edges = [side_ops(t, 1, top, o), side_ops(b, -1, bottom, o)]
    shade = (to_xy(e, (t + b) / 2), to_xy(p, (t + b) / 2))
    return [fill], edges, abs(ct - t) < 0.5, abs(cb - b) < 0.5, shade


def _corner(gap, beyond, radius=TAB_RADIUS):
    """The concave corner where the bridge meets a card that goes on
    ``beyond`` it: never more than the gap it curves in, nor than how far the
    card goes on."""
    return max(min(radius, gap, beyond), 0) if beyond > 0.5 else 0


def shape_bounds(polygons):
    xs = [op[i] for poly in polygons for op in poly for i in range(len(op))[1::2]]
    ys = [op[i] for poly in polygons for op in poly for i in range(len(op))[2::2]]
    return (min(xs), min(ys), max(xs), max(ys))
