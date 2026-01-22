#!/usr/bin/env python3
"""
A robust, state-managed tooltip helper for GTK4.

On X11 with compositor, the popover-based approach can cause segfaults, so we
fall back to native GTK tooltips on X11 backends.
"""

import contextlib

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib, Gtk

from biglinux_microphone.utils.i18n import _


def _is_x11_backend() -> bool:
    """Check if we're running on X11 backend (not Wayland)."""
    try:
        display = Gdk.Display.get_default()
        if display is None:
            return False
        # Check the display type name to determine backend
        display_type = type(display).__name__
        return "X11" in display_type
    except Exception:
        return False


# =============================================================================
# Tooltip Texts Definition
# =============================================================================

TOOLTIPS = {
    # Main toggle
    "noise_reduction_toggle": _(
        "Enable AI-powered noise reduction using neural network processing.\n"
        "Model: GTCRN (Speech Enhancement Model Requiring Ultralow Computational Resources)\n"
        "Stats: 48.2K parameters, 33.0 MMACs/sec\n\n"
        "This removes background noise like fans, air conditioning, keyboard typing, "
        "and other environmental sounds while preserving voice clarity."
    ),
    # AI Model selection
    "ai_model": _(
        "Select the AI model for noise processing:\n\n"
        "• Low Latency: Faster processing, best for real-time calls\n"
        "• Full Quality: Better noise reduction, slightly more CPU usage"
    ),
    "noise_reduction_strength": _(
        "Controls the intensity of noise reduction.\n\n"
        "TIP: Temporarily disable the Gate to adjust this value. "
        "Re-enable the Gate after finding the ideal intensity level."
    ),
    # Gate Filter section
    "gate_toggle": _(
        "Enable the Silence Filter (Gate).\n\n"
        "The Gate is applied AFTER noise reduction. It eliminates residual sounds "
        "that the filter reduced but did not completely cancel.\n\n"
        "When you stop talking, it completely cuts the audio, ensuring absolute silence."
    ),
    "gate_threshold": _(
        "Sets the minimum volume to activate the microphone.\n\n"
        "• HIGHER values (-20 dB): Only loud speech passes\n"
        "• LOWER values (-50 dB): Soft speech also passes\n\n"
        "Tip: Start at -40 dB and adjust according to your voice."
    ),
    "gate_range": _(
        "How much to silence when you are not talking.\n\n"
        "• LOWER values (-60 dB): Almost total silence\n"
        "• HIGHER values (-20 dB): Gentle reduction\n\n"
        "Use -60 dB for total silence or -40 dB for a more natural transition."
    ),
    "gate_attack": _(
        "The speed at which the microphone opens when you start talking.\n\n"
        "• Fast (10ms): Best for not cutting off the first syllable.\n"
        "• Slow (200ms): Smoother transition (fade-in), but may miss the beginning."
    ),
    "gate_hold": _(
        "Time the microphone remains open after you stop talking.\n\n"
        "Prevents the sound from cutting out between words or brief pauses in speech.\n"
        "Values between 200ms and 500ms are ideal."
    ),
    "gate_release": _(
        "Speed to silence after the hold time.\n\n"
        "Defines a smooth 'fade-out' so the silence cut doesn't sound abrupt."
    ),
    # Voice Effects section
    "stereo_toggle": _(
        "Enable voice effects and stereo processing.\n\n"
        "Transforms your mono microphone into stereo output or applies "
        "professional voice effects for streaming and podcasting."
    ),
    "stereo_mode": _(
        "Choose the voice processing effect:\n\n"
        "• Dual Mono: Copy signal to both channels (stereo output)\n"
        "• Studio: Professional radio voice with compression and presence\n"
        "• Voice Changer: Adjust your voice pitch from deep to sharp"
    ),
    "stereo_width": _(
        "Adjust the intensity of the selected effect.\n\n"
        "• Studio: Controls the amount of compression/presence\n"
        "• Voice Changer: Controls the pitch (Low to High)"
    ),
    # Equalizer section
    "equalizer_toggle": _(
        "Enable the 10-band parametric equalizer.\n\n"
        "Adjust the tonal balance of your voice by boosting or cutting "
        "specific frequency ranges."
    ),
    "equalizer_preset": _(
        "Quick presets for common use cases:\n\n"
        "• Flat: No changes\n"
        "• Voice Boost: Enhance vocal clarity\n"
        "• Podcast: Professional broadcast sound\n"
        "• Warm: Rich, full tone\n"
        "• Bright: Clear, crisp sound"
    ),
    "equalizer_bands": _(
        "Adjust individual frequency bands:\n\n"
        "• Left bands (31-250 Hz): Bass and low frequencies\n"
        "• Middle bands (500-2000 Hz): Voice fundamentals\n"
        "• Right bands (4000-16000 Hz): Brightness and clarity\n\n"
        "Drag sliders up to boost, down to cut."
    ),
    # Advanced toggle
    "advanced_toggle": _(
        "Show additional controls for fine-tuning your audio.\n\n"
        "Includes equalizer, AI model selection, and visualizer options."
    ),
    # Monitor section
    "monitor_toggle": _(
        "Activate audio loopback (Monitor).\n\n"
        "Allows hearing your own processed voice to test adjustments.\n"
        "Note: There will always be a small delay (latency) in the return."
    ),
    "monitor_delay": _(
        "Add an extra delay to the monitor return.\n\n"
        "Useful for checking audio/video sync in recordings "
        "or to avoid mental confusion when speaking and hearing yourself immediately."
    ),
    # Bluetooth section
    "bluetooth_toggle": _(
        "Automatically manage the Bluetooth Headset profile.\n\n"
        "Switches to high quality (A2DP) when only listening,\n"
        "and automatically activates the microphone (HSP/HFP) when needed.\n\n"
        "WARNING: Using the bluetooth microphone (HSP/HFP) significantly reduces\n"
        "the quality of both the audio you hear and the microphone."
    ),
    # Visualizer
    "visualizer_style": _(
        "Choose how the audio visualizer displays:\n\n"
        "• Modern Waves: Smooth wave animation\n"
        "• Retro Bars: Classic equalizer bars\n"
        "• Circular: Radial visualization"
    ),
}


