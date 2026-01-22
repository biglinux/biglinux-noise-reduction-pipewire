#!/usr/bin/env python3
"""
Audio visualization widgets for BigLinux Microphone Settings.

Provides real-time audio level and spectrum visualization components.
"""

from __future__ import annotations

import logging
import math
from typing import TYPE_CHECKING

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Gdk, GLib, Gtk

if TYPE_CHECKING:
    import cairo

    from ..services.audio_monitor import AudioLevels

logger = logging.getLogger(__name__)


class LevelBar(Gtk.DrawingArea):
    """
    Custom audio level bar widget.

    Displays a smooth, animated level meter with peak indicator.
    """

    def __init__(
        self,
        orientation: Gtk.Orientation = Gtk.Orientation.HORIZONTAL,
        show_peak: bool = True,
    ) -> None:
        """
        Initialize the level bar.

        Args:
            orientation: Bar orientation
            show_peak: Whether to show peak indicator
        """
        super().__init__()

        self._orientation = orientation
        self._show_peak = show_peak
        self._level = 0.0
        self._peak = 0.0
        self._target_level = 0.0

        # Animation
        self._smooth_factor = 0.3
        self._peak_hold_ms = 1000
        self._peak_timer: int | None = None

        # Colors
        self._color_low = Gdk.RGBA()
        self._color_low.parse("#4ec9b0")  # Teal
        self._color_mid = Gdk.RGBA()
        self._color_mid.parse("#dcdcaa")  # Yellow
        self._color_high = Gdk.RGBA()
        self._color_high.parse("#f14c4c")  # Red
        self._color_peak = Gdk.RGBA()
        self._color_peak.parse("#ffffff")  # White
        self._color_bg = Gdk.RGBA()
        self._color_bg.parse("rgba(255,255,255,0.1)")

        # Size
        if orientation == Gtk.Orientation.HORIZONTAL:
            self.set_size_request(200, 12)
        else:
            self.set_size_request(12, 100)

        self.set_draw_func(self._draw)

        # Start animation timer
        GLib.timeout_add(16, self._animate)  # ~60fps

    def set_level(self, level: float) -> None:
        """
        Set the current level.

        Args:
            level: Level value (0.0 to 1.0)
        """
        self._target_level = max(0.0, min(1.0, level))

        # Update peak
        if level > self._peak:
            self._peak = level
            # Reset peak timer
            if self._peak_timer is not None:
                GLib.source_remove(self._peak_timer)
            self._peak_timer = GLib.timeout_add(self._peak_hold_ms, self._decay_peak)

    def _decay_peak(self) -> bool:
        """Decay peak indicator."""
        self._peak = self._level
        self._peak_timer = None
        return False

    def _animate(self) -> bool:
        """Animate level changes."""
        # Smooth interpolation
        diff = self._target_level - self._level
        self._level += diff * self._smooth_factor

        # Clamp very small changes
        if abs(diff) < 0.001:
            self._level = self._target_level

        self.queue_draw()
        return True

    def _get_color_at_position(self, pos: float) -> Gdk.RGBA:
        """Get gradient color at position."""
        if pos < 0.6:
            return self._color_low
        elif pos < 0.85:
            # Interpolate between low and mid
            t = (pos - 0.6) / 0.25
            return self._interpolate_color(self._color_low, self._color_mid, t)
        else:
            # Interpolate between mid and high
            t = (pos - 0.85) / 0.15
            return self._interpolate_color(self._color_mid, self._color_high, t)

    @staticmethod
    def _interpolate_color(c1: Gdk.RGBA, c2: Gdk.RGBA, t: float) -> Gdk.RGBA:
        """Interpolate between two colors."""
        result = Gdk.RGBA()
        result.red = c1.red + (c2.red - c1.red) * t
        result.green = c1.green + (c2.green - c1.green) * t
        result.blue = c1.blue + (c2.blue - c1.blue) * t
        result.alpha = 1.0
        return result

    def _draw(
        self,
        _area: Gtk.DrawingArea,
        cr: object,  # Cairo context
        width: int,
        height: int,
    ) -> None:
        """Draw the level bar."""
        # Background
        cr.set_source_rgba(
            self._color_bg.red,
            self._color_bg.green,
            self._color_bg.blue,
            self._color_bg.alpha,
        )
        self._rounded_rect(cr, 0, 0, width, height, 4)
        cr.fill()

        if self._level <= 0:
            return

        # Draw level bar with gradient
        if self._orientation == Gtk.Orientation.HORIZONTAL:
            level_width = int(width * self._level)

            # Draw gradient segments
            segment_width = max(1, level_width // 20)
            for x in range(0, level_width, segment_width):
                pos = x / width
                color = self._get_color_at_position(pos)
                cr.set_source_rgba(color.red, color.green, color.blue, 1.0)

                seg_w = min(segment_width, level_width - x)
                if x == 0:
                    self._rounded_rect_left(cr, x, 1, seg_w, height - 2, 3)
                elif x + seg_w >= level_width:
                    self._rounded_rect_right(cr, x, 1, seg_w, height - 2, 3)
                else:
                    cr.rectangle(x, 1, seg_w, height - 2)
                cr.fill()

            # Peak indicator
            if self._show_peak and self._peak > 0:
                peak_x = int(width * self._peak)
                cr.set_source_rgba(
                    self._color_peak.red,
                    self._color_peak.green,
                    self._color_peak.blue,
                    0.9,
                )
                cr.rectangle(peak_x - 1, 1, 2, height - 2)
                cr.fill()
        else:
            # Vertical orientation
            level_height = int(height * self._level)
            y_start = height - level_height

            for y in range(y_start, height, 4):
                pos = (height - y) / height
                color = self._get_color_at_position(pos)
                cr.set_source_rgba(color.red, color.green, color.blue, 1.0)
                cr.rectangle(1, y, width - 2, min(3, height - y))
                cr.fill()

            # Peak indicator
            if self._show_peak and self._peak > 0:
                peak_y = height - int(height * self._peak)
                cr.set_source_rgba(
                    self._color_peak.red,
                    self._color_peak.green,
                    self._color_peak.blue,
                    0.9,
                )
                cr.rectangle(1, peak_y - 1, width - 2, 2)
                cr.fill()

    @staticmethod
    def _rounded_rect(
        cr: cairo.Context, x: float, y: float, w: float, h: float, r: float
    ) -> None:
        """Draw a rounded rectangle."""
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

    @staticmethod
    def _rounded_rect_left(
        cr: cairo.Context, x: float, y: float, w: float, h: float, r: float
    ) -> None:
        """Draw rectangle with rounded left corners."""
        cr.move_to(x + r, y)
        cr.line_to(x + w, y)
        cr.line_to(x + w, y + h)
        cr.line_to(x + r, y + h)
        cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
        cr.line_to(x, y + r)
        cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
        cr.close_path()

    @staticmethod
    def _rounded_rect_right(
        cr: cairo.Context, x: float, y: float, w: float, h: float, r: float
    ) -> None:
        """Draw rectangle with rounded right corners."""
        cr.move_to(x, y)
        cr.line_to(x + w - r, y)
        cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
        cr.line_to(x + w, y + h - r)
        cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
        cr.line_to(x, y + h)
        cr.close_path()


class SpectrumVisualizerWidget(Gtk.DrawingArea):
    """
    Spectrum analyzer visualization widget.

    Displays audio frequency bands as animated bars.
    """

    def __init__(self, num_bands: int = 10) -> None:
        """
        Initialize the spectrum visualizer.

        Args:
            num_bands: Number of frequency bands
        """
        super().__init__()

        self._num_bands = num_bands
        self._bands = [0.0] * num_bands
        self._target_bands = [0.0] * num_bands

        # Animation
        self._smooth_factor = 0.25

        # Appearance
        self._bar_spacing = 4
        self._corner_radius = 3

        # Colors
        self._color_bar = Gdk.RGBA()
        self._color_bar.parse("#62a0ea")  # Blue
        self._color_bar_high = Gdk.RGBA()
        self._color_bar_high.parse("#f66151")  # Red
        self._color_bg = Gdk.RGBA()
        self._color_bg.parse("rgba(255,255,255,0.05)")

        self.set_size_request(200, 60)
        self.set_draw_func(self._draw)

        # Animation timer
        GLib.timeout_add(16, self._animate)

    def set_bands(self, bands: list[float]) -> None:
        """
        Set frequency band values.

        Args:
            bands: List of band values (0.0 to 1.0)
        """
        for i, value in enumerate(bands[: self._num_bands]):
            self._target_bands[i] = max(0.0, min(1.0, value))

    def set_level(self, level: float) -> None:
        """
        Generate pseudo-spectrum from single level.

        Creates visual variation for display purposes.

        Args:
            level: Input level (0.0 to 1.0)
        """
        import random

        for i in range(self._num_bands):
            # Create variation
            variation = random.uniform(0.6, 1.4)
            # Mid frequencies tend to have more energy
            mid = self._num_bands // 2
            distance_from_mid = abs(i - mid) / mid
            weight = 1.0 - (distance_from_mid * 0.4)

            self._target_bands[i] = level * variation * weight

    def _animate(self) -> bool:
        """Animate band changes."""
        changed = False
        for i in range(self._num_bands):
            diff = self._target_bands[i] - self._bands[i]
            if abs(diff) > 0.001:
                self._bands[i] += diff * self._smooth_factor
                changed = True

        if changed:
            self.queue_draw()
        return True

    def _draw(
        self,
        _area: Gtk.DrawingArea,
        cr: object,
        width: int,
        height: int,
    ) -> None:
        """Draw the spectrum visualizer."""
        # Calculate bar dimensions
        total_spacing = self._bar_spacing * (self._num_bands - 1)
        bar_width = (width - total_spacing) / self._num_bands

        for i, level in enumerate(self._bands):
            x = i * (bar_width + self._bar_spacing)
            bar_height = max(4, height * level)
            y = height - bar_height

            # Color based on level
            color = self._color_bar_high if level > 0.8 else self._color_bar

            # Draw bar background
            cr.set_source_rgba(
                self._color_bg.red,
                self._color_bg.green,
                self._color_bg.blue,
                self._color_bg.alpha,
            )
            self._rounded_rect(cr, x, 0, bar_width, height, self._corner_radius)
            cr.fill()

            # Draw bar
            if level > 0:
                cr.set_source_rgba(
                    color.red,
                    color.green,
                    color.blue,
                    0.8 + (level * 0.2),
                )
                self._rounded_rect(cr, x, y, bar_width, bar_height, self._corner_radius)
                cr.fill()

    @staticmethod
    def _rounded_rect(
        cr: cairo.Context, x: float, y: float, w: float, h: float, r: float
    ) -> None:
        """Draw a rounded rectangle."""
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


class DualLevelMeter(Gtk.Box):
    """
    Dual level meter showing input and output levels.

    Provides visual comparison between original and processed audio.
    """

    def __init__(self) -> None:
        """Initialize the dual level meter."""
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)

        self.add_css_class("dual-level-meter")

        # Input level
        input_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        input_label = Gtk.Label(label="IN")
        input_label.add_css_class("dim-label")
        input_label.set_size_request(24, -1)
        self._input_bar = LevelBar()
        self._input_bar.set_hexpand(True)
        input_box.append(input_label)
        input_box.append(self._input_bar)

        # Output level
        output_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        output_label = Gtk.Label(label="OUT")
        output_label.add_css_class("dim-label")
        output_label.set_size_request(24, -1)
        self._output_bar = LevelBar()
        self._output_bar.set_hexpand(True)
        output_box.append(output_label)
        output_box.append(self._output_bar)

        self.append(input_box)
        self.append(output_box)

    def set_levels(self, input_level: float, output_level: float) -> None:
        """
        Set both input and output levels.

        Args:
            input_level: Input level (0.0 to 1.0)
            output_level: Output level (0.0 to 1.0)
        """
        self._input_bar.set_level(input_level)
        self._output_bar.set_level(output_level)

    def set_audio_levels(self, levels: AudioLevels) -> None:
        """
        Set levels from AudioLevels object.

        Args:
            levels: AudioLevels from audio monitor
        """
        self._input_bar.set_level(levels.input_level)
        self._output_bar.set_level(levels.output_level)


