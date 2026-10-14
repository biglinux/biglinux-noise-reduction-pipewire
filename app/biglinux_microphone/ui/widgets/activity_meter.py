"""Home spectrum and sample-peak meter, using OBS's medium decay profile."""

from __future__ import annotations

import math
import time

import cairo
import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("PangoCairo", "1.0")
from gi.repository import Adw, Gtk, PangoCairo

from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.utils.i18n import _

# OBS 32.2.2 VolumeMeter: immediate attack, 20 dB / 1.7 s, 20 s peak hold.
DECAY_DB_PER_SECOND = 11.76
PEAK_HOLD_SECONDS = 20.0
WARNING_DB = -20.0
ERROR_DB = -9.0


class ActivityMeter(Gtk.Box):
    """Frequency energy and waveform peak have separate, explicit dB scales.

    Capture supplies bands mapped from -75..-20 dBFS and a sample peak mapped
    from -60..0 dBFS. Only the latter drives the level, colour and held peak.
    """

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        self._bands: list[float] = []
        self._targets: list[float] = []
        self._target_db = self.level_db = self.held_db = -60.0
        self._last_frame = self._last_tick = self._held_at = time.monotonic()
        self._tick_id = 0
        self._reading = Gtk.Label(xalign=0, css_classes=["numeric"])
        self._held = Gtk.Label(xalign=1, hexpand=True, css_classes=["numeric"])
        readings = Gtk.Box(spacing=12)
        readings.append(self._reading)
        readings.append(self._held)
        self.append(readings)
        self._canvas = Gtk.DrawingArea(hexpand=True, content_height=96)
        self._canvas.add_css_class("caption")
        self._canvas.set_accessible_role(Gtk.AccessibleRole.METER)
        self._canvas.update_property(
            [
                Gtk.AccessibleProperty.LABEL,
                Gtk.AccessibleProperty.VALUE_MIN,
                Gtk.AccessibleProperty.VALUE_MAX,
            ],
            [_("Microphone level"), -60.0, 0.0],
        )
        self._canvas.set_draw_func(weak_callback(self, type(self)._draw))
        self.append(self._canvas)
        self.connect("unmap", weak_callback(self, lambda self, _: self.clear()))
        self._show_reading()

    def set_bands(self, bands: list[float]) -> None:
        self._targets = [
            max(0.0, min(1.0, v)) if math.isfinite(v) else 0.0 for v in bands
        ]
        if len(self._bands) != len(self._targets):
            self._bands = [0.0] * len(self._targets)

    def set_value(self, peak: float) -> None:
        if not math.isfinite(peak):
            self.clear()
            return
        self._last_frame = time.monotonic()
        self._target_db = -60.0 + 60.0 * max(0.0, min(1.0, float(peak)))
        self._advance(self._last_frame)
        self._show_reading()
        self._canvas.queue_draw()
        if self.get_mapped() and not self._tick_id:
            self._tick_id = self.add_tick_callback(self._tick)

    def clear(self) -> None:
        if self._tick_id:
            self.remove_tick_callback(self._tick_id)
            self._tick_id = 0
        self._bands = self._targets = []
        self._target_db = self.level_db = self.held_db = -60.0
        self._last_frame = self._last_tick = self._held_at = time.monotonic()
        self._show_reading()
        self._canvas.queue_draw()

    def _advance(self, now: float) -> None:
        elapsed = max(0.0, now - self._last_tick)
        self._last_tick = now
        if now - self._last_frame > 0.5:
            self._target_db = -60.0
            self._targets = [0.0] * len(self._targets)
        self.level_db = max(
            self._target_db, self.level_db - DECAY_DB_PER_SECOND * elapsed
        )
        if self._target_db >= self.held_db or now - self._held_at > PEAK_HOLD_SECONDS:
            self.held_db = self._target_db
            self._held_at = now
        for index, target in enumerate(self._targets):
            current = self._bands[index]
            factor = 1.0 - math.exp(-elapsed / (0.045 if target > current else 0.16))
            self._bands[index] = current + (target - current) * factor

    def _tick(self, _widget, _clock) -> bool:
        now = time.monotonic()
        if now - self._last_tick < 1.0 / 30.0:
            return True
        self._advance(now)
        self._show_reading()
        self._canvas.queue_draw()
        if (
            self.level_db <= -60
            and self.held_db <= -60
            and all(v < 0.001 for v in self._bands)
        ):
            self._tick_id = 0
            return False
        return True

    def _show_reading(self) -> None:
        level = f"{self.level_db:.1f}" if self.level_db > -60 else "≤ −60"
        held = f"{self.held_db:.1f}" if self.held_db > -60 else "≤ −60"
        reading = _("Level: %s dBFS") % level
        peak = _("Peak: %s dBFS") % held
        if self._reading.get_text() != reading or self._held.get_text() != peak:
            self._reading.set_text(reading)
            self._held.set_text(peak)
            self._canvas.update_property(
                [Gtk.AccessibleProperty.VALUE_NOW, Gtk.AccessibleProperty.VALUE_TEXT],
                [self.level_db, f"{reading}; {peak}"],
            )

    def _draw(self, area, cr: cairo.Context, width: int, height: int) -> None:
        if width < 80 or height < 60:
            return
        fg = area.get_color()
        dark = Adw.StyleManager.get_default().get_dark()
        green, yellow, red = (
            ((0.34, 0.89, 0.54), (0.97, 0.89, 0.36), (1.0, 0.48, 0.39))
            if dark
            else ((0.10, 0.50, 0.30), (0.61, 0.42, 0.0), (0.75, 0.11, 0.16))
        )
        left, usable = 3.0, width - 6.0
        for low, high, color in (
            (-60, WARNING_DB, green),
            (WARNING_DB, ERROR_DB, yellow),
            (ERROR_DB, 0, red),
        ):
            x = left + (low + 60) / 60 * usable
            span = (high - low) / 60 * usable
            cr.set_source_rgba(*color, 0.18)
            cr.rectangle(x, 0, span, 10)
            cr.fill()
            filled = max(0, min(high, self.level_db) - low) / 60 * usable
            cr.set_source_rgb(*color)
            cr.rectangle(x, 0, filled, 10)
            cr.fill()
        if self.held_db > -60:
            x = left + (self.held_db + 60) / 60 * usable
            cr.set_source_rgba(fg.red, fg.green, fg.blue, 1)
            cr.rectangle(min(width - 3, x - 1), 0, 2, 13)
            cr.fill()

        def label(text: str, x: float, y: float) -> None:
            layout = area.create_pango_layout(text)
            w, _h = layout.get_pixel_size()
            cr.set_source_rgba(fg.red, fg.green, fg.blue, 0.85)
            cr.move_to(max(0, min(width - w, x - w / 2)), y)
            PangoCairo.show_layout(cr, layout)

        for db in (-60, -40, -20, -9, 0):
            label(str(db), left + (db + 60) / 60 * usable, 12)
        # Frequency bars keep the previous -75..-20 dBFS sensitivity. Their
        # colour comes from the waveform peak, not FFT-bin amplitude.
        _text_width, text_height = area.create_pango_layout("0").get_pixel_size()
        top, bottom = 18.0 + text_height, height - text_height - 4.0
        color = (
            red
            if self.level_db >= ERROR_DB
            else yellow
            if self.level_db >= WARNING_DB
            else green
        )
        for fraction in (0.0, 0.5, 1.0):
            cr.set_source_rgba(fg.red, fg.green, fg.blue, 0.12)
            cr.rectangle(left, bottom - fraction * (bottom - top), usable, 1)
            cr.fill()
        if self._bands:
            step = usable / len(self._bands)
            for index, value in enumerate(self._bands):
                bar_height = value * (bottom - top)
                cr.set_source_rgb(*color)
                cr.rectangle(
                    left + index * step,
                    bottom - bar_height,
                    max(1, step - 3),
                    bar_height,
                )
                cr.fill()
        for hz, text in (
            (50, "50 Hz"),
            (200, "200"),
            (1000, "1k"),
            (5000, "5k"),
            (20000, "20k Hz"),
        ):
            x = left + math.log(hz / 50) / math.log(20000 / 50) * usable
            label(text, x, height - text_height)