class TooltipHelper:
    """
    Manages a single, reusable Gtk.Popover to display custom tooltips.

    Rationale: This is the canonical implementation. It uses a singleton popover
    to prevent state conflicts. The animation is handled by CSS classes, and the
    fade-in is reliably triggered by hooking into the popover's "map" signal.
    This avoids all race conditions with the GTK renderer.

    On X11, uses native GTK tooltips to avoid segfaults with compositor.
    """

    def __init__(self, tooltips_enabled_callback=None):
        """
        Initialize the tooltip helper.

        Args:
            tooltips_enabled_callback: Optional callable that returns bool
                                      indicating if tooltips are enabled
        """
        self._tooltips_enabled_callback = tooltips_enabled_callback

        # --- State Machine Variables ---
        self.active_widget = None
        self.show_timer_id = None

        # Check if we need to use native tooltips (X11)
        self._use_native_tooltips = _is_x11_backend()

        # CSS provider for tooltip colors
        self._color_css_provider = None

        # On X11, skip popover creation entirely
        if self._use_native_tooltips:
            self.popover = None
            self.label = None
            self.css_provider = None
            return

        # --- The Single, Reusable Popover (Wayland only) ---
        self.popover = Gtk.Popover()
        self.popover.set_autohide(False)
        self.popover.set_has_arrow(False)  # Clean look without arrow
        self.popover.set_position(Gtk.PositionType.TOP)
        # Offset the popover slightly above the widget
        self.popover.set_offset(0, -12)

        self.label = Gtk.Label(
            wrap=True,
            max_width_chars=50,
            margin_start=12,
            margin_end=12,
            margin_top=8,
            margin_bottom=8,
            halign=Gtk.Align.START,
        )
        self.popover.set_child(self.label)

        # --- CSS for Class-Based Animation ---
        self.css_provider = Gtk.CssProvider()
        css = b"""
        .tooltip-popover {
            opacity: 0;
            transition: opacity 250ms ease-in-out;
        }
        .tooltip-popover.visible {
            opacity: 1;
        }
        """
        self.css_provider.load_from_data(css)
        self.popover.add_css_class("tooltip-popover")

        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(),
            self.css_provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
        )

        # Connect to the "map" signal to trigger the fade-in animation.
        self.popover.connect("map", self._on_popover_map)

        # Apply initial tooltip colors based on theme
        GLib.idle_add(self.update_colors)

    def _on_popover_map(self, _popover):
        """Called when the popover is drawn. Adds the .visible class to fade in."""
        if self.popover:
            self.popover.add_css_class("visible")

    def is_enabled(self):
        """Check if tooltips are enabled."""
        if self._tooltips_enabled_callback:
            return self._tooltips_enabled_callback()
        return True  # Default to enabled

    def add_tooltip(self, widget, tooltip_key: str):
        """
        Connect a widget to the tooltip management system.

        Args:
            widget: The GTK widget to add tooltip to
            tooltip_key: Key from TOOLTIPS dictionary
        """
        tooltip_text = TOOLTIPS.get(tooltip_key, "")

        # On X11, use native GTK tooltips
        if self._use_native_tooltips:
            if tooltip_text:
                widget.set_tooltip_text(tooltip_text)
            return

        # Wayland: use custom popover tooltips
        widget.tooltip_key = tooltip_key

        motion_controller = Gtk.EventControllerMotion.new()
        motion_controller.connect("enter", self._on_enter, widget)
        motion_controller.connect("leave", self._on_leave)
        widget.add_controller(motion_controller)

    def _clear_timer(self):
        """Clear any pending show timer."""
        if self.show_timer_id:
            GLib.source_remove(self.show_timer_id)
            self.show_timer_id = None

    def _on_enter(self, _controller, _x, _y, widget):
        """Handle mouse entering a widget with tooltip."""
        if not self.is_enabled() or self.active_widget == widget:
            return

        self._clear_timer()
        self._hide_tooltip()

        self.active_widget = widget
        self.show_timer_id = GLib.timeout_add(250, self._show_tooltip)

    def _on_leave(self, _controller):
        """Handle mouse leaving a widget with tooltip."""
        self._clear_timer()
        if self.active_widget:
            self._hide_tooltip(animate=True)
            self.active_widget = None

    def _show_tooltip(self):
        """Display the tooltip popover."""
        if not self.active_widget or not self.popover:
            return GLib.SOURCE_REMOVE

        # Safety check: ensure widget is still in valid state
        try:
            if (
                not self.active_widget.get_mapped()
                or not self.active_widget.get_visible()
            ):
                self.active_widget = None
                return GLib.SOURCE_REMOVE

            # Check if widget has a valid parent and is in a toplevel
            parent = self.active_widget.get_parent()
            if parent is None:
                self.active_widget = None
                return GLib.SOURCE_REMOVE

            # Check if we can get a native ancestor
            native = self.active_widget.get_native()
            if native is None:
                self.active_widget = None
                return GLib.SOURCE_REMOVE
        except Exception:
            self.active_widget = None
            return GLib.SOURCE_REMOVE

        tooltip_key = getattr(self.active_widget, "tooltip_key", None)
        if not tooltip_key:
            return GLib.SOURCE_REMOVE

        tooltip_text = TOOLTIPS.get(tooltip_key)

        if not tooltip_text:
            return GLib.SOURCE_REMOVE

        try:
            # Configure and place on screen. The popover is initially transparent
            # due to the .tooltip-popover class. The "map" signal will then
            # trigger the animation by adding the .visible class.
            self.label.set_text(tooltip_text)

            # Unparent first if already parented
            if self.popover.get_parent() is not None:
                self.popover.unparent()

            # Ensure clean CSS state before showing
            self.popover.remove_css_class("visible")

            self.popover.set_parent(self.active_widget)
            self.popover.popup()
        except Exception as e:
            print(f"Tooltip error: {e}")
            self.active_widget = None

        self.show_timer_id = None
        return GLib.SOURCE_REMOVE

    def _hide_tooltip(self, animate=False):
        """Hide the tooltip popover."""
        if not self.popover:
            return

        try:
            if not self.popover.is_visible():
                return

            def do_cleanup():
                try:
                    if self.popover:
                        self.popover.popdown()
                        if self.popover.get_parent():
                            self.popover.unparent()
                except Exception:
                    pass
                return GLib.SOURCE_REMOVE

            # This triggers the fade-out animation.
            self.popover.remove_css_class("visible")

            if animate:
                # Wait for animation to finish before cleaning up.
                GLib.timeout_add(200, do_cleanup)
            else:
                do_cleanup()
        except Exception:
            pass

    def update_colors(self):
        """Update tooltip colors based on current GTK/Adwaita theme."""
        # Skip on X11 - using native tooltips
        if self._use_native_tooltips:
            return

        # Detect colors from GTK/Adwaita theme
        try:
            style_manager = Adw.StyleManager.get_default()
            is_dark = style_manager.get_dark()
            if is_dark:
                # Dark theme colors - darker base to compensate for adjustment
                bg_color = "#2a2a2a"
                fg_color = "#ffffff"
            else:
                # Light theme colors
                bg_color = "#fafafa"
                fg_color = "#2e2e2e"
        except Exception:
            # Fallback to dark theme defaults
            bg_color = "#2a2a2a"
            fg_color = "#ffffff"

        # Adjust tooltip background for better contrast
        tooltip_bg = self._adjust_tooltip_background(bg_color)

        # Detect if dark theme for border color
        is_dark_theme = False
        try:
            hex_val = bg_color.lstrip("#")
            r = int(hex_val[0:2], 16)
            g = int(hex_val[2:4], 16)
            b = int(hex_val[4:6], 16)
            luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255
            is_dark_theme = luminance < 0.5
        except (ValueError, IndexError):
            pass

        # Set subtle border color based on theme
        border_color = "#707070" if is_dark_theme else "#a0a0a0"

        # Build CSS
        css_parts = ["popover.tooltip-popover > contents {"]
        css_parts.append(f"    background-color: {tooltip_bg};")
        css_parts.append("    background-image: none;")
        css_parts.append(f"    color: {fg_color};")
        css_parts.append(f"    border: 1px solid {border_color};")
        css_parts.append("    border-radius: 8px;")
        css_parts.append("}")
        css_parts.append(f"popover.tooltip-popover label {{ color: {fg_color}; }}")

        css = "\n".join(css_parts)

        # Get display
        display = Gdk.Display.get_default()
        if not display:
            return

        # Remove existing color provider if any
        if self._color_css_provider:
            with contextlib.suppress(Exception):
                Gtk.StyleContext.remove_provider_for_display(
                    display, self._color_css_provider
                )
            self._color_css_provider = None

        # Add new color provider with highest priority
        provider = Gtk.CssProvider()
        provider.load_from_data(css.encode("utf-8"))
        with contextlib.suppress(Exception):
            Gtk.StyleContext.add_provider_for_display(
                display,
                provider,
                Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION + 100,
            )
            self._color_css_provider = provider

    def _adjust_tooltip_background(self, bg_color: str) -> str:
        """Adjust tooltip background color for better contrast."""
        try:
            hex_val = bg_color.lstrip("#")
            r = int(hex_val[0:2], 16)
            g = int(hex_val[2:4], 16)
            b = int(hex_val[4:6], 16)

            luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255

            if luminance < 0.5:
                # Dark theme - lighten slightly
                adjustment = 50
                r = min(255, r + adjustment)
                g = min(255, g + adjustment)
                b = min(255, b + adjustment)
            else:
                # Light theme - darken slightly
                adjustment = 30
                r = max(0, r - adjustment)
                g = max(0, g - adjustment)
                b = max(0, b - adjustment)

            return f"#{r:02x}{g:02x}{b:02x}"
        except (ValueError, IndexError):
            return bg_color

    def cleanup(self):
        """Call this when the application is shutting down."""
        self._clear_timer()

        if not self.popover:
            return

        try:
            if self.popover.get_parent():
                self.popover.unparent()
        except Exception:
            pass