class WaveformWidget(Gtk.DrawingArea):
    """
    Waveform visualization widget.

    Displays audio waveform in real-time.
    """

    def __init__(self, buffer_size: int = 100) -> None:
        """
        Initialize the waveform widget.

        Args:
            buffer_size: Number of samples to display
        """
        super().__init__()

        self._buffer_size = buffer_size
        self._samples: list[float] = [0.0] * buffer_size
        self._sample_index = 0

        # Colors
        self._color_line = Gdk.RGBA()
        self._color_line.parse("#62a0ea")
        self._color_fill = Gdk.RGBA()
        self._color_fill.parse("rgba(98, 160, 234, 0.2)")
        self._color_center = Gdk.RGBA()
        self._color_center.parse("rgba(255, 255, 255, 0.1)")

        self.set_size_request(200, 60)
        self.set_draw_func(self._draw)

    def add_sample(self, value: float) -> None:
        """
        Add a sample to the waveform.

        Args:
            value: Sample value (0.0 to 1.0)
        """
        self._samples[self._sample_index] = value
        self._sample_index = (self._sample_index + 1) % self._buffer_size
        self.queue_draw()

    def set_level(self, level: float) -> None:
        """
        Add level as waveform sample.

        Adds random variation for visualization.

        Args:
            level: Level value (0.0 to 1.0)
        """
        import random

        variation = level * random.uniform(-1, 1)
        self.add_sample(variation)

    def _draw(
        self,
        _area: Gtk.DrawingArea,
        cr: object,
        width: int,
        height: int,
    ) -> None:
        """Draw the waveform."""
        center_y = height / 2

        # Draw center line
        cr.set_source_rgba(
            self._color_center.red,
            self._color_center.green,
            self._color_center.blue,
            self._color_center.alpha,
        )
        cr.set_line_width(1)
        cr.move_to(0, center_y)
        cr.line_to(width, center_y)
        cr.stroke()

        if not any(s != 0 for s in self._samples):
            return

        # Draw waveform
        cr.set_source_rgba(
            self._color_line.red,
            self._color_line.green,
            self._color_line.blue,
            0.8,
        )
        cr.set_line_width(2)

        sample_width = width / self._buffer_size

        # Start from current position in ring buffer
        cr.move_to(0, center_y)

        for i in range(self._buffer_size):
            actual_index = (self._sample_index + i) % self._buffer_size
            sample = self._samples[actual_index]

            x = i * sample_width
            y = center_y - (sample * center_y * 0.9)

            if i == 0:
                cr.move_to(x, y)
            else:
                cr.line_to(x, y)

        cr.stroke()

        # Fill under the curve
        cr.set_source_rgba(
            self._color_fill.red,
            self._color_fill.green,
            self._color_fill.blue,
            self._color_fill.alpha,
        )

        cr.move_to(0, center_y)
        for i in range(self._buffer_size):
            actual_index = (self._sample_index + i) % self._buffer_size
            sample = self._samples[actual_index]

            x = i * sample_width
            y = center_y - (sample * center_y * 0.9)
            cr.line_to(x, y)

        cr.line_to(width, center_y)
        cr.close_path()
        cr.fill()
