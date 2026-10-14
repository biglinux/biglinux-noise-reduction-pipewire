"""Per-frequency residual-noise threshold editor used only by DFN3.

The backend stores ten dB anchors.  The editor draws live spectrum activity
behind the line so the control communicates its real meaning: audio below the
line is muted.  Pointer dragging is optional; keyboard users can focus each
anchor through the companion scales created by :class:`ThresholdEditor`.
"""

from __future__ import annotations

import math
from collections.abc import Callable

import gi

gi.require_version("Gtk", "4.0")

import cairo
from gi.repository import Gdk, Gtk

from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.utils.i18n import _

ANCHORS_HZ = (60, 125, 250, 500, 1000, 2000, 4000, 8000, 12000, 16000)
MIN_DB = -60.0
MAX_DB = 0.0


class _ThresholdCanvas(Gtk.DrawingArea):
    def __init__(self, on_curve: Callable[[list[float]], None]) -> None:
        super().__init__()
        self._curve = [-60.0] * len(ANCHORS_HZ)
        self._bands = [0.0] * 30
        self._on_curve = on_curve
        self._drag = -1
        self.set_content_height(190)
        self.set_hexpand(True)
        self.set_draw_func(weak_callback(self, type(self)._draw))

        click = Gtk.GestureClick(button=1)
        click.connect("pressed", weak_callback(self, type(self)._pressed))
        click.connect("released", weak_callback(self, type(self)._released))
        self.add_controller(click)
        motion = Gtk.EventControllerMotion()
        motion.connect("motion", weak_callback(self, type(self)._motion))
        self.add_controller(motion)

    def set_curve(self, curve: list[float]) -> None:
        values = [max(MIN_DB, min(MAX_DB, float(v))) for v in curve[: len(ANCHORS_HZ)]]
        if len(values) < len(ANCHORS_HZ):
            values += [-60.0] * (len(ANCHORS_HZ) - len(values))
        self._curve = values
        self.queue_draw()

    def set_bands(self, bands: list[float]) -> None:
        self._bands = [max(0.0, min(1.0, float(v))) for v in bands[:30]]
        self.queue_draw()

    def _geom(self) -> tuple[float, float, float, float]:
        width = float(self.get_width())
        height = float(self.get_height())
        return 38.0, 12.0, max(1.0, width - 50.0), max(1.0, height - 42.0)

    def _anchor_x(self, index: int, left: float, width: float) -> float:
        # The anchors are logarithmic in frequency, like hearing and like the FFT
        lo = math.log10(ANCHORS_HZ[0])
        hi = math.log10(ANCHORS_HZ[-1])
        t = (math.log10(ANCHORS_HZ[index]) - lo) / (hi - lo)
        return left + t * width

    @staticmethod
    def _y_from_db(db: float, top: float, height: float) -> float:
        t = (db - MIN_DB) / (MAX_DB - MIN_DB)
        return top + height * (1.0 - t)

    @staticmethod
    def _db_from_y(y: float, top: float, height: float) -> float:
        t = 1.0 - max(0.0, min(1.0, (y - top) / height))
        return MIN_DB + t * (MAX_DB - MIN_DB)

    def _nearest(self, x: float) -> int:
        left, _top, width, _height = self._geom()
        return min(
            range(len(ANCHORS_HZ)),
            key=lambda i: abs(self._anchor_x(i, left, width) - x),
        )

    def _pressed(self, _gesture, _n, x: float, y: float) -> None:
        self._drag = self._nearest(x)
        self._move(self._drag, y)

    def _motion(self, _controller, _x: float, y: float) -> None:
        if self._drag >= 0:
            self._move(self._drag, y)

    def _released(self, *_args) -> None:
        self._drag = -1

    def _move(self, index: int, y: float) -> None:
        _left, top, _width, height = self._geom()
        value = round(self._db_from_y(y, top, height), 1)
        if value == self._curve[index]:
            return
        self._curve[index] = value
        self.queue_draw()
        self._on_curve(list(self._curve))

    def _color(
        self, name: str, fallback: tuple[float, float, float, float]
    ) -> Gdk.RGBA:
        ok, rgba = self.get_style_context().lookup_color(name)
        if ok:
            return rgba
        out = Gdk.RGBA()
        out.red, out.green, out.blue, out.alpha = fallback
        return out

    def _draw(self, _area, cr: cairo.Context, width: int, height: int) -> None:
        left, top, chart_w, chart_h = self._geom()
        fg = self._color("window_fg_color", (0.20, 0.20, 0.22, 1.0))
        accent = self._color("accent_color", (0.20, 0.45, 0.95, 1.0))
        success = self._color("success_color", (0.18, 0.70, 0.37, 1.0))

        # Horizontal dB guides.
        cr.set_line_width(1.0)
        for db in (0, -20, -40, -60):
            y = self._y_from_db(db, top, chart_h)
            cr.set_source_rgba(fg.red, fg.green, fg.blue, 0.10)
            cr.move_to(left, y)
            cr.line_to(left + chart_w, y)
            cr.stroke()
            cr.set_source_rgba(fg.red, fg.green, fg.blue, 0.56)
            cr.select_font_face(
                "Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_NORMAL
            )
            cr.set_font_size(10)
            cr.move_to(2, y + 4)
            cr.show_text(f"{db} dB")

        # Live spectrum bars (normalised 0..1 maps to -60..0 dB for the editor).
        if self._bands:
            n = len(self._bands)
            gap = 2.0
            bar_w = max(2.0, (chart_w - gap * (n - 1)) / n)
            for i, level in enumerate(self._bands):
                db = MIN_DB + level * (MAX_DB - MIN_DB)
                y = self._y_from_db(db, top, chart_h)
                x = left + i * (bar_w + gap)
                cr.rectangle(x, y, bar_w, top + chart_h - y)
                cr.set_source_rgba(success.red, success.green, success.blue, 0.38)
                cr.fill()

        # Threshold line and handles.
        points: list[tuple[float, float]] = []
        for i, db in enumerate(self._curve):
            points.append(
                (self._anchor_x(i, left, chart_w), self._y_from_db(db, top, chart_h))
            )
        cr.set_line_width(2.5)
        cr.set_source_rgba(accent.red, accent.green, accent.blue, 1.0)
        for i, (x, y) in enumerate(points):
            if i == 0:
                cr.move_to(x, y)
            else:
                cr.line_to(x, y)
        cr.stroke()
        for x, y in points:
            cr.arc(x, y, 5.0, 0, 2 * math.pi)
            cr.set_source_rgba(accent.red, accent.green, accent.blue, 1.0)
            cr.fill_preserve()
            cr.set_line_width(2.0)
            cr.set_source_rgba(1, 1, 1, 0.95)
            cr.stroke()

        # Sparse frequency labels.
        for i in (0, 2, 4, 6, 7, 9):
            hz = ANCHORS_HZ[i]
            text = f"{hz // 1000} kHz" if hz >= 1000 else f"{hz} Hz"
            x = self._anchor_x(i, left, chart_w)
            cr.set_source_rgba(fg.red, fg.green, fg.blue, 0.58)
            cr.set_font_size(10)
            ext = cr.text_extents(text)
            cr.move_to(
                max(left, min(left + chart_w - ext.width, x - ext.width / 2)),
                top + chart_h + 18,
            )
            cr.show_text(text)


