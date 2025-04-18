import gi
import numpy as np  # Actually needed for array processing
import threading  # Keep for future threading needs
import cairo  # Needed for drawing
import time  # Used for timestamps
import math  # Required for calculations
import random  # Used for visualization
import asyncio  # Used in noise_reducer_service connectivity
import os  # Used for file path checks
import gettext  # Add gettext for internationalization

# Set up translation function
_ = gettext.gettext

gi.require_version("Gtk", "4.0")
gi.require_version("Gst", "1.0")
gi.require_version("GstAudio", "1.0")
try:
    gi.require_version("Rsvg", "2.0")
    from gi.repository import Gtk, GLib, Gst, Rsvg  # Remove Gdk, GObject, GstAudio

    HAS_RSVG = True
except (ImportError, ValueError):
    from gi.repository import Gtk, GLib, Gst  # Keep just what's needed

    HAS_RSVG = False
    print(_("Rsvg module not available, will use fallback icon drawing"))

# Initialize GStreamer
Gst.init(None)


class AudioVisualizer(Gtk.Box):
    """
    Audio visualization component using GTK4 with real microphone input via Pipewire.
    Shows sound waves in different visualization styles.
    """

    # Visualization style constants
    STYLE_MODERN = 0
    STYLE_RETRO = 1
    STYLE_RADIAL = 2  # Renumbered

    def __init__(self):
        # Initialize as a vertical box container instead of just a drawing area
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)

        # Set size - returned to original height (no system controls)
        self.set_size_request(300, 130)

        # Noise reduction status and service reference
        self.noise_reducer_service = None
        self.noise_reduction_active = False

        # SVG icon handlers
        self.icon_on = None
        self.icon_off = None
        self.load_svg_icons()

        # Create the drawing area for visualization
        self.drawing_area = Gtk.DrawingArea()
        self.drawing_area.set_content_width(300)
        self.drawing_area.set_content_height(100)
        self.drawing_area.set_draw_func(self.on_draw, None)
        self.drawing_area.set_vexpand(True)

        # Enable click handling on the drawing area
        self.click_controller = Gtk.GestureClick.new()
        self.click_controller.connect("pressed", self.on_click)
        self.drawing_area.add_controller(self.click_controller)

        # Add the drawing area to our container
        self.append(self.drawing_area)

        # Current visualization style
        self.current_style = self.STYLE_MODERN

        # Create style selection buttons
        self.create_style_buttons()

        # Audio visualization properties
        self.frequency_data = np.zeros(16)
        self.data_mapping = {
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

        # Audio capture tracking
        self.last_spectrum_time = 0
        self.debug_mode = True
        self.startup_grace_period = True  # Add grace period for startup
        self.non_disruptive_mode = True  # Use gentler initialization

        # Device monitoring - make it optional (initialize to None)
        self.current_audio_device = None
        self.device_monitor = None

        # Only set up device monitoring if needed, using a timeout
        GLib.timeout_add(3000, self.setup_device_monitor)

        # Restart timer for the audio pipeline to handle initial startup issues
        self._pipeline_restart_attempts = 0
        self._max_restart_attempts = 5  # Increased from 3 to 5

        # Set up audio pipeline with a longer delay to allow UI initialization first
        # and avoid disrupting the system's audio
        GLib.timeout_add(1000, self.delayed_pipeline_setup)

        # Add timers for refreshing the visualization and checking microphone changes
        GLib.timeout_add(50, self.update_visualization)
        GLib.timeout_add(1000, self.check_microphone_changes)

        # Timer to end startup grace period
        GLib.timeout_add(5000, self.end_startup_grace_period)

    def create_style_buttons(self):
        """Create buttons to switch between visualization styles."""
        # Create a horizontal box for buttons
        button_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        button_box.set_halign(Gtk.Align.CENTER)
        button_box.set_margin_top(4)
        button_box.set_margin_bottom(4)

        # Ensure the button box has a minimum height to prevent allocation issues
        button_box.set_size_request(-1, 32)  # Default width, fixed height

        # Create style buttons
        modern_button = Gtk.Button(label=_("Modern Waves"))
        modern_button.connect(
            "clicked", self.on_style_button_clicked, self.STYLE_MODERN
        )
        modern_button.set_tooltip_text(_("Sound waves with green to red gradient"))

        retro_button = Gtk.Button(label=_("Retro Bars"))
        retro_button.connect("clicked", self.on_style_button_clicked, self.STYLE_RETRO)
        retro_button.set_tooltip_text(_("Classic equalizer-style visualization"))

        # Add radial spectrum button
        radial_button = Gtk.Button(label=_("Spectrum"))
        radial_button.connect(
            "clicked", self.on_style_button_clicked, self.STYLE_RADIAL
        )
        radial_button.set_tooltip_text(_("Spectrum visualizer with green to red gradient"))

        # Add buttons to button box
        button_box.append(modern_button)
        button_box.append(retro_button)
        button_box.append(radial_button)

        # Add button box to main container
        self.append(button_box)

    def on_style_button_clicked(self, button, style):
        """Handle style button clicks."""
        self.current_style = style
        self.drawing_area.queue_draw()

    def set_noise_reducer_service(self, service):
        """Connect to a noise reducer service to control status."""
        self.noise_reducer_service = service
        # Initial status check
        GLib.timeout_add(1000, self.update_noise_reduction_status)

    def update_noise_reduction_status(self):
        """Update the noise reduction status asynchronously."""
        if not self.noise_reducer_service:
            return True

        def check_status():
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)

            try:
                status = loop.run_until_complete(
                    self.noise_reducer_service.get_noise_reduction_status()
                )
                GLib.idle_add(
                    lambda: self.set_noise_reduction_status(status == "enabled")
                )
            finally:
                loop.close()

        threading.Thread(target=check_status, daemon=True).start()
        return True  # Continue checking periodically

    def set_noise_reduction_status(self, is_active):
        """Set the noise reduction status and update the icon."""
        if self.noise_reduction_active != is_active:
            self.noise_reduction_active = is_active
            self.drawing_area.queue_draw()  # Redraw to show the correct icon

    def toggle_noise_reduction(self):
        """Toggle the noise reduction state."""
        if not self.noise_reducer_service:
            return

        def toggle_service():
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)

            try:
                if self.noise_reduction_active:
                    loop.run_until_complete(
                        self.noise_reducer_service.stop_noise_reduction()
                    )
                    GLib.idle_add(lambda: self.set_noise_reduction_status(False))
                else:
                    loop.run_until_complete(
                        self.noise_reducer_service.start_noise_reduction()
                    )
                    GLib.idle_add(lambda: self.set_noise_reduction_status(True))
            finally:
                loop.close()

        threading.Thread(target=toggle_service, daemon=True).start()

    def load_svg_icons(self):
        """Load the SVG icons for noise reduction status."""
        if not HAS_RSVG:
            return

        try:
            on_path = (
                "/usr/share/icons/hicolor/scalable/status/big-noise-reduction-on.svg"
            )
            off_path = (
                "/usr/share/icons/hicolor/scalable/status/big-noise-reduction-off.svg"
            )

            # Check if icons exist
            if os.path.exists(on_path):
                self.icon_on = Rsvg.Handle.new_from_file(on_path)

            if os.path.exists(off_path):
                self.icon_off = Rsvg.Handle.new_from_file(off_path)

            if not self.icon_on or not self.icon_off:
                print("Warning: Could not load noise reduction SVG icons")
        except Exception as e:
            print(f"Error loading SVG icons: {e}")

    def on_click(self, controller, n_press, x, y):
        """Handle clicks on the drawing area."""
        # Check if the click is within the icon area
        width = self.drawing_area.get_width()
        height = self.drawing_area.get_height()
        center_x = width / 2
        center_y = height / 2
        icon_radius = (
            min(width, height) * 0.2
        )  # Slightly larger than drawing radius for easier targeting

        # Calculate if click is inside the icon area
        distance = math.sqrt((x - center_x) ** 2 + (y - center_y) ** 2)
        if distance <= icon_radius:
            self.toggle_noise_reduction()

    def setup_device_monitor(self):
        """Set up device monitoring to detect microphone changes."""
        if self.device_monitor:
            return False  # Already set up

        try:
            # Create a device monitor that watches for audio source changes
            self.device_monitor = Gst.DeviceMonitor.new()

            # Add a filter for audio sources, but be careful with the filter settings
            # to avoid potential errors
            try:
                self.device_monitor.add_filter("Audio/Source", None)
            except Exception as e:
                print(f"Warning: Could not add device filter: {e}")
                # Continue anyway - the monitor can still work without filter

            # Fix the signal connection using proper signal names
            bus = self.device_monitor.get_bus()
            bus.add_signal_watch()
            bus.connect("message", self.on_device_monitor_message)

            # Start monitoring
            success = self.device_monitor.start()

            if success:
                print("Device monitor started successfully")
            else:
                print("Failed to start device monitor")
                return False

        except Exception as e:
            print(f"Error setting up device monitor: {e}")
            return False

        return False  # Don't repeat this timeout

    def on_device_monitor_message(self, bus, message):
        """Handle device monitor messages from the bus."""
        if message.type == Gst.MessageType.DEVICE_ADDED:
            device = message.parse_device_added()
            print(f"Device added: {device.get_display_name()}")
            self.check_microphone_changes()
        elif message.type == Gst.MessageType.DEVICE_REMOVED:
            device = message.parse_device_removed()
            print(f"Device removed: {device.get_display_name()}")
            self.check_microphone_changes()

        return True

    def get_default_audio_source(self):
        """Get the current default audio source (microphone)."""
        try:
            # First try environment variables that may indicate default source
            import os

            default_source = os.environ.get("PULSE_SOURCE") or os.environ.get(
                "PIPEWIRE_DEFAULT_SOURCE"
            )
            if default_source:
                return default_source

            # Try a simple approach first - this is non-disruptive
            try:
                # Try to run pactl to get the default source
                import subprocess

                result = subprocess.run(
                    ["pactl", "get-default-source"],
                    capture_output=True,
                    text=True,
                    timeout=1,
                )
                if result.returncode == 0 and result.stdout.strip():
                    return result.stdout.strip()
            except:
                pass  # Ignore errors from pactl

            # Use GStreamer's device provider only if the simpler approach fails
            device_provider = Gst.DeviceProviderFactory.get_by_name(
                "pulsedeviceprovider"
            )
            if not device_provider:
                return "default"  # Return a generic name if provider not available

            devices = device_provider.get_devices()
            if not devices:
                return "default"

            # Debug: Print all found devices
            if self.debug_mode:
                print(f"Found {len(devices)} audio devices")

            # Find the default source
            for device in devices:
                # Check if it's a source (microphone)
                if device.get_device_class() == "Audio/Source":
                    props = device.get_properties()

                    # Correctly access properties from GstStructure
                    device_name = None
                    if props.has_field("device.description"):
                        device_name = props.get_string("device.description")
                    elif props.has_field("device.name"):
                        device_name = props.get_string("device.name")

                    # Use display_name as fallback
                    if not device_name:
                        device_name = device.get_display_name()

                    if device_name and "default" in device_name.lower():
                        return device_name

            # If no default device is found but we have sources, return the first one
            if devices:
                for device in devices:
                    if device.get_device_class() == "Audio/Source":
                        # Just return the display name which should be available
                        return device.get_display_name()

            # If all else fails, return a generic name
            return "default"
        except Exception as e:
            print(f"Error getting default audio source: {e}")
            return "default"

    def check_microphone_changes(self):
        """Check if the default microphone has changed and restart pipeline if needed."""
        if self.startup_grace_period:
            return True  # Skip check during startup

        try:
            new_device = self.get_default_audio_source()

            # If the default device has changed, restart the audio pipeline
            if new_device != self.current_audio_device:
                print(
                    f"Default microphone changed from '{self.current_audio_device}' to '{new_device}'"
                )
                self.current_audio_device = new_device

                # Stop current pipeline if it exists
                if hasattr(self, "pipeline"):
                    self.pipeline.set_state(Gst.State.NULL)

                # Restart with new pipeline
                self.setup_audio_pipeline()

        except Exception as e:
            print(f"Error checking microphone changes: {e}")

        return True  # Continue the timeout

    def end_startup_grace_period(self):
        """End the startup grace period."""
        self.startup_grace_period = False
        print("Startup grace period ended")
        return False  # Run once

    def delayed_pipeline_setup(self):
        """Sets up audio pipeline with a slight delay after UI initialization."""
        print("Starting delayed pipeline setup")

        # Get default audio device first, before creating pipeline
        self.current_audio_device = self.get_default_audio_source()
        if self.current_audio_device:
            print(f"Detected default audio device: {self.current_audio_device}")

        # Create pipeline without disrupting system audio
        self.setup_audio_pipeline()
        return False  # Don't repeat this timeout

    def setup_audio_pipeline(self):
        """Sets up the GStreamer pipeline for audio capture."""
        try:
            # Stop any existing pipeline - only our own
            if hasattr(self, "pipeline") and self.pipeline:
                print("Stopping existing pipeline")
                self.pipeline.set_state(Gst.State.NULL)

            # Try a non-disruptive approach first using autoaudiosrc
            if self.non_disruptive_mode:
                print("Using non-disruptive audio pipeline")

                # Modified pipeline to avoid int range assertion errors
                pipeline_str = (
                    "autoaudiosrc name=src ! "
                    "queue ! "
                    "audioconvert ! "
                    "audioresample ! "
                    # Remove the specific format specifications that are causing range errors
                    "audio/x-raw ! "
                    # Fix spectrum bands to avoid int range error
                    "spectrum bands=16 threshold=-80 interval=50000000 post-messages=true ! "
                    "fakesink sync=false"
                )

                self.pipeline = Gst.parse_launch(pipeline_str)

                # Make source element completely non-disruptive
                src = self.pipeline.get_by_name("src")
                if src:
                    # Try to set properties that make it gentler on system resources
                    try:
                        if hasattr(src.props, "do-timestamp"):
                            src.set_property("do-timestamp", True)
                        if hasattr(src.props, "buffer-time"):
                            src.set_property("buffer-time", 500000)  # 500ms buffer
                    except Exception:
                        pass  # Ignore if properties don't exist

                # Setup bus to watch for messages
                bus = self.pipeline.get_bus()
                bus.add_signal_watch()
                bus.connect("message::element", self.on_message)
                bus.connect("message::error", self.on_error)
                bus.connect("message::state-changed", self.on_state_changed)
                bus.connect("message::eos", self.on_eos)

                # Start the pipeline directly to PLAYING state
                ret = self.pipeline.set_state(Gst.State.PLAYING)
                print(f"Pipeline set to PLAYING state: {ret}")

                # Reset timestamp
                self.last_spectrum_time = time.time()
                return

            # If non-disruptive approach is disabled, try with more aggressive options
            pipeline_options = [
                # Option 1: Simplified PipeWire source - no format restrictions
                (
                    "pipewiresrc ! "
                    "audioconvert ! "
                    "audio/x-raw ! "
                    "spectrum bands=32 threshold=-80 interval=50000000 post-messages=true ! "
                    "fakesink sync=false"
                ),
                # Option 2: Use PulseAudio source with minimal pipeline
                (
                    "pulsesrc ! "
                    "audioconvert ! "
                    "audio/x-raw ! "
                    "spectrum bands=32 threshold=-80 interval=50000000 post-messages=true ! "
                    "fakesink sync=false"
                ),
                # Option 3: Auto-select audio source (simplest option) - minimal caps
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
                    print(f"Trying audio pipeline option {i + 1}...")
                    self.pipeline = Gst.parse_launch(pipeline_str)

                    # Setup bus to watch for messages
                    bus = self.pipeline.get_bus()
                    bus.add_signal_watch()
                    bus.connect("message::element", self.on_message)
                    bus.connect("message::error", self.on_error)
                    bus.connect("message::state-changed", self.on_state_changed)
                    bus.connect("message::eos", self.on_eos)

                    # Start the pipeline
                    ret = self.pipeline.set_state(Gst.State.PLAYING)
                    if ret != Gst.StateChangeReturn.FAILURE:
                        print(f"Audio pipeline {i + 1} started successfully")
                        pipeline_success = True

                        # Store the current device
                        self.current_audio_device = self.get_default_audio_source()
                        if self.current_audio_device:
                            print(f"Using audio device: {self.current_audio_device}")

                        # Reset timestamp to now to give pipeline time to produce data
                        self.last_spectrum_time = time.time()

                        # Reset restart attempts counter
                        self._pipeline_restart_attempts = 0

                        break
                    else:
                        self.pipeline.set_state(Gst.State.NULL)
                        print(f"Pipeline {i + 1} failed to start")

                except Exception as e:
                    print(f"Error with pipeline {i + 1}: {e}")

            if not pipeline_success:
                print(
                    "All audio pipelines failed, visualization will not display audio data"
                )

                # Try to restart the pipeline if we haven't exceeded the maximum attempts
                if self._pipeline_restart_attempts < self._max_restart_attempts:
                    self._pipeline_restart_attempts += 1
                    restart_delay = 2000 * self._pipeline_restart_attempts
                    print(f"Scheduling pipeline restart in {restart_delay / 1000}s")
                    GLib.timeout_add(restart_delay, self.delayed_pipeline_setup)

        except Exception as e:
            print(f"Error setting up audio pipeline: {e}")

        # Set initial timestamp
        self.last_spectrum_time = time.time()

    def complete_pipeline_start(self):
        """Complete pipeline startup after initial READY state."""
        if hasattr(self, "pipeline") and self.pipeline:
            ret = self.pipeline.set_state(Gst.State.PLAYING)
            print(f"Pipeline set to PLAYING state: {ret}")
            # Reset timestamp
            self.last_spectrum_time = time.time()
        return False  # Don't repeat

    def on_message(self, bus, message):
        """Handle element messages from GStreamer bus."""
        if message.get_structure() and message.get_structure().get_name() == "spectrum":
            # Process spectrum data from messages
            structure = message.get_structure()
            if structure:
                # Get the magnitude array
                magnitudes = structure.get_value("magnitude")
                if magnitudes:
                    self.last_spectrum_time = time.time()
                    self.process_spectrum_data(magnitudes)
                    return True
                else:
                    print("Warning: Received spectrum message without magnitude values")
        return False

    def on_error(self, bus, message):
        """Handle error messages from GStreamer bus."""
        err, debug = message.parse_error()
        print(f"Error: {err}, Debug: {debug}")

    def on_state_changed(self, bus, message):
        """Handle state changed messages from the pipeline."""
        if message.src == self.pipeline:
            old_state, new_state, pending_state = message.parse_state_changed()
            state_names = {
                Gst.State.NULL: "NULL",
                Gst.State.READY: "READY",
                Gst.State.PAUSED: "PAUSED",
                Gst.State.PLAYING: "PLAYING",
            }
            print(
                f"Pipeline state changed from {state_names.get(old_state)} to {state_names.get(new_state)}"
            )

            if new_state == Gst.State.PLAYING:
                # Reset timestamp to avoid immediate "No spectrum data" message
                self.last_spectrum_time = time.time()
                # Restart grace period when pipeline starts playing
                self.startup_grace_period = True
                GLib.timeout_add(3000, self.end_startup_grace_period)

    def on_eos(self, bus, message):
        """Handle end-of-stream messages."""
        print("End of stream received, attempting to restart pipeline")
        GLib.idle_add(self.delayed_pipeline_setup)

    def process_spectrum_data(self, magnitudes):
        """Process spectrum data from GStreamer message."""
        try:
            # Convert magnitudes to numpy array
            mag_array = np.array(magnitudes)

            # Check if audio is silent (max level is very low)
            max_db = np.max(mag_array)
            is_silent = max_db <= -90.0

            # Debug info with less frequency to reduce console spam
            if random.random() < 0.05:  # Only print ~5% of the time
                min_db = np.min(mag_array)
                print(
                    f"Audio spectrum data range: min={min_db:.1f}dB, max={max_db:.1f}dB"
                )

            if is_silent:
                # For silent audio, use zeros with no random variation
                self.frequency_data = np.zeros(16)
                return

            # Normalize dB values to a visible range for visualization
            # dB values are typically -90 to 0, convert to 0-1 range with enhanced sensitivity
            normalized = np.zeros_like(mag_array, dtype=float)

            # More dramatic scaling to make small changes more visible
            min_db_threshold = -80  # Treat anything below this as silence
            max_db_threshold = -20  # Treat this as the max value for normalization

            # Scale and normalize with more emphasis on quiet sounds
            for i in range(len(mag_array)):
                # Limit the range
                mag_db = min(max(mag_array[i], min_db_threshold), 0)

                # Apply non-linear scaling to make quiet sounds more visible
                # Map -80dB..0dB to 0..1 with emphasis on middle range
                if mag_db <= min_db_threshold:
                    normalized[i] = 0
                else:
                    # Non-linear mapping emphasizes changes in the middle range
                    # This creates more visible movement in the visualization
                    normalized_linear = (mag_db - min_db_threshold) / (
                        0 - min_db_threshold
                    )
                    normalized[i] = np.power(
                        normalized_linear, 0.5
                    )  # Square root for non-linear emphasis

            # Sample 16 bands from the spectrum for visualization
            if len(normalized) >= 16:
                # Use more of the low frequency range where voice is more present
                indices = np.logspace(0, np.log10(len(normalized) * 0.7), 16).astype(
                    int
                )
                indices = np.clip(indices, 0, len(normalized) - 1)
                self.frequency_data = normalized[indices]
            else:
                self.frequency_data = normalized[:16]

            # Apply minimal smoothing with slight movement only when sound is present
            for i in range(len(self.frequency_data)):
                # Add small random movement for visual interest only when not silent
                if (
                    not is_silent and self.frequency_data[i] > 0.01
                ):  # Only add movement to visible bars
                    self.frequency_data[i] += random.uniform(-0.02, 0.02)
                    self.frequency_data[i] = max(0, min(1, self.frequency_data[i]))

        except Exception as e:
            print(f"Error processing spectrum data: {e}")

    def update_visualization(self):
        """Updates frequency data and requests redraw."""
        try:
            # Check if we've received any spectrum data recently
            current_time = time.time()
            time_since_data = current_time - self.last_spectrum_time

            # During startup or first few seconds, don't report no data
            if self.startup_grace_period or time_since_data <= 3:
                pass  # No action, just wait for data
            elif time_since_data > 3:
                # If no recent data, display empty bars (zeros)
                self.frequency_data = np.zeros(16)

                # If we haven't tried too many times, try restarting the pipeline
                if (
                    self._pipeline_restart_attempts < self._max_restart_attempts
                    and time_since_data > 5
                ):
                    self._pipeline_restart_attempts += 1
                    print(f"No data for {time_since_data:.1f}s, attempting restart")
                    GLib.idle_add(self.setup_audio_pipeline)

            # Request redraw
            self.drawing_area.queue_draw()

        except Exception as e:
            print(f"Error updating visualization: {e}")

        return True  # Continue the timer

    def on_draw(self, area, cr, width, height, data):
        """Draws audio visualization based on the current style."""
        try:
            # Clear background with transparency
            cr.set_source_rgba(0, 0, 0, 0)
            cr.paint()

            # Draw based on selected style
            if self.current_style == self.STYLE_MODERN:
                self.draw_modern_waves(cr, width, height)
            elif self.current_style == self.STYLE_RETRO:
                self.draw_retro_bars(cr, width, height)
            elif self.current_style == self.STYLE_RADIAL:
                self.draw_radial_spectrum(cr, width, height)
        except Exception as e:
            print(f"Error in on_draw: {e}")
            # Draw error message
            cr.set_source_rgba(0.8, 0.0, 0.0, 0.7)
            cr.select_font_face("Sans", cairo.FontSlant.NORMAL, cairo.FontWeight.BOLD)
            cr.set_font_size(12)
            cr.move_to(10, height / 2)
            cr.show_text(_("Visualization Error"))

    def draw_modern_waves(self, cr, width, height):
        """Draws sound waves with green to yellow to red gradients."""
        # Calculate center for proper positioning
        center_x = width / 2
        center_y = height / 2

        # Draw a more subtle glow beneath the visualization area
        cr.save()
        glow_gradient = cairo.RadialGradient(
            center_x, center_y, 0, center_x, center_y, width / 3
        )
        glow_gradient.add_color_stop_rgba(0, 0.1, 0.3, 0.1, 0.2)  # Green inner glow
        glow_gradient.add_color_stop_rgba(1, 0.0, 0.0, 0.0, 0.0)  # Fade to transparent

        cr.set_source(glow_gradient)
        cr.rectangle(0, 0, width, height)
        cr.fill()
        cr.restore()

        # Create path for the waveform
        cr.save()

        # Define wave thickness and number of points
        wave_thickness = 3
        wave_padding = height * 0.15  # Space at top and bottom
        wave_height = height - (wave_padding * 2)
        num_points = 100  # More points for smoother curve

        # Create gradient for the sound wave - reversed: low=green, high=red
        wave_gradient = cairo.LinearGradient(0, wave_padding, 0, height - wave_padding)
        wave_gradient.add_color_stop_rgba(
            0, 0.9, 0.2, 0.1, 0.9
        )  # Red at top (high level)
        wave_gradient.add_color_stop_rgba(0.5, 0.9, 0.9, 0.1, 0.85)  # Yellow in middle
        wave_gradient.add_color_stop_rgba(
            1, 0.1, 0.8, 0.1, 0.8
        )  # Green at bottom (low level)

        # We'll draw multiple waves with different phases for a futuristic effect
        for wave_idx in range(3):
            phase_offset = wave_idx * 1.5  # Stagger the waves
            opacity_factor = 0.9 - wave_idx * 0.25  # More transparent for each wave

            cr.save()

            # Create the wave path
            cr.new_path()

            # Start point for the wave
            point_spacing = width / num_points

            # Map our 16 frequency values to 100 points with interpolation
            wave_values = []
            for i in range(num_points + 1):
                # Map point index to a frequency index
                freq_idx = min(15, int((i / num_points) * 16))
                next_freq_idx = min(15, freq_idx + 1)

                # Interpolation factor between the two frequency points
                interp_factor = (i / num_points * 16) - freq_idx

                # Get the interpolated value
                value1 = self.frequency_data[self.data_mapping.get(freq_idx, freq_idx)]
                value2 = self.frequency_data[
                    self.data_mapping.get(next_freq_idx, next_freq_idx)
                ]
                interp_value = value1 * (1 - interp_factor) + value2 * interp_factor

                # Add phase offset and scaling effect
                wave_height_factor = (
                    1.0 - wave_idx * 0.15
                )  # Each wave is slightly smaller
                value = interp_value * wave_height_factor

                # Apply a sine wave effect for futuristic look
                mod_factor = (
                    math.sin((i / num_points * 10 + phase_offset) * math.pi) * 0.15
                )
                value = value * (1 + mod_factor)

                wave_values.append(value)

            # First point of the wave
            first_value = wave_values[0]
            first_y = center_y + ((0.5 - first_value) * wave_height)
            cr.move_to(0, first_y)

            # Draw the wave path with smooth curves
            for i in range(1, num_points + 1):
                x = i * point_spacing
                value = wave_values[i - 1]

                # Calculate y position based on value (center ± half of wave height)
                y = center_y + ((0.5 - value) * wave_height)

                # Draw smooth curve to this point using bezier curve
                if i < num_points:
                    # Calculate control points
                    next_value = wave_values[i]
                    next_y = center_y + ((0.5 - next_value) * wave_height)
                    cp1_x = x - point_spacing * 0.5
                    cp1_y = y
                    cp2_x = x
                    cp2_y = next_y

                    # Draw curve to next point
                    cr.curve_to(cp1_x, cp1_y, cp2_x, cp2_y, x + point_spacing, next_y)
                else:
                    cr.line_to(x, y)

            # Set stroke width based on wave index
            cr.set_line_width(wave_thickness * (1.0 - wave_idx * 0.2))

            # Apply the gradient with adjusted opacity
            cr.set_source(wave_gradient)
            cr.set_operator(cairo.OPERATOR_OVER)
            cr.stroke()

            # Add glow effect along the wave path
            cr.set_source_rgba(0.9, 0.4, 0.1, 0.4 * opacity_factor)  # Warm glow color
            cr.set_line_width(wave_thickness * 2.5 * (1.0 - wave_idx * 0.2))
            cr.set_operator(cairo.OPERATOR_ADD)
            cr.stroke()

            cr.restore()

        cr.restore()

        # Draw noise reduction icon instead of microphone
        self.draw_noise_reduction_icon(cr, width, height, "warm")

    def draw_radial_spectrum(self, cr, width, height):
        """Draws a circular spectrum visualizer with bars arranged around the circumference."""
        # Define constants
        center_x = width / 2
        center_y = height / 2
        max_radius = min(width, height) * 0.45
        inner_radius = min(width, height) * 0.2
        bar_count = 48  # More bars for smoother look
        max_bar_height = max_radius - inner_radius

        # Draw background with the same green glow as Modern Waves
        cr.save()
        glow_gradient = cairo.RadialGradient(
            center_x, center_y, 0, center_x, center_y, width / 3
        )
        glow_gradient.add_color_stop_rgba(0, 0.1, 0.3, 0.1, 0.2)  # Green inner glow
        glow_gradient.add_color_stop_rgba(1, 0.0, 0.0, 0.0, 0.0)  # Fade to transparent

        cr.set_source(glow_gradient)
        cr.rectangle(0, 0, width, height)
        cr.fill()
        cr.restore()

        # Draw spectrum bars arranged in a circle
        for i in range(bar_count):
            # Map bar index to frequency data
            freq_idx = i % 16
            value = self.frequency_data[self.data_mapping.get(freq_idx, freq_idx)]

            # Calculate bar properties
            angle = (i / bar_count) * 2 * math.pi
            bar_height = value * max_bar_height
            bar_width_degrees = (
                (2 * math.pi) / bar_count * 0.7
            )  # 70% of available space

            # Calculate bar corners
            inner_x = center_x + inner_radius * math.cos(angle)
            inner_y = center_y + inner_radius * math.sin(angle)
            outer_x = center_x + (inner_radius + bar_height) * math.cos(angle)
            outer_y = center_y + (inner_radius + bar_height) * math.sin(angle)

            # Calculate side points
            half_width = bar_width_degrees / 2
            inner_start_x = center_x + inner_radius * math.cos(angle - half_width)
            inner_start_y = center_y + inner_radius * math.sin(angle - half_width)
            inner_end_x = center_x + inner_radius * math.cos(angle + half_width)
            inner_end_y = center_y + inner_radius * math.sin(angle + half_width)

            outer_start_x = center_x + (inner_radius + bar_height) * math.cos(
                angle - half_width
            )
            outer_start_y = center_y + (inner_radius + bar_height) * math.sin(
                angle - half_width
            )

            # Create a gradient for this bar - green to yellow to red
            gradient = cairo.LinearGradient(inner_x, inner_y, outer_x, outer_y)

            # Green to yellow to red gradient
            gradient.add_color_stop_rgba(0, 0.1, 0.8, 0.1, 0.7)  # Green - inner
            gradient.add_color_stop_rgba(0.5, 0.9, 0.9, 0.1, 0.8)  # Yellow - middle
            gradient.add_color_stop_rgba(1, 0.9, 0.1, 0.1, 0.9)  # Red - outer

            # Only draw if we have a positive value
            if bar_height > 0:
                # Draw bar with rounded edges
                cr.save()
                cr.set_source(gradient)

                # Draw the bar path
                cr.move_to(inner_start_x, inner_start_y)
                cr.line_to(outer_start_x, outer_start_y)

                # Draw outer arc
                outer_radius = inner_radius + bar_height
                cr.arc(
                    center_x,
                    center_y,
                    outer_radius,
                    angle - half_width,
                    angle + half_width,
                )

                cr.line_to(inner_end_x, inner_end_y)

                # Draw inner arc
                cr.arc_negative(
                    center_x,
                    center_y,
                    inner_radius,
                    angle + half_width,
                    angle - half_width,
                )

                cr.close_path()
                cr.fill()

                # Add glow effect for more intensity
                if value > 0.3:
                    glow_alpha = value * 0.4
                    cr.set_source_rgba(0.9, 0.4, 0.0, glow_alpha)
                    cr.set_line_width(2)
                    cr.move_to(outer_start_x, outer_start_y)
                    cr.arc(
                        center_x,
                        center_y,
                        outer_radius,
                        angle - half_width,
                        angle + half_width,
                    )
                    cr.stroke()

                cr.restore()

        # Draw a subtle pulse effect around the center
        pulse_size = 1.0 + 0.1 * math.sin(time.time() * 3)  # Subtle pulsing effect
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

        # Draw noise reduction icon with a warm style
        self.draw_noise_reduction_icon(cr, width, height, "warm")

    def draw_noise_reduction_icon(self, cr, width, height, style):
        """Draw noise reduction icon in the center."""
        center_x = width / 2
        center_y = height / 2
        # Make the icon larger
        icon_radius = min(width, height) * 0.22  # Increased from 0.18 to 0.22

        # Draw the outer circle (background for the icon)
        cr.save()

        # Use consistent colors based on active state, regardless of style
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

        # Draw a more prominent ring around the icon
        cr.set_source_rgba(*ring_color)
        cr.set_line_width(3.0)  # Thicker ring
        cr.arc(center_x, center_y, icon_radius, 0, 2 * math.pi)
        cr.stroke()

        cr.restore()

        # Draw the SVG icon based on noise reduction status
        icon_drawn = False
        if HAS_RSVG:
            icon = self.icon_on if self.noise_reduction_active else self.icon_off
            if icon:
                try:
                    # Create a scale transformation to fit the icon in the circle
                    # Make the icon itself slightly smaller relative to the circle
                    icon_size = (
                        icon_radius * 0.6
                    )  # Adjusted from 1.2 to 1.0 to fit in the circle better

                    # Get the dimensions of the SVG
                    dim = icon.get_dimensions()
                    svg_width = dim.width
                    svg_height = dim.height

                    # Calculate the scale factors
                    scale_x = (icon_size * 2) / svg_width
                    scale_y = (icon_size * 2) / svg_height
                    scale = min(scale_x, scale_y)  # Use the smaller scale to fit

                    # Center the icon
                    cr.save()
                    cr.translate(
                        center_x - (svg_width * scale / 2),
                        center_y - (svg_height * scale / 2),
                    )
                    cr.scale(scale, scale)

                    # Render the SVG
                    icon.render_cairo(cr)
                    cr.restore()
                    icon_drawn = True
                except Exception as e:
                    print(f"Error rendering SVG icon: {e}")

        # Fallback to drawing a simple icon if SVG rendering fails
        if not icon_drawn:
            self.draw_fallback_icon(
                cr, center_x, center_y, icon_radius * 0.8, self.noise_reduction_active
            )

    def draw_fallback_icon(self, cr, cx, cy, radius, is_active):
        """Draw a fallback icon if SVG loading fails."""
        # Use green for active, gray for inactive, regardless of visualization style
        color = (0.2, 0.8, 0.2) if is_active else (0.6, 0.6, 0.6)

        # Draw a simplified icon representing noise reduction
        cr.save()

        # Draw sound wave symbol
        cr.set_line_width(2)
        cr.set_source_rgba(*color, 0.9)

        # Draw three arcs to represent sound waves
        sizes = [0.5, 0.7, 0.9]
        for size in sizes:
            arc_radius = radius * size
            if is_active:
                # When active, draw complete arcs
                cr.arc(cx, cy, arc_radius, math.pi * 0.8, math.pi * 2.2)
            else:
                # When inactive, draw broken arcs
                cr.arc(cx, cy, arc_radius, math.pi * 0.8, math.pi * 1.3)
                cr.stroke()
                cr.arc(cx, cy, arc_radius, math.pi * 1.7, math.pi * 2.2)
            cr.stroke()

        # Draw a center circle
        cr.set_source_rgba(*color, 0.9)
        cr.arc(cx, cy, radius * 0.2, 0, math.pi * 2)
        cr.fill()

        # If noise reduction is active, draw a checkmark
        if is_active:
            cr.set_source_rgba(1, 1, 1, 0.9)
            cr.set_line_width(2)
            cr.move_to(cx - radius * 0.1, cy)
            cr.line_to(cx, cy + radius * 0.1)
            cr.line_to(cx + radius * 0.2, cy - radius * 0.1)
            cr.stroke()

        cr.restore()

    def draw_retro_bars(self, cr, width, height):
        """Draws retro style equalizer bars."""
        # Set up dimensions for bars
        num_bars = 16
        bar_width = width / (num_bars * 1.5)
        bar_spacing = (width - (num_bars * bar_width)) / (num_bars + 1)
        max_bar_height = height * 0.75
        padding_bottom = height * 0.15

        # Draw background with a soft glow
        cr.save()
        bg_gradient = cairo.LinearGradient(0, 0, 0, height)
        bg_gradient.add_color_stop_rgba(0, 0.05, 0.1, 0.05, 0.4)  # Dark background top
        bg_gradient.add_color_stop_rgba(1, 0.02, 0.05, 0.02, 0.2)  # Darker bottom

        cr.set_source(bg_gradient)
        cr.rectangle(0, 0, width, height)
        cr.fill()
        cr.restore()

        # Draw grid lines for retro effect
        cr.save()
        cr.set_line_width(0.5)
        cr.set_source_rgba(0.1, 0.4, 0.1, 0.3)  # Soft grid lines

        # Horizontal lines
        grid_lines = 10
        for i in range(1, grid_lines):
            y = height - padding_bottom - (i * max_bar_height / grid_lines)
            cr.move_to(0, y)
            cr.line_to(width, y)

        # Vertical lines
        for i in range(num_bars + 1):
            x = bar_spacing + i * (bar_width + bar_spacing)
            cr.move_to(x, height - padding_bottom)
            cr.move_to(x, height - padding_bottom - max_bar_height)

        cr.stroke()
        cr.restore()

        # Draw each bar with green to red gradient
        for i in range(num_bars):
            # Get value from frequency data with proper mapping
            value = self.frequency_data[self.data_mapping.get(i, i)]

            # Calculate bar dimensions
            bar_height = value * max_bar_height
            x = bar_spacing + i * (bar_width + bar_spacing)
            y = height - padding_bottom - bar_height

            # Draw bar with gradient - green to yellow to red based on height
            cr.save()
            gradient = cairo.LinearGradient(
                0, height - padding_bottom, 0, height - padding_bottom - max_bar_height
            )
            gradient.add_color_stop_rgba(0, 0.1, 0.8, 0.1, 0.9)  # Green at bottom
            gradient.add_color_stop_rgba(0.5, 0.9, 0.9, 0.1, 0.9)  # Yellow in middle
            gradient.add_color_stop_rgba(1, 0.9, 0.1, 0.1, 0.9)  # Red at top

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

        # Draw horizontal baseline
        cr.save()
        cr.set_source_rgba(0.2, 0.8, 0.2, 0.7)
        cr.set_line_width(2)
        cr.move_to(0, height - padding_bottom)
        cr.line_to(width, height - padding_bottom)
        cr.stroke()
        cr.restore()

        # Draw noise reduction icon with a retro style
        self.draw_noise_reduction_icon(cr, width, height, "retro")

    def __del__(self):
        """Clean up resources when the widget is destroyed."""
        # Stop the device monitor
        if hasattr(self, "device_monitor") and self.device_monitor:
            self.device_monitor.stop()

        # Stop the pipeline
        if hasattr(self, "pipeline") and self.pipeline:
            self.pipeline.set_state(Gst.State.NULL)
