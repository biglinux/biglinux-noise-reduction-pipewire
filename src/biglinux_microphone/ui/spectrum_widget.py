#!/usr/bin/env python3
"""
Premium spectrum analyzer widget - Award-winning design.

Features modern, professional audio visualization with:
- Integrated peak meter with numerical display
- Professional typography and spacing
- Smooth animations and gradients
- Calibrated dB scale (-60 to 0dB)
- Clear frequency markers
- Elegant dark theme
"""

from __future__ import annotations

import logging
import math
from typing import TYPE_CHECKING

import cairo
import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Gdk, GLib, Gtk

if TYPE_CHECKING:
    import cairo

logger = logging.getLogger(__name__)

# Constants for spectrum visualization
SPECTRUM_NUM_BANDS = 30
SPECTRUM_HEIGHT = 160  # Taller for better visual impact
SPECTRUM_BAR_SPACING = 3
SPECTRUM_CORNER_RADIUS = 2
SPECTRUM_BG_RADIUS = 12
SPECTRUM_ANIMATION_FPS = 60
SPECTRUM_SMOOTH_FACTOR = 0.45


class SpectrumAnalyzerWidget(Gtk.DrawingArea):
    """
    Premium audio spectrum analyzer with award-winning design.

    Features:
    - 32 frequency bands with smooth animation
    - Integrated peak level meter
    - Professional dB scaling
    - Modern typography
    - Gradient colors matching audio levels
    """

    def __init__(self, num_bands: int = SPECTRUM_NUM_BANDS) -> None:
        """Initialize the premium spectrum analyzer."""
        super().__init__()

        self._num_bands = num_bands
        self._bands = [0.0] * num_bands
        self._target_bands = [0.0] * num_bands
        self._bands = [0.0] * num_bands
        self._target_bands = [0.0] * num_bands
        self._peaks = [0.0] * num_bands
        self._peaks_hold_times = [0] * num_bands

        # Overall peak level for meter
        self._peak_level = 0.0
        self._peak_hold = 0.0
        self._peak_hold_time = 0

        # Animation parameters
        self._smooth_factor = SPECTRUM_SMOOTH_FACTOR
        self._peak_decay = 0.01  # Faster decay (was 0.005, requested faster descent)

        # Premium color scheme - dark with vibrant accents
        self._bg_color = Gdk.RGBA()
        self._bg_color.parse("#0f0f0f")  # Almost black

        self._bar_bg = Gdk.RGBA()
        self._bar_bg.parse("rgba(255,255,255,0.04)")

        # Widget sizing
        self.set_size_request(-1, SPECTRUM_HEIGHT)
        self.set_vexpand(False)
        self.set_hexpand(True)

        # Set up drawing
        self.set_draw_func(self._draw)

        # Animation timer
        self._animation_source: int | None = None
        self._start_animation()

        logger.debug("Premium SpectrumAnalyzerWidget initialized")

    def _start_animation(self) -> None:
        """Start the animation timer."""
        if self._animation_source is None:
            interval_ms = 1000 // SPECTRUM_ANIMATION_FPS
            self._animation_source = GLib.timeout_add(interval_ms, self._animate)

    def _stop_animation(self) -> None:
        """Stop the animation timer."""
        if self._animation_source is not None:
            GLib.source_remove(self._animation_source)
            self._animation_source = None

    def set_bands(self, bands: list[float]) -> None:
        """Set frequency band values."""
        input_count = len(bands)
        if input_count == 0:
            return

        for i in range(self._num_bands):
            if input_count == self._num_bands:
                self._target_bands[i] = max(0.0, min(1.0, bands[i]))
            else:
                src_idx = int(i * input_count / self._num_bands)
                src_idx = min(src_idx, input_count - 1)
                self._target_bands[i] = max(0.0, min(1.0, bands[src_idx]))

    def set_level(self, level: float) -> None:
        """Generate pseudo-spectrum from audio level."""
        level = max(0.0, min(1.0, level))

        for i in range(self._num_bands):
            position = i / self._num_bands

            # Natural voice frequency curve
            if position < 0.15:
                weight = 0.4 + position * 2
            elif position < 0.5:
                weight = 0.8 + 0.2 * math.sin((position - 0.15) * math.pi / 0.35)
            elif position < 0.75:
                weight = 0.6
            else:
                weight = 0.3 - 0.2 * (position - 0.75) / 0.25

            variation = 0.85 + 0.3 * math.sin(i * 0.7 + level * 10)
            self._target_bands[i] = level * weight * variation

    def _animate(self) -> bool:
        """Animate band changes with smooth interpolation."""
        changed = False

        # Update overall peak level
        max_band = max(self._target_bands) if self._target_bands else 0.0
        if max_band > self._peak_level:
            self._peak_level = max_band
            changed = True
        else:
            self._peak_level = max(0, self._peak_level - 0.01)
            if self._peak_level > 0.01:
                changed = True

        # Peak hold with timer
        if max_band > self._peak_hold:
            self._peak_hold = max_band
            self._peak_hold_time = 60  # Hold for ~1 second at 60fps
            changed = True
        elif self._peak_hold_time > 0:
            self._peak_hold_time -= 1
            changed = True
        else:
            self._peak_hold = max(0, self._peak_hold - 0.005)
            if self._peak_hold > 0.01:
                changed = True

        for i in range(self._num_bands):
            # Smooth interpolation for bands
            diff = self._target_bands[i] - self._bands[i]
            if abs(diff) > 0.001:
                self._bands[i] += diff * self._smooth_factor
                changed = True

            # Update peaks
            # Update peaks with hold logic
            if self._bands[i] > self._peaks[i]:
                self._peaks[i] = self._bands[i]
                self._peaks_hold_times[i] = 40  # Hold for ~0.7s
                changed = True
            elif self._peaks_hold_times[i] > 0:
                self._peaks_hold_times[i] -= 1
                changed = True
            else:
                self._peaks[i] = max(0, self._peaks[i] - self._peak_decay)

        if changed or any(p > 0.01 for p in self._peaks):
            self.queue_draw()

        return True

    def _draw(
        self,
        _area: Gtk.DrawingArea,
        cr: cairo.Context,
        width: int,
        height: int,
    ) -> None:
        """Draw the premium spectrum analyzer."""
        # Draw background
        cr.set_source_rgba(
            self._bg_color.red,
            self._bg_color.green,
            self._bg_color.blue,
            1.0,
        )
        self._draw_rounded_rect(cr, 0, 0, width, height, SPECTRUM_BG_RADIUS)
        cr.fill()

        # Layout constants
        padding = 12
        meter_height = 42  # Peak meter at top (increased for labels)
        freq_height = 18  # Frequency labels at bottom

        spectrum_y = padding + meter_height + 8
        spectrum_height = height - spectrum_y - freq_height - padding
        spectrum_width = width - (padding * 2)

        # Draw peak meter at top
        self._draw_peak_meter(cr, padding, padding, spectrum_width, meter_height)

        # Calculate bar dimensions
        total_spacing = SPECTRUM_BAR_SPACING * (self._num_bands - 1)
        bar_width = max(2, (spectrum_width - total_spacing) / self._num_bands)

        # Create vertical Zone Gradients (Bottom to Top)
        def create_bar_gradient(alpha_mult: float) -> cairo.LinearGradient:
            g = cairo.LinearGradient(0, spectrum_y + spectrum_height, 0, spectrum_y)
            # Green Zone (darker/professional green)
            # R=0, G=0.6, B=0
            g.add_color_stop_rgba(0.0, 0.0, 0.6 * alpha_mult, 0.0, 1.0)
            g.add_color_stop_rgba(0.75, 0.0, 0.6 * alpha_mult, 0.0, 1.0)

            # Orange Zone
            g.add_color_stop_rgba(0.7501, 1.0 * alpha_mult, 0.6 * alpha_mult, 0.0, 1.0)
            g.add_color_stop_rgba(0.875, 1.0 * alpha_mult, 0.6 * alpha_mult, 0.0, 1.0)

            # Red Zone
            g.add_color_stop_rgba(0.8751, 1.0 * alpha_mult, 0.0, 0.0, 1.0)
            g.add_color_stop_rgba(1.0, 1.0 * alpha_mult, 0.0, 0.0, 1.0)
            return g

        bg_gradient = create_bar_gradient(0.2)
        fg_gradient = create_bar_gradient(1.0)

        # Draw spectrum bars
        for i in range(self._num_bands):
            x = padding + i * (bar_width + SPECTRUM_BAR_SPACING)
            level = self._bands[i]
            peak = self._peaks[i]

            # 1. Background Bar (Dark Track)
            cr.set_source(bg_gradient)
            self._draw_rounded_rect(
                cr, x, spectrum_y, bar_width, spectrum_height, SPECTRUM_CORNER_RADIUS
            )
            cr.fill()

            # 2. Active Bar
            if level > 0.003:
                bar_height = max(2, spectrum_height * level)
                y = spectrum_y + spectrum_height - bar_height

                cr.set_source(fg_gradient)
                self._draw_rounded_rect(
                    cr, x, y, bar_width, bar_height, SPECTRUM_CORNER_RADIUS
                )
                cr.fill()

            # 3. Cuts (Segmented look) - every 10dB
            # Widget BG color to cut through
            cr.set_source_rgba(
                self._bg_color.red, self._bg_color.green, self._bg_color.blue, 1.0
            )
            cr.set_line_width(1.0)

            # Iterate -70dB to -10dB (0.125 to 0.875)
            for step in range(1, 8):
                ratio = step / 8.0  # 0.125, 0.25, ...
                cut_y = spectrum_y + spectrum_height * (1 - ratio)

                # Draw cut line across this bar
                cr.move_to(x, cut_y)
                cr.line_to(x + bar_width, cut_y)
                cr.stroke()

            # Peak indicator (Sticky Peak)
            if peak > 0.02:
                peak_y = spectrum_y + spectrum_height - (spectrum_height * peak)
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.9)
                cr.rectangle(x, peak_y - 0.5, bar_width, 1.5)
                cr.fill()

        # Draw frequency labels
        self._draw_frequency_labels(
            cr, padding, spectrum_y + spectrum_height + 4, spectrum_width, freq_height
        )

        # Draw subtle grid lines for dB levels
        self._draw_db_grid(cr, padding, spectrum_y, spectrum_width, spectrum_height)

    def _draw_peak_meter(
        self, cr: cairo.Context, x: float, y: float, width: float, height: float
    ) -> None:
        """Draw integrated peak meter with numerical display."""
        # Convert peak level (0-1) to dB (-80 to 0)
        peak_ratio = self._peak_level if self._peak_level > 0.0001 else 0.0
        db_value = -80 + (peak_ratio * 80)
        db_value = max(-80, min(0, db_value))

        # Convert peak hold (0-1) to dB
        hold_ratio = self._peak_hold if self._peak_hold > 0.0001 else 0.0
        db_hold = -80 + (hold_ratio * 80)
        db_hold = max(-80, min(0, db_hold))

        # Limits for colors
        val_color = (0.2, 0.7, 0.2, 1.0)  # Darker/Professional Green
        if db_value > -3:
            val_color = (1.0, 0.2, 0.2, 1.0)  # Red
        elif db_value > -10:
            val_color = (1.0, 0.7, 0.0, 1.0)  # Orange

        # Peak hold color
        hold_color = (0.6, 0.6, 0.6, 1.0)  # Grey default
        if db_hold > -3:
            hold_color = (1.0, 0.2, 0.2, 1.0)
        elif db_hold > -10:
            hold_color = (1.0, 0.7, 0.0, 1.0)

        # Draw labels
        cr.select_font_face(
            "sans-serif", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_NORMAL
        )
        cr.set_font_size(9)
        cr.set_source_rgba(0.6, 0.6, 0.6, 1.0)

        # Draw separate small labels
        cr.move_to(x, y + 10)
        cr.show_text("VAL / PEAK")

        # Draw numerical values (Value | Peak)
        cr.select_font_face(
            "monospace", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD
        )
        cr.set_font_size(15)

        # Draw first value (Instant)
        val_text = f"{db_value:+.1f}"
        cr.set_source_rgba(*val_color)
        cr.move_to(x, y + 28)
        cr.show_text(val_text)

        # Divider
        val_ext = cr.text_extents(val_text)
        div_x = x + val_ext.x_advance + 5
        cr.set_source_rgba(0.4, 0.4, 0.4, 1.0)
        cr.move_to(div_x, y + 28)
        cr.show_text("|")

        # Draw second value (Hold)
        div_ext = cr.text_extents("|")
        hold_x = div_x + div_ext.x_advance + 5
        hold_text = f"{db_hold:+.1f} dB"
        cr.set_source_rgba(*hold_color)
        cr.move_to(hold_x, y + 28)
        cr.show_text(hold_text)

        # Calculate total text width for meter position
        hold_ext = cr.text_extents(hold_text)
        total_text_width = hold_x + hold_ext.x_advance - x

        # Draw meter bar (Refined Style)
        meter_x = x + total_text_width + 20
        meter_width = width - total_text_width - 25
        meter_bar_height = 8

        # --- create shared gradients ---
        # We need two gradients: one bright (foreground), one dark (background)
        # Both share the exact same stops for perfect alignment

        # Stops for zones: -80..0 map to 0..1
        # -20dB = (-20 - (-80)) / 80 = 60/80 = 0.75
        # -10dB = (-10 - (-80)) / 80 = 70/80 = 0.875

        def create_meter_gradient(alpha_mult: float) -> cairo.LinearGradient:
            g = cairo.LinearGradient(meter_x, 0, meter_x + meter_width, 0)
            # Green Zone (darker/professional green instead of neon)
            # R=0, G=0.6, B=0
            g.add_color_stop_rgba(0.0, 0.0, 0.6 * alpha_mult, 0.0, 1.0)
            g.add_color_stop_rgba(0.75, 0.0, 0.6 * alpha_mult, 0.0, 1.0)

            # Orange Zone
            g.add_color_stop_rgba(0.7501, 1.0 * alpha_mult, 0.6 * alpha_mult, 0.0, 1.0)
            g.add_color_stop_rgba(0.875, 1.0 * alpha_mult, 0.6 * alpha_mult, 0.0, 1.0)

            # Red Zone
            g.add_color_stop_rgba(0.8751, 1.0 * alpha_mult, 0.0, 0.0, 1.0)
            g.add_color_stop_rgba(1.0, 1.0 * alpha_mult, 0.0, 0.0, 1.0)
            return g

        # 1. Background (Darker shade of the same zones)
        # Use a multiplier (e.g., 0.2) to make it "dark green", "dark orange", etc.
        bg_gradient = create_meter_gradient(0.2)
        cr.set_source(bg_gradient)
        self._draw_rounded_rect(cr, meter_x, y + 12, meter_width, meter_bar_height, 4)
        cr.fill()

        # 2. Active Foreground (Bright)
        active_width = ((db_value + 80) / 80) * meter_width

        # Clamp active width to avoid drawing errors
        active_width = max(0, min(meter_width, active_width))

        if active_width > 1:
            fg_gradient = create_meter_gradient(1.0)
            cr.set_source(fg_gradient)
            self._draw_rounded_rect(
                cr, meter_x, y + 12, active_width, meter_bar_height, 4
            )
            cr.fill()

        # 3. Ruler Marks (every 10dB)
        cr.set_line_width(1.0)

        # Setup font for ruler labels
        cr.select_font_face(
            "sans-serif", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_NORMAL
        )
        cr.set_font_size(9)

        for db in range(-70, 0, 10):  # -70, -60, ... -10
            # Map dB to x position
            # -80 is x=0, 0 is x=1
            ratio = (db + 80) / 80
            tick_x = meter_x + (ratio * meter_width)

            # Draw tick
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.5)
            cr.move_to(tick_x, y + 12)
            cr.line_to(tick_x, y + 12 + meter_bar_height)
            cr.stroke()

            # Draw Label
            label = str(db)
            extents = cr.text_extents(label)

            # Center text below the tick
            text_x = tick_x - (extents.width / 2)
            # y + 12 (top of bar) + 8 (bar height) + 10 (padding)
            text_y = y + 12 + meter_bar_height + 10

            cr.set_source_rgba(0.6, 0.6, 0.6, 0.8)
            cr.move_to(text_x, text_y)
            cr.show_text(label)

        # Peak hold indicator
        if self._peak_hold > 0.01:
            hold_ratio = self._peak_hold
            hold_x = meter_x + (hold_ratio * meter_width)

            # Ensure it stays within bounds
            hold_x = max(meter_x, min(meter_x + meter_width, hold_x))

            cr.set_source_rgba(1.0, 1.0, 1.0, 0.9)
            cr.rectangle(hold_x - 1, y + 10, 2, 12)
            cr.fill()

    def _draw_db_grid(
        self, cr: cairo.Context, x: float, y: float, width: float, height: float
    ) -> None:
        """Draw subtle dB reference lines and labels."""
        # Standard dB levels (every 20dB)
        db_marks = [-20, -40, -60]

        cr.set_line_width(0.5)

        # Setup font for right-side labels
        cr.select_font_face(
            "sans-serif", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_NORMAL
        )
        cr.set_font_size(9)

        for db in db_marks:
            # Map -80 to 0 dB as 0 to 1
            position = (db + 80) / 80
            line_y = y + height * (1 - position)

            # Draw line
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.08)
            cr.move_to(x, line_y)
            cr.line_to(x + width, line_y)
            cr.stroke()

            # Draw label on the right
            label = str(db)
            extents = cr.text_extents(label)

            # Position at far right, slightly above line
            text_x = x + width - extents.width - 2
            text_y = line_y - 2

            cr.set_source_rgba(0.6, 0.6, 0.6, 0.6)
            cr.move_to(text_x, text_y)
            cr.show_text(label)

    def _draw_frequency_labels(
        self, cr: cairo.Context, x: float, y: float, width: float, height: float
    ) -> None:
        """Draw clean frequency labels."""
        # Professional frequency markers
        freq_markers = [
            (0, "60 Hz"),
            (7, "250 Hz"),
            (15, "1 kHz"),
            (24, "4 kHz"),
            (29, "10.5 kHz"),
        ]

        cr.select_font_face(
            "sans-serif", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_NORMAL
        )
        cr.set_font_size(9)
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.9)

        total_spacing = SPECTRUM_BAR_SPACING * (self._num_bands - 1)
        bar_width = (width - total_spacing) / self._num_bands

        for band_idx, label in freq_markers:
            band_x = x + band_idx * (bar_width + SPECTRUM_BAR_SPACING) + bar_width / 2

            text_ext = cr.text_extents(label)
            cr.move_to(band_x - text_ext.width / 2, y + 12)
            cr.show_text(label)

    @staticmethod
    def _draw_rounded_rect(
        cr: cairo.Context, x: float, y: float, w: float, h: float, r: float
    ) -> None:
        """Draw a rounded rectangle path."""
        r = min(r, w / 2, h / 2)
        if r < 1:
            cr.rectangle(x, y, w, h)
            return

        cr.move_to(x + r, y)
        cr.line_to(x + w - r, y)
        cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
        cr.line_to(x + w, y + h - r)
        cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
        cr.line_to(x + r, y + h)
        cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
        cr.line_to(x, y + r)
        cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
        cr.close_path()

    def reset(self) -> None:
        """Reset all bands to zero."""
        self._bands = [0.0] * self._num_bands
        self._target_bands = [0.0] * self._num_bands
        self._peaks = [0.0] * self._num_bands
        self._peaks_hold_times = [0] * self._num_bands
        self._peak_level = 0.0
        self._peak_hold = 0.0
        self.queue_draw()

    def do_destroy(self) -> None:
        """Clean up when widget is destroyed."""
        self._stop_animation()
