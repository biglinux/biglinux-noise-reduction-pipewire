#!/usr/bin/env python3
"""
Audio visualization component for BigLinux Noise Reduction application.

This module provides a GTK4 widget for visualizing audio input from the microphone
with various visualization styles and integrated noise reduction controls.
"""

# Standard library imports
from __future__ import annotations
import math
import random
import asyncio
import os
import gettext
import logging
import threading
import time
from enum import IntEnum, auto
from typing import Optional, Callable, Any, Dict, List, Tuple, Union, cast

# Third-party imports
import numpy as np
import cairo
import gi

# Configure gi versions before importing modules
gi.require_version("Gtk", "4.0")
gi.require_version("Gst", "1.0")
gi.require_version("GstAudio", "1.0")

# Try to import Rsvg, but provide fallback if not available
try:
    gi.require_version("Rsvg", "2.0")
    from gi.repository import Gtk, GLib, Gst, Rsvg

    HAS_RSVG = True
except (ImportError, ValueError):
    from gi.repository import Gtk, GLib, Gst

    HAS_RSVG = False

# Initialize gettext for internationalization
gettext.textdomain("biglinux-noise-reduction-pipewire")
_ = gettext.gettext

# Configure module logger
logging.basicConfig(level=logging.INFO, format="%(levelname)s:%(name)s: %(message)s")
logger = logging.getLogger(__name__)

# Initialize GStreamer
Gst.init(None)

# Type aliases for improved readability
Color = Tuple[float, float, float, float]  # RGBA color
FrequencyData = np.ndarray  # Array of frequency magnitudes


class VisualizationStyle(IntEnum):
    """Enumeration of available visualization styles."""

    MODERN = auto()
    RETRO = auto()
    RADIAL = auto()