class ThresholdEditor(Gtk.Box):
    """Canvas plus keyboard-accessible anchor controls."""

    def __init__(self, on_changed: Callable[[list[float]], None]) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self._on_changed = on_changed
        self._loading = False
        self._curve = [-60.0] * len(ANCHORS_HZ)
        self.canvas = _ThresholdCanvas(weak_callback(self, type(self)._canvas_changed))
        self.append(self.canvas)

        # Compact horizontal strip of native scales gives keyboard and AT users
        # a complete alternative to dragging points on the custom canvas.
        scroller = Gtk.ScrolledWindow(hexpand=True)
        scroller.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.NEVER)
        row = Gtk.Box(spacing=8)
        self._scales: list[Gtk.Scale] = []
        for i, hz in enumerate(ANCHORS_HZ):
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
            scale = Gtk.Scale.new_with_range(
                Gtk.Orientation.VERTICAL, MIN_DB, MAX_DB, 1
            )
            scale.set_inverted(True)
            scale.set_draw_value(False)
            scale.set_size_request(54, 82)
            scale.set_value(MIN_DB)
            label = f"{hz // 1000}k" if hz >= 1000 else str(hz)
            scale.update_property(
                [Gtk.AccessibleProperty.LABEL], [_("%s Hz threshold (dB)") % str(hz)]
            )
            scale.connect(
                "value-changed", weak_callback(self, type(self)._scale_changed), i
            )
            box.append(scale)
            box.append(Gtk.Label(label=label, css_classes=["caption", "dim-label"]))
            row.append(box)
            self._scales.append(scale)
        scroller.set_child(row)
        self.append(scroller)

    def set_curve(self, curve: list[float]) -> None:
        self._loading = True
        try:
            values = [
                max(MIN_DB, min(MAX_DB, float(v))) for v in curve[: len(ANCHORS_HZ)]
            ]
            if len(values) < len(ANCHORS_HZ):
                values += [MIN_DB] * (len(ANCHORS_HZ) - len(values))
            self._curve = values
            self.canvas.set_curve(values)
            for scale, value in zip(self._scales, values, strict=True):
                scale.set_value(value)
        finally:
            self._loading = False

    def set_bands(self, bands: list[float]) -> None:
        self.canvas.set_bands(bands)

    def _canvas_changed(self, curve: list[float]) -> None:
        if self._loading:
            return
        self._loading = True
        try:
            self._curve = curve
            for scale, value in zip(self._scales, curve, strict=True):
                scale.set_value(value)
        finally:
            self._loading = False
        self._on_changed(list(curve))

    def _scale_changed(self, scale: Gtk.Scale, index: int) -> None:
        if self._loading:
            return
        self._curve[index] = float(scale.get_value())
        self.canvas.set_curve(self._curve)
        self._on_changed(list(self._curve))