class AudioVisualizer(Gtk.Box):
    """
    Audio visualization component using GTK4 with real microphone input via Pipewire.

    This widget displays sound waves in different visualization styles and provides
    controls for noise reduction functionality.
    """

    # Class constants
    DEFAULT_WIDTH: int = 300
    DEFAULT_HEIGHT: int = 130
    VISUALIZATION_UPDATE_INTERVAL: int = 50  # milliseconds
    MICROPHONE_CHECK_INTERVAL: int = 1000  # milliseconds
    STARTUP_GRACE_PERIOD: int = 5000  # milliseconds
    PIPELINE_SETUP_DELAY: int = 1000  # milliseconds
    DEVICE_MONITOR_SETUP_DELAY: int = 3000  # milliseconds
    MAX_RESTART_ATTEMPTS: int = 5

    # Default audio data mapping to improve visualization
    DEFAULT_DATA_MAPPING: Dict[int, int] = {
        0: 15,
        1: 10,
        2: 8,
        3: 9,
        4: 6,
        5: 5,
        6: 2,
        7: 1,
        8: 0,
        9: 4,
        10: 3,
        11: 7,
        12: 11,
        13: 12,
        14: 13,
        15: 14,
    }

    def __init__(self) -> None:
        """Initialize the AudioVisualizer widget."""
        # Initialize as a vertical box container
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)

        # Set initial size
        self.set_size_request(self.DEFAULT_WIDTH, self.DEFAULT_HEIGHT)

        # Initialize state variables
        self.noise_reducer_service: Optional[Any] = None
        self.noise_reduction_active: bool = False
        self.current_style: VisualizationStyle = VisualizationStyle.MODERN
        self.frequency_data: FrequencyData = np.zeros(16)
        self.data_mapping: Dict[int, int] = self.DEFAULT_DATA_MAPPING.copy()
        self.last_spectrum_time: float = time.time()
        self.startup_grace_period: bool = True
        self.non_disruptive_mode: bool = True
        self.current_audio_device: Optional[str] = None
        self.device_monitor: Optional[Gst.DeviceMonitor] = None
        self._pipeline_restart_attempts: int = 0
        self._pipeline: Optional[Gst.Pipeline] = None

        # SVG icon handlers
        self.icon_on: Optional[Rsvg.Handle] = None
        self.icon_off: Optional[Rsvg.Handle] = None
        self._load_svg_icons()

        # Create and configure UI components
        self._setup_ui()

        # Schedule delayed initialization tasks
        GLib.timeout_add(self.PIPELINE_SETUP_DELAY, self._delayed_pipeline_setup)
        GLib.timeout_add(self.DEVICE_MONITOR_SETUP_DELAY, self._setup_device_monitor)
        GLib.timeout_add(self.STARTUP_GRACE_PERIOD, self._end_startup_grace_period)

        # Add recurring update timers
        GLib.timeout_add(self.VISUALIZATION_UPDATE_INTERVAL, self._update_visualization)
        GLib.timeout_add(self.MICROPHONE_CHECK_INTERVAL, self._check_microphone_changes)

    def _setup_ui(self) -> None:
        """Set up the user interface components."""
        # Create the drawing area for visualization
        self.drawing_area = Gtk.DrawingArea()
        self.drawing_area.set_content_width(self.DEFAULT_WIDTH)
        self.drawing_area.set_content_height(100)
        self.drawing_area.set_draw_func(self._on_draw, None)
        self.drawing_area.set_vexpand(True)

        # Add click handling for the drawing area
        click_controller = Gtk.GestureClick.new()
        click_controller.connect("pressed", self._on_click)
        self.drawing_area.add_controller(click_controller)

        # Add the drawing area to our container
        self.append(self.drawing_area)

        # Create style selection buttons
        self._create_style_buttons()

    def _create_style_buttons(self) -> None:
        """Create buttons to switch between visualization styles."""
        # Create a horizontal box for buttons
        button_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        button_box.set_halign(Gtk.Align.CENTER)
        button_box.set_margin_top(4)
        button_box.set_margin_bottom(4)
        button_box.set_size_request(-1, 32)  # Default width, fixed height

        # Create style buttons with proper tooltips
        button_configs = [
            (
                VisualizationStyle.MODERN,
                _("Modern Waves"),
                _("Sound waves with green to red gradient"),
            ),
            (
                VisualizationStyle.RETRO,
                _("Retro Bars"),
                _("Classic equalizer-style visualization"),
            ),
            (
                VisualizationStyle.RADIAL,
                _("Spectrum"),
                _("Spectrum visualizer with green to red gradient"),
            ),
        ]

        for style, label, tooltip in button_configs:
            button = Gtk.Button(label=label)
            button.connect("clicked", self._on_style_button_clicked, style)
            button.set_tooltip_text(tooltip)
            button_box.append(button)

        # Add button box to main container
        self.append(button_box)

    def _on_style_button_clicked(
        self, button: Gtk.Button, style: VisualizationStyle
    ) -> None:
        """
        Handle style button clicks.

        Args:
            button: The button that was clicked
            style: The visualization style to switch to
        """
        self.current_style = style
        self.drawing_area.queue_draw()

    def set_noise_reducer_service(self, service: Any) -> None:
        """
        Connect to a noise reducer service to control status.

        Args:
            service: The noise reducer service object
        """
        self.noise_reducer_service = service
        # Initial status check
        GLib.timeout_add(1000, self._update_noise_reduction_status)

    def _update_noise_reduction_status(self) -> bool:
        """
        Update the noise reduction status asynchronously.

        Returns:
            bool: True to continue the timer, False to stop it
        """
        if not self.noise_reducer_service:
            return True

        async def _check_status_async() -> None:
            """Async helper to check the noise reduction status."""
            try:
                status = await self.noise_reducer_service.get_noise_reduction_status()
                GLib.idle_add(
                    lambda: self._set_noise_reduction_status(status == "enabled")
                )
            except Exception as e:
                logger.error(f"Error checking noise reduction status: {e}")

        # Use a proper async executor instead of manual threading
        asyncio.run_coroutine_threadsafe(
            _check_status_async(), asyncio.get_event_loop()
        )
        return True  # Continue checking periodically

    def _set_noise_reduction_status(self, is_active: bool) -> None:
        """
        Set the noise reduction status and update the icon.

        Args:
            is_active: Whether noise reduction is active
        """
        if self.noise_reduction_active != is_active:
            self.noise_reduction_active = is_active
            self.drawing_area.queue_draw()  # Redraw to show the correct icon

    def _toggle_noise_reduction(self) -> None:
        """Toggle the noise reduction state."""
        if not self.noise_reducer_service:
            return

        async def _toggle_service_async() -> None:
            """Async helper to toggle the noise reduction service."""
            try:
                if self.noise_reduction_active:
                    await self.noise_reducer_service.stop_noise_reduction()
                    GLib.idle_add(lambda: self._set_noise_reduction_status(False))
                else:
                    await self.noise_reducer_service.start_noise_reduction()
                    GLib.idle_add(lambda: self._set_noise_reduction_status(True))
            except Exception as e:
                logger.error(f"Error toggling noise reduction: {e}")

        # Use a proper async executor
        asyncio.run_coroutine_threadsafe(
            _toggle_service_async(), asyncio.get_event_loop()
        )

    def _load_svg_icons(self) -> None:
        """Load the SVG icons for noise reduction status."""
        if not HAS_RSVG:
            return

        try:
            icon_paths = {
                "on": "/usr/share/icons/hicolor/scalable/status/big-noise-reduction-on.svg",
                "off": "/usr/share/icons/hicolor/scalable/status/big-noise-reduction-off.svg",
            }

            for state, path in icon_paths.items():
                if os.path.exists(path):
                    if state == "on":
                        self.icon_on = Rsvg.Handle.new_from_file(path)
                    else:
                        self.icon_off = Rsvg.Handle.new_from_file(path)
                else:
                    logger.warning(f"Icon file not found: {path}")

            if not self.icon_on or not self.icon_off:
                logger.warning("Could not load noise reduction SVG icons")

        except Exception as e:
            logger.error(f"Error loading SVG icons: {e}")

    def _on_click(
        self, controller: Gtk.GestureClick, n_press: int, x: float, y: float
    ) -> None:
        """
        Handle clicks on the drawing area.

        Args:
            controller: The gesture controller
            n_press: Number of presses
            x: X coordinate of the click
            y: Y coordinate of the click
        """
        # Check if the click is within the icon area
        width = self.drawing_area.get_width()
        height = self.drawing_area.get_height()
        center_x = width / 2
        center_y = height / 2
        icon_radius = min(width, height) * 0.2

        # Calculate if click is inside the icon area
        distance = math.sqrt((x - center_x) ** 2 + (y - center_y) ** 2)
        if distance <= icon_radius:
            self._toggle_noise_reduction()

    def _setup_device_monitor(self) -> bool:
        """
        Set up device monitoring to detect microphone changes.

        Returns:
            bool: False to stop the timer
        """
        if self.device_monitor:
            return False  # Already set up

        try:
            # Create a device monitor that watches for audio source changes
            self.device_monitor = Gst.DeviceMonitor.new()

            # Add a filter for audio sources
            try:
                self.device_monitor.add_filter("Audio/Source", None)
            except Exception as e:
                logger.warning(f"Could not add device filter: {e}")

            # Connect to the device monitor bus
            bus = self.device_monitor.get_bus()
            bus.add_signal_watch()
            bus.connect("message", self._on_device_monitor_message)

            # Start monitoring
            if self.device_monitor.start():
                logger.info("Device monitor started successfully")
            else:
                logger.error("Failed to start device monitor")
                return False

        except Exception as e:
            logger.exception(f"Error setting up device monitor: {e}")
            return False

        return False  # Don't repeat this timeout

    def _on_device_monitor_message(self, bus: Gst.Bus, message: Gst.Message) -> bool:
        """
        Handle device monitor messages from the bus.

        Args:
            bus: The GStreamer bus
            message: The message received

        Returns:
            bool: True to keep the handler
        """
        try:
            if message.type == Gst.MessageType.DEVICE_ADDED:
                device = message.parse_device_added()
                logger.info(f"Device added: {device.get_display_name()}")
                self._check_microphone_changes()
            elif message.type == Gst.MessageType.DEVICE_REMOVED:
                device = message.parse_device_removed()
                logger.info(f"Device removed: {device.get_display_name()}")
                self._check_microphone_changes()
        except Exception as e:
            logger.error(f"Error processing device message: {e}")

        return True

    def _get_default_audio_source(self) -> str:
        """
        Get the current default audio source (microphone).

        Returns:
            str: The name of the default audio source
        """
        # First try environment variables
        import os

        if default_source := (
            os.environ.get("PULSE_SOURCE") or os.environ.get("PIPEWIRE_DEFAULT_SOURCE")
        ):
            return default_source

        # Try pactl command
        try:
            import subprocess

            result = subprocess.run(
                ["pactl", "get-default-source"],
                capture_output=True,
                text=True,
                timeout=1,
            )
            if result.returncode == 0 and result.stdout.strip():
                return result.stdout.strip()
        except (subprocess.SubprocessError, FileNotFoundError):
            pass  # Ignore errors from pactl

        # Use GStreamer's device provider
        try:
            device_provider = Gst.DeviceProviderFactory.get_by_name(
                "pulsedeviceprovider"
            )
            if not device_provider:
                return "default"

            devices = device_provider.get_devices()
            if not devices:
                return "default"

            # Find the default source
            for device in devices:
                if device.get_device_class() == "Audio/Source":
                    props = device.get_properties()

                    # Get device name from properties
                    device_name = None
                    for prop_name in ("device.description", "device.name"):
                        if props.has_field(prop_name):
                            device_name = props.get_string(prop_name)
                            if device_name:
                                break

                    # Use display_name as fallback
                    if not device_name:
                        device_name = device.get_display_name()

                    if device_name and "default" in device_name.lower():
                        return device_name

            # Return the first source if we couldn't find the default one
            for device in devices:
                if device.get_device_class() == "Audio/Source":
                    return device.get_display_name()

        except Exception as e:
            logger.error(f"Error getting default audio source: {e}")

        return "default"

    def _check_microphone_changes(self) -> bool:
        """
        Check if the default microphone has changed and restart pipeline if needed.

        Returns:
            bool: True to continue the timer
        """
        if self.startup_grace_period:
            return True  # Skip check during startup

        try:
            new_device = self._get_default_audio_source()

            # If the default device has changed, restart the audio pipeline
            if new_device != self.current_audio_device:
                logger.info(
                    f"Default microphone changed from '{self.current_audio_device}' to '{new_device}'"
                )
                self.current_audio_device = new_device

                # Restart pipeline
                self._setup_audio_pipeline()

        except Exception as e:
            logger.error(f"Error checking microphone changes: {e}")

        return True  # Continue the timeout

    def _end_startup_grace_period(self) -> bool:
        """
        End the startup grace period.

        Returns:
            bool: False to stop the timer
        """
        self.startup_grace_period = False
        logger.info("Startup grace period ended")
        return False  # Run once

    def _delayed_pipeline_setup(self) -> bool:
        """
        Set up audio pipeline with a delay after UI initialization.

        Returns:
            bool: False to stop the timer
        """
        logger.info("Starting delayed pipeline setup")

        # Get default audio device first
        self.current_audio_device = self._get_default_audio_source()
        if self.current_audio_device:
            logger.info(f"Detected default audio device: {self.current_audio_device}")

        # Create pipeline
        self._setup_audio_pipeline()
        return False  # Don't repeat this timeout

    def _setup_audio_pipeline(self) -> None:
        """Set up the GStreamer pipeline for audio capture."""
        try:
            # Stop any existing pipeline
            self._stop_pipeline()

            # Try non-disruptive approach
            if self.non_disruptive_mode:
                self._setup_non_disruptive_pipeline()
                return

            # Try different pipeline options if non-disruptive mode is disabled
            self._try_alternative_pipelines()

        except Exception as e:
            logger.error(f"Error setting up audio pipeline: {e}")

        # Set initial timestamp
        self.last_spectrum_time = time.time()

    def _stop_pipeline(self) -> None:
        """Stop the existing pipeline if it exists."""
        if hasattr(self, "_pipeline") and self._pipeline:
            logger.info("Stopping existing pipeline")
            self._pipeline.set_state(Gst.State.NULL)
            # Reset pipeline reference
            self._pipeline = None

    def _setup_non_disruptive_pipeline(self) -> None:
        """Set up a non-disruptive audio pipeline that uses minimal resources."""
        logger.info("Using non-disruptive audio pipeline")

        # Pipeline using autoaudiosrc with minimal configuration
        pipeline_str = (
            "autoaudiosrc name=src ! "
            "queue ! "
            "audioconvert ! "
            "audioresample ! "
            "audio/x-raw ! "  # Simplified format specification
            "spectrum bands=16 threshold=-80 interval=50000000 post-messages=true ! "
            "fakesink sync=false"
        )

        self._pipeline = Gst.parse_launch(pipeline_str)

        # Configure source element for gentle operation
        if src := self._pipeline.get_by_name("src"):
            self._configure_gentle_source(src)

        # Set up message handling
        self._setup_pipeline_messages()

        # Start the pipeline
        ret = self._pipeline.set_state(Gst.State.PLAYING)
        logger.info(f"Pipeline set to PLAYING state: {ret}")

        # Reset timestamp
        self.last_spectrum_time = time.time()

    def _configure_gentle_source(self, src: Gst.Element) -> None:
        """Configure a source element to be gentle on system resources."""
        try:
            # Set properties that make it use less resources
            for prop_name, value in {
                "do-timestamp": True,
                "buffer-time": 500000,  # 500ms buffer
            }.items():
                if hasattr(src.props, prop_name):
                    src.set_property(prop_name, value)
        except Exception:
            pass  # Ignore if properties don't exist

    def _try_alternative_pipelines(self) -> None:
        """Try different pipeline configurations until one works."""
        pipeline_options = [
            # Option 1: Simplified PipeWire source
            (
                "pipewiresrc ! "
                "audioconvert ! "
                "audio/x-raw ! "
                "spectrum bands=32 threshold=-80 interval=50000000 post-messages=true ! "
                "fakesink sync=false"
            ),
            # Option 2: Use PulseAudio source
            (
                "pulsesrc ! "
                "audioconvert ! "
                "audio/x-raw ! "
                "spectrum bands=32 threshold=-80 interval=50000000 post-messages=true ! "
                "fakesink sync=false"
            ),
            # Option 3: Auto-select audio source (simplest option)
            (
                "autoaudiosrc ! "
                "audioconvert ! "
                "spectrum bands=32 threshold=-80 interval=50000000 post-messages=true ! "
                "fakesink sync=false"
            ),
            # Option 4: ALSA fallback
            (
                "alsasrc ! "
                "audioconvert ! "
                "audio/x-raw ! "
                "spectrum bands=32 threshold=-80 interval=50000000 post-messages=true ! "
                "fakesink sync=false"
            ),
        ]

        # Try each pipeline until one works
        pipeline_success = False

        for i, pipeline_str in enumerate(pipeline_options):
            try:
                logger.info(f"Trying audio pipeline option {i + 1}...")
                self._pipeline = Gst.parse_launch(pipeline_str)

                # Setup message handling
                self._setup_pipeline_messages()

                # Start the pipeline
                if (
                    self._pipeline.set_state(Gst.State.PLAYING)
                    != Gst.StateChangeReturn.FAILURE
                ):
                    logger.info(f"Audio pipeline {i + 1} started successfully")
                    pipeline_success = True

                    # Store the current device
                    self.current_audio_device = self._get_default_audio_source()
                    logger.info(f"Using audio device: {self.current_audio_device}")

                    # Reset restart attempts counter
                    self._pipeline_restart_attempts = 0

                    # Reset timestamp
                    self.last_spectrum_time = time.time()
                    break
                else:
                    self._pipeline.set_state(Gst.State.NULL)
                    logger.error(f"Pipeline {i + 1} failed to start")

            except Exception as e:
                logger.error(f"Error with pipeline {i + 1}: {e}")

        if not pipeline_success:
            logger.error("All audio pipelines failed")
            self._schedule_pipeline_restart()

    def _setup_pipeline_messages(self) -> None:
        """Set up message handling for the pipeline."""
        if not self._pipeline:
            return

        bus = self._pipeline.get_bus()
        bus.add_signal_watch()
        bus.connect("message::element", self._on_message)
        bus.connect("message::error", self._on_error)
        bus.connect("message::state-changed", self._on_state_changed)
        bus.connect("message::eos", self._on_eos)

    def _schedule_pipeline_restart(self) -> None:
        """Schedule a pipeline restart if we haven't exceeded the maximum attempts."""
        if self._pipeline_restart_attempts < self.MAX_RESTART_ATTEMPTS:
            self._pipeline_restart_attempts += 1
            restart_delay = 2000 * self._pipeline_restart_attempts
            logger.info(f"Scheduling pipeline restart in {restart_delay / 1000}s")
            GLib.timeout_add(restart_delay, self._delayed_pipeline_setup)

    def _on_message(self, bus: Gst.Bus, message: Gst.Message) -> bool:
        """
        Handle element messages from GStreamer bus.

        Args:
            bus: The GStreamer bus
            message: The message received

        Returns:
            bool: True if message was handled
        """
        if message.get_structure() and message.get_structure().get_name() == "spectrum":
            try:
                structure = message.get_structure()
                if magnitudes := structure.get_value("magnitude"):
                    self.last_spectrum_time = time.time()
                    self._process_spectrum_data(magnitudes)
                    return True
                else:
                    logger.warning("Received spectrum message without magnitude values")
            except Exception as e:
                logger.error(f"Error processing spectrum message: {e}")

        return False

    def _on_error(self, bus: Gst.Bus, message: Gst.Message) -> None:
        """
        Handle error messages from GStreamer bus.

        Args:
            bus: The GStreamer bus
            message: The error message
        """
        err, debug = message.parse_error()
        logger.error(f"Pipeline error: {err}, Debug: {debug}")

    def _on_state_changed(self, bus: Gst.Bus, message: Gst.Message) -> None:
        """
        Handle state changed messages from the pipeline.

        Args:
            bus: The GStreamer bus
            message: The state change message
        """
        if not hasattr(self, "_pipeline") or message.src != self._pipeline:
            return

        old_state, new_state, pending_state = message.parse_state_changed()
        state_names = {
            Gst.State.NULL: "NULL",
            Gst.State.READY: "READY",
            Gst.State.PAUSED: "PAUSED",
            Gst.State.PLAYING: "PLAYING",
        }
        logger.info(
            f"Pipeline state changed from {state_names.get(old_state)} to {state_names.get(new_state)}"
        )

        if new_state == Gst.State.PLAYING:
            # Reset timestamp to avoid immediate "No spectrum data" message
            self.last_spectrum_time = time.time()
            # Restart grace period when pipeline starts playing
            self.startup_grace_period = True
            GLib.timeout_add(3000, self._end_startup_grace_period)

    def _on_eos(self, bus: Gst.Bus, message: Gst.Message) -> None:
        """
        Handle end-of-stream messages.

        Args:
            bus: The GStreamer bus
            message: The EOS message
        """
        logger.info("End of stream received, attempting to restart pipeline")
        GLib.idle_add(self._delayed_pipeline_setup)

    def _process_spectrum_data(self, magnitudes: List[float]) -> None:
        """
        Process spectrum data from GStreamer message.

        Args:
            magnitudes: The list of magnitude values from the spectrum element
        """
        try:
            # Convert magnitudes to numpy array
            mag_array = np.array(magnitudes)

            # Check if audio is silent
            max_db = np.max(mag_array)
            is_silent = max_db <= -90.0

            # Log occasional debug info to reduce console spam
            if random.random() < 0.05:  # Only ~5% of the time
                min_db = np.min(mag_array)
                logger.debug(
                    f"Audio spectrum range: min={min_db:.1f}dB, max={max_db:.1f}dB"
                )

            if is_silent:
                # For silent audio, use zeros
                self.frequency_data = np.zeros(16)
                return

            # Process and normalize the spectrum data
            self._normalize_spectrum_data(mag_array, is_silent)

        except Exception as e:
            logger.error(f"Error processing spectrum data: {e}")

    def _normalize_spectrum_data(self, mag_array: np.ndarray, is_silent: bool) -> None:
        """
        Normalize the spectrum data for visualization.

        Args:
            mag_array: Raw magnitude array
            is_silent: Whether the audio is currently silent
        """
        # Normalize dB values to a visible range for visualization
        normalized = np.zeros_like(mag_array, dtype=float)

        # Parameters for scaling
        min_db_threshold = -80  # Treat anything below this as silence

        # Apply non-linear scaling to make quiet sounds more visible
        for i in range(len(mag_array)):
            # Limit the range
            mag_db = min(max(mag_array[i], min_db_threshold), 0)

            if mag_db <= min_db_threshold:
                normalized[i] = 0
            else:
                # Non-linear mapping emphasizes changes in the middle range
                normalized_linear = (mag_db - min_db_threshold) / (-min_db_threshold)
                normalized[i] = np.power(
                    normalized_linear, 0.5
                )  # Square root for non-linear emphasis

        # Sample bands from the spectrum for visualization
        if len(normalized) >= 16:
            # Use logarithmically spaced indices to emphasize low frequencies
            indices = np.logspace(0, np.log10(len(normalized) * 0.7), 16).astype(int)
            indices = np.clip(indices, 0, len(normalized) - 1)
            self.frequency_data = normalized[indices]
        else:
            self.frequency_data = normalized[:16]

        # Apply minimal smoothing with slight movement
        if not is_silent:
            for i in range(len(self.frequency_data)):
                if self.frequency_data[i] > 0.01:  # Only add movement to visible bars
                    self.frequency_data[i] += random.uniform(-0.02, 0.02)
                    self.frequency_data[i] = max(0, min(1, self.frequency_data[i]))

    def _update_visualization(self) -> bool:
        """
        Update visualization data and request redraw.

        Returns:
            bool: True to continue the timer
        """
        try:
            # Check if we've received any spectrum data recently
            current_time = time.time()
            time_since_data = current_time - self.last_spectrum_time

            # Handle data timeout conditions
            if not self.startup_grace_period and time_since_data > 3:
                # If no recent data, display empty visualization
                self.frequency_data = np.zeros(16)

                # Try restarting pipeline if we've been without data for too long
                if (
                    self._pipeline_restart_attempts < self.MAX_RESTART_ATTEMPTS
                    and time_since_data > 5
                ):
                    self._pipeline_restart_attempts += 1
                    logger.info(
                        f"No data for {time_since_data:.1f}s, attempting restart"
                    )
                    GLib.idle_add(self._setup_audio_pipeline)

            # Request redraw
            self.drawing_area.queue_draw()

        except Exception as e:
            logger.error(f"Error updating visualization: {e}")

        return True  # Continue the timer

    def _on_draw(
        self,
        area: Gtk.DrawingArea,
        cr: cairo.Context,
        width: int,
        height: int,
        data: Any,
    ) -> None:
        """
        Draw the visualization based on the current style.

        Args:
            area: The drawing area
            cr: The Cairo context
            width: The width of the drawing area
            height: The height of the drawing area
            data: User data (unused)
        """
        try:
            # Clear background with transparency
            cr.set_source_rgba(0, 0, 0, 0)
            cr.paint()

            # Draw based on selected style
            style_handlers = {
                VisualizationStyle.MODERN: self._draw_modern_waves,
                VisualizationStyle.RETRO: self._draw_retro_bars,
                VisualizationStyle.RADIAL: self._draw_radial_spectrum,
            }

            if handler := style_handlers.get(self.current_style):
                handler(cr, width, height)
            else:
                # Fallback to modern style
                self._draw_modern_waves(cr, width, height)

        except Exception as e:
            logger.error(f"Error in on_draw: {e}")
            self._draw_error_message(cr, width, height, str(e))

    def _draw_error_message(
        self, cr: cairo.Context, width: int, height: int, error: str
    ) -> None:
        """
        Draw an error message on the visualization area.

        Args:
            cr: The Cairo context
            width: The width of the drawing area
            height: The height of the drawing area
            error: The error message
        """
        cr.set_source_rgba(0.8, 0.0, 0.0, 0.7)
        cr.select_font_face("Sans", cairo.FontSlant.NORMAL, cairo.FontWeight.BOLD)
        cr.set_font_size(12)
        cr.move_to(10, height / 2)
        cr.show_text(_("Visualization Error"))

    def _draw_modern_waves(self, cr: cairo.Context, width: int, height: int) -> None:
        """
        Draw sound waves with green to yellow to red gradients.

        Args:
            cr: The Cairo context
            width: The width of the drawing area
            height: The height of the drawing area
        """
        center_x = width / 2
        center_y = height / 2

        # Draw background glow
        self._draw_background_glow(cr, center_x, center_y, width)

        # Drawing constants
        wave_thickness = 3
        wave_padding = height * 0.15
        wave_height = height - (wave_padding * 2)
        num_points = 100

        # Create wave gradient
        wave_gradient = cairo.LinearGradient(0, wave_padding, 0, height - wave_padding)
        wave_gradient.add_color_stop_rgba(0, 0.9, 0.2, 0.1, 0.9)  # Red (top)
        wave_gradient.add_color_stop_rgba(0.5, 0.9, 0.9, 0.1, 0.85)  # Yellow (middle)
        wave_gradient.add_color_stop_rgba(1, 0.1, 0.8, 0.1, 0.8)  # Green (bottom)

        # Draw multiple waves with different phases
        for wave_idx in range(3):
            phase_offset = wave_idx * 1.5
            opacity_factor = 0.9 - wave_idx * 0.25

            cr.save()
            cr.new_path()

            # Calculate wave points
            point_spacing = width / num_points
            wave_values = self._calculate_wave_values(
                num_points, wave_idx, phase_offset
            )

            # Draw the wave
            self._draw_wave_path(cr, wave_values, point_spacing, center_y, wave_height)

            # Set line properties and stroke
            cr.set_line_width(wave_thickness * (1.0 - wave_idx * 0.2))
            cr.set_source(wave_gradient)
            cr.set_operator(cairo.OPERATOR_OVER)
            cr.stroke()

            # Add glow effect
            cr.set_source_rgba(0.9, 0.4, 0.1, 0.4 * opacity_factor)
            cr.set_line_width(wave_thickness * 2.5 * (1.0 - wave_idx * 0.2))
            cr.set_operator(cairo.OPERATOR_ADD)
            cr.stroke()

            cr.restore()

        # Draw noise reduction icon
        self._draw_noise_reduction_icon(cr, width, height, "warm")

    def _draw_background_glow(
        self, cr: cairo.Context, center_x: float, center_y: float, width: float
    ) -> None:
        """
        Draw a subtle background glow for visualizations.

        Args:
            cr: The Cairo context
            center_x: X coordinate of the center
            center_y: Y coordinate of the center
            width: Width of the drawing area
        """
        cr.save()
        glow_gradient = cairo.RadialGradient(
            center_x, center_y, 0, center_x, center_y, width / 3
        )
        glow_gradient.add_color_stop_rgba(0, 0.1, 0.3, 0.1, 0.2)  # Green inner glow
        glow_gradient.add_color_stop_rgba(1, 0.0, 0.0, 0.0, 0.0)  # Fade to transparent

        cr.set_source(glow_gradient)
        cr.rectangle(0, 0, width, center_y * 2)
        cr.fill()
        cr.restore()

    def _calculate_wave_values(
        self, num_points: int, wave_idx: int, phase_offset: float
    ) -> List[float]:
        """
        Calculate wave values for rendering with smooth interpolation.

        Args:
            num_points: Number of points to calculate
            wave_idx: Index of the wave (for sizing)
            phase_offset: Phase offset for the wave

        Returns:
            List of calculated wave values
        """
        wave_values = []
        wave_height_factor = 1.0 - wave_idx * 0.15

        for i in range(num_points + 1):
            # Map point index to a frequency index with interpolation
            freq_idx = min(15, int((i / num_points) * 16))
            next_freq_idx = min(15, freq_idx + 1)

            # Interpolation factor
            interp_factor = (i / num_points * 16) - freq_idx

            # Get interpolated value
            value1 = self.frequency_data[self.data_mapping.get(freq_idx, freq_idx)]
            value2 = self.frequency_data[
                self.data_mapping.get(next_freq_idx, next_freq_idx)
            ]
            interp_value = value1 * (1 - interp_factor) + value2 * interp_factor

            # Apply scaling and wave effect
            value = interp_value * wave_height_factor
            mod_factor = math.sin((i / num_points * 10 + phase_offset) * math.pi) * 0.15
            value = value * (1 + mod_factor)

            wave_values.append(value)

        return wave_values

    def _draw_wave_path(
        self,
        cr: cairo.Context,
        wave_values: List[float],
        point_spacing: float,
        center_y: float,
        wave_height: float,
    ) -> None:
        """
        Draw a smooth wave path using the provided values.

        Args:
            cr: The Cairo context
            wave_values: List of wave height values
            point_spacing: Spacing between points
            center_y: Y coordinate of the center
            wave_height: Height of the wave
        """
        # First point
        first_value = wave_values[0]
        first_y = center_y + ((0.5 - first_value) * wave_height)
        cr.move_to(0, first_y)

        # Draw smooth curve through all points
        for i in range(1, len(wave_values)):
            x = i * point_spacing
            value = wave_values[i - 1]
            y = center_y + ((0.5 - value) * wave_height)

            # Use bezier curves for smoother rendering
            if i < len(wave_values) - 1:
                next_value = wave_values[i]
                next_y = center_y + ((0.5 - next_value) * wave_height)

                cp1_x = x - point_spacing * 0.5
                cp1_y = y
                cp2_x = x
                cp2_y = next_y

                cr.curve_to(cp1_x, cp1_y, cp2_x, cp2_y, x + point_spacing, next_y)
            else:
                cr.line_to(x, y)

    def _draw_radial_spectrum(self, cr: cairo.Context, width: int, height: int) -> None:
        """
        Draw a circular spectrum visualizer with bars around the circumference.

        Args:
            cr: The Cairo context
            width: The width of the drawing area
            height: The height of the drawing area
        """
        # Define constants
        center_x = width / 2
        center_y = height / 2
        max_radius = min(width, height) * 0.45
        inner_radius = min(width, height) * 0.2
        bar_count = 48
        max_bar_height = max_radius - inner_radius

        # Draw background glow (reuse from modern waves)
        self._draw_background_glow(cr, center_x, center_y, width)

        # Draw spectrum bars
        for i in range(bar_count):
            freq_idx = i % 16
            value = self.frequency_data[self.data_mapping.get(freq_idx, freq_idx)]

            # Skip drawing if bar has no height
            if value <= 0:
                continue

            # Calculate bar geometry
            angle = (i / bar_count) * 2 * math.pi
            bar_height = value * max_bar_height
            bar_width_degrees = (2 * math.pi) / bar_count * 0.7
            half_width = bar_width_degrees / 2

            # Draw the spectrum bar
            self._draw_spectrum_bar(
                cr,
                center_x,
                center_y,
                angle,
                inner_radius,
                bar_height,
                half_width,
                value,
            )

        # Draw center pulse effect
        self._draw_center_pulse(cr, center_x, center_y, inner_radius)

        # Draw noise reduction icon
        self._draw_noise_reduction_icon(cr, width, height, "warm")

    def _draw_spectrum_bar(
        self,
        cr: cairo.Context,
        center_x: float,
        center_y: float,
        angle: float,
        inner_radius: float,
        bar_height: float,
        half_width: float,
        value: float,
    ) -> None:
        """
        Draw an individual spectrum bar for the radial visualization.

        Args:
            cr: The Cairo context
            center_x: X coordinate of the center
            center_y: Y coordinate of the center
            angle: Angle of the bar (in radians)
            inner_radius: Inner radius of the spectrum
            bar_height: Height of this bar
            half_width: Half of the angular width of the bar
            value: The normalized value (0-1) of this bar
        """
        # Calculate bar coordinates
        outer_radius = inner_radius + bar_height

        # Inner and outer arc endpoints
        inner_start_x = center_x + inner_radius * math.cos(angle - half_width)
        inner_start_y = center_y + inner_radius * math.sin(angle - half_width)
        inner_end_x = center_x + inner_radius * math.cos(angle + half_width)
        inner_end_y = center_y + inner_radius * math.sin(angle + half_width)

        outer_start_x = center_x + outer_radius * math.cos(angle - half_width)
        outer_start_y = center_y + outer_radius * math.sin(angle - half_width)

        # Create bar gradient
        gradient = cairo.LinearGradient(
            center_x + inner_radius * math.cos(angle),
            center_y + inner_radius * math.sin(angle),
            center_x + outer_radius * math.cos(angle),
            center_y + outer_radius * math.sin(angle),
        )

        # Set gradient colors
        gradient.add_color_stop_rgba(0, 0.1, 0.8, 0.1, 0.7)  # Green (inner)
        gradient.add_color_stop_rgba(0.5, 0.9, 0.9, 0.1, 0.8)  # Yellow (middle)
        gradient.add_color_stop_rgba(1, 0.9, 0.1, 0.1, 0.9)  # Red (outer)

        # Draw bar
        cr.save()
        cr.set_source(gradient)

        # Create bar path
        cr.move_to(inner_start_x, inner_start_y)
        cr.line_to(outer_start_x, outer_start_y)
        cr.arc(center_x, center_y, outer_radius, angle - half_width, angle + half_width)
        cr.line_to(inner_end_x, inner_end_y)
        cr.arc_negative(
            center_x, center_y, inner_radius, angle + half_width, angle - half_width
        )
        cr.close_path()
        cr.fill()

        # Add glow effect for high values
        if value > 0.3:
            glow_alpha = value * 0.4
            cr.set_source_rgba(0.9, 0.4, 0.0, glow_alpha)
            cr.set_line_width(2)
            cr.move_to(outer_start_x, outer_start_y)
            cr.arc(
                center_x, center_y, outer_radius, angle - half_width, angle + half_width
            )
            cr.stroke()

        cr.restore()

    def _draw_center_pulse(
        self, cr: cairo.Context, center_x: float, center_y: float, inner_radius: float
    ) -> None:
        """
        Draw a pulsing effect in the center of the radial visualization.

        Args:
            cr: The Cairo context
            center_x: X coordinate of the center
            center_y: Y coordinate of the center
            inner_radius: Inner radius of the spectrum
        """
        # Create pulsing effect
        pulse_size = 1.0 + 0.1 * math.sin(time.time() * 3)

        cr.save()
        glow = cairo.RadialGradient(
            center_x,
            center_y,
            inner_radius * 0.8,
            center_x,
            center_y,
            inner_radius * pulse_size,
        )
        glow.add_color_stop_rgba(0, 0.9, 0.4, 0.0, 0.1)  # Subtle orange glow
        glow.add_color_stop_rgba(1, 0.9, 0.2, 0.0, 0)  # Fade out

        cr.set_source(glow)
        cr.arc(center_x, center_y, inner_radius * pulse_size, 0, 2 * math.pi)
        cr.fill()
        cr.restore()

    def _draw_retro_bars(self, cr: cairo.Context, width: int, height: int) -> None:
        """
        Draw retro style equalizer bars.

        Args:
            cr: The Cairo context
            width: The width of the drawing area
            height: The height of the drawing area
        """
        # Set up dimensions for bars
        num_bars = 16
        bar_width = width / (num_bars * 1.5)
        bar_spacing = (width - (num_bars * bar_width)) / (num_bars + 1)
        max_bar_height = height * 0.75
        padding_bottom = height * 0.15

        # Draw retro background
        self._draw_retro_background(cr, width, height, padding_bottom, max_bar_height)

        # Draw each bar
        for i in range(num_bars):
            value = self.frequency_data[self.data_mapping.get(i, i)]
            bar_height = value * max_bar_height
            x = bar_spacing + i * (bar_width + bar_spacing)
            y = height - padding_bottom - bar_height

            self._draw_retro_bar(
                cr, x, y, bar_width, bar_height, height - padding_bottom, max_bar_height
            )

        # Draw horizontal baseline
        cr.save()
        cr.set_source_rgba(0.2, 0.8, 0.2, 0.7)
        cr.set_line_width(2)
        cr.move_to(0, height - padding_bottom)
        cr.line_to(width, height - padding_bottom)
        cr.stroke()
        cr.restore()

        # Draw noise reduction icon
        self._draw_noise_reduction_icon(cr, width, height, "retro")

    def _draw_retro_background(
        self,
        cr: cairo.Context,
        width: int,
        height: int,
        padding_bottom: float,
        max_bar_height: float,
    ) -> None:
        """
        Draw the background for the retro visualization style.

        Args:
            cr: The Cairo context
            width: The width of the drawing area
            height: The height of the drawing area
            padding_bottom: Bottom padding
            max_bar_height: Maximum height of bars
        """
        # Draw gradient background
        cr.save()
        bg_gradient = cairo.LinearGradient(0, 0, 0, height)
        bg_gradient.add_color_stop_rgba(0, 0.05, 0.1, 0.05, 0.4)  # Dark background top
        bg_gradient.add_color_stop_rgba(1, 0.02, 0.05, 0.02, 0.2)  # Darker bottom

        cr.set_source(bg_gradient)
        cr.rectangle(0, 0, width, height)
        cr.fill()
        cr.restore()

        # Draw grid lines
        cr.save()
        cr.set_line_width(0.5)
        cr.set_source_rgba(0.1, 0.4, 0.1, 0.3)

        # Horizontal grid lines
        grid_lines = 10
        for i in range(1, grid_lines):
            y = height - padding_bottom - (i * max_bar_height / grid_lines)
            cr.move_to(0, y)
            cr.line_to(width, y)

        cr.stroke()
        cr.restore()

    def _draw_retro_bar(
        self,
        cr: cairo.Context,
        x: float,
        y: float,
        bar_width: float,
        bar_height: float,
        baseline_y: float,
        max_bar_height: float,
    ) -> None:
        """
        Draw a single retro-style equalizer bar.

        Args:
            cr: The Cairo context
            x: X position of the bar
            y: Y position of the bar (top)
            bar_width: Width of the bar
            bar_height: Height of the bar
            baseline_y: Y position of the baseline
            max_bar_height: Maximum possible bar height
        """
        # Create bar gradient
        gradient = cairo.LinearGradient(0, baseline_y, 0, baseline_y - max_bar_height)
        gradient.add_color_stop_rgba(0, 0.1, 0.8, 0.1, 0.9)  # Green at bottom
        gradient.add_color_stop_rgba(0.5, 0.9, 0.9, 0.1, 0.9)  # Yellow in middle
        gradient.add_color_stop_rgba(1, 0.9, 0.1, 0.1, 0.9)  # Red at top

        # Draw bar
        cr.save()

        # Fill bar
        cr.set_source(gradient)
        cr.rectangle(x, y, bar_width, bar_height)
        cr.fill()

        # Add highlight on top of the bar
        cr.set_source_rgba(1.0, 1.0, 0.6, 0.7)
        cr.rectangle(x, y, bar_width, 2)
        cr.fill()

        # Add segments for retro look
        segment_height = 3
        segments = int(bar_height / (segment_height * 2))

        cr.set_source_rgba(0.0, 0.0, 0.0, 0.3)  # Dark line color
        for j in range(segments):
            segment_y = y + (j * segment_height * 2)
            cr.rectangle(x, segment_y + segment_height, bar_width, segment_height)
        cr.fill()

        cr.restore()

    def _draw_noise_reduction_icon(
        self, cr: cairo.Context, width: int, height: int, style: str
    ) -> None:
        """
        Draw the noise reduction icon in the center of the visualization.

        Args:
            cr: The Cairo context
            width: Width of the drawing area
            height: Height of the drawing area
            style: Visual style to apply ("warm" or "retro")
        """
        center_x = width / 2
        center_y = height / 2
        icon_radius = min(width, height) * 0.22

        # Draw background circle
        cr.save()

        # Set colors based on active state
        if self.noise_reduction_active:
            # Active state - green theme
            radial_gradient = cairo.RadialGradient(
                center_x, center_y, icon_radius * 0.5, center_x, center_y, icon_radius
            )
            radial_gradient.add_color_stop_rgba(0, 0.1, 0.3, 0.1, 0.9)  # Green center
            radial_gradient.add_color_stop_rgba(
                1, 0.05, 0.2, 0.05, 0.7
            )  # Dark green edge
            ring_color = (0.2, 0.8, 0.2, 0.7)  # Bright green ring
        else:
            # Inactive state - gray theme
            radial_gradient = cairo.RadialGradient(
                center_x, center_y, icon_radius * 0.5, center_x, center_y, icon_radius
            )
            radial_gradient.add_color_stop_rgba(
                0, 0.25, 0.25, 0.25, 0.8
            )  # Light gray center
            radial_gradient.add_color_stop_rgba(
                1, 0.15, 0.15, 0.15, 0.6
            )  # Dark gray edge
            ring_color = (0.5, 0.5, 0.5, 0.5)  # Gray ring

        # Draw circle background
        cr.set_source(radial_gradient)
        cr.arc(center_x, center_y, icon_radius, 0, 2 * math.pi)
        cr.fill()

        # Draw ring
        cr.set_source_rgba(*ring_color)
        cr.set_line_width(3.0)
        cr.arc(center_x, center_y, icon_radius, 0, 2 * math.pi)
        cr.stroke()

        cr.restore()

        # Try to draw SVG icon
        icon_drawn = False
        if HAS_RSVG:
            icon = self.icon_on if self.noise_reduction_active else self.icon_off
            if icon:
                try:
                    self._draw_svg_icon(cr, icon, center_x, center_y, icon_radius)
                    icon_drawn = True
                except Exception as e:
                    logger.error(f"Error rendering SVG icon: {e}")

        # Fallback to drawing a simple icon if SVG rendering fails
        if not icon_drawn:
            self._draw_fallback_icon(
                cr, center_x, center_y, icon_radius * 0.8, self.noise_reduction_active
            )

    def _draw_svg_icon(
        self,
        cr: cairo.Context,
        icon: Rsvg.Handle,
        center_x: float,
        center_y: float,
        icon_radius: float,
    ) -> None:
        """
        Draw an SVG icon centered in the visualization.

        Args:
            cr: The Cairo context
            icon: The SVG icon handle
            center_x: X coordinate of the center
            center_y: Y coordinate of the center
            icon_radius: Radius of the icon area
        """
        # Get SVG dimensions
        dim = icon.get_dimensions()
        svg_width = dim.width
        svg_height = dim.height

        # Calculate scale to fit in the icon area
        icon_size = icon_radius * 0.6
        scale_x = (icon_size * 2) / svg_width
        scale_y = (icon_size * 2) / svg_height
        scale = min(scale_x, scale_y)

        # Center the icon
        cr.save()
        cr.translate(
            center_x - (svg_width * scale / 2), center_y - (svg_height * scale / 2)
        )
        cr.scale(scale, scale)

        # Render the SVG
        icon.render_cairo(cr)
        cr.restore()

    def _draw_fallback_icon(
        self, cr: cairo.Context, cx: float, cy: float, radius: float, is_active: bool
    ) -> None:
        """
        Draw a fallback icon if SVG rendering fails.

        Args:
            cr: The Cairo context
            cx: X coordinate of the center
            cy: Y coordinate of the center
            radius: Radius of the icon
            is_active: Whether noise reduction is active
        """
        # Set color based on state
        color = (0.2, 0.8, 0.2) if is_active else (0.6, 0.6, 0.6)

        cr.save()

        # Draw sound wave arcs
        cr.set_line_width(2)
        cr.set_source_rgba(*color, 0.9)

        sizes = [0.5, 0.7, 0.9]
        for size in sizes:
            arc_radius = radius * size
            if is_active:
                # Complete arcs when active
                cr.arc(cx, cy, arc_radius, math.pi * 0.8, math.pi * 2.2)
                cr.stroke()
            else:
                # Broken arcs when inactive
                cr.arc(cx, cy, arc_radius, math.pi * 0.8, math.pi * 1.3)
                cr.stroke()
                cr.arc(cx, cy, arc_radius, math.pi * 1.7, math.pi * 2.2)
                cr.stroke()

        # Draw center circle
        cr.set_source_rgba(*color, 0.9)
        cr.arc(cx, cy, radius * 0.2, 0, math.pi * 2)
        cr.fill()

        # Draw checkmark when active
        if is_active:
            cr.set_source_rgba(1, 1, 1, 0.9)
            cr.set_line_width(2)
            cr.move_to(cx - radius * 0.1, cy)
            cr.line_to(cx, cy + radius * 0.1)
            cr.line_to(cx + radius * 0.2, cy - radius * 0.1)
            cr.stroke()

        cr.restore()

    def __del__(self) -> None:
        """Clean up resources when the widget is destroyed."""
        # Stop the device monitor
        if hasattr(self, "device_monitor") and self.device_monitor:
            self.device_monitor.stop()

        # Stop the pipeline
        self._stop_pipeline()
