#!/usr/bin/env python3

import sys  # Used for sys.argv and sys.exit
import gi
import os  # Used for file operations
import asyncio  # Used for async operations
import threading  # Used for threading operations
from pathlib import Path  # Used for file path operations
import gettext  # Used for translations
import logging  # Used for logging
import subprocess  # Used for running shell commands
import json  # Used for settings load/save

# Set up gettext for translations
gettext.textdomain("biglinux-noise-reduction-pipewire")
_ = gettext.gettext

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, GLib, Gio, Gdk

from noise_reducer_service import NoiseReducerService
from audio_visualizer import AudioVisualizer

# setup module logger
logging.basicConfig(level=logging.INFO, format="%(levelname)s:%(name)s: %(message)s")
logger = logging.getLogger(__name__)


class NoiseReducerApp(Adw.Application):
    def __init__(self) -> None:
        super().__init__(
            application_id="com.biglinux.noise_reducer",
            flags=Gio.ApplicationFlags.DEFAULT_FLAGS,
        )
        self.connect("activate", self.on_activate)

        # Set proper color scheme via Adwaita style manager (to fix warnings)
        self.style_manager = Adw.StyleManager.get_default()
        self.style_manager.set_color_scheme(Adw.ColorScheme.PREFER_DARK)

    def on_activate(self, app: Adw.Application) -> None:
        self.service = NoiseReducerService()
        # Pass the service to the window
        self.win = NoiseReducerWindow(application=app, service=self.service)
        self.win.present()


class NoiseReducerWindow(Adw.ApplicationWindow):
    def __init__(self, *args, service=None, **kwargs):
        super().__init__(*args, **kwargs)
        self.service = service

        # Settings file path
        self.settings_file = (
            Path.home() / ".config" / "biglinux-noise-reducer" / "settings.json"
        )
        # Ensure directory exists
        self.settings_file.parent.mkdir(parents=True, exist_ok=True)

        # Load settings
        self.settings = self.load_settings()

        # Set up the window with saved dimensions
        saved_width = self.settings.get("window_width", 700)
        saved_height = self.settings.get("window_height", 640)
        self.set_default_size(saved_width, saved_height)
        self.set_size_request(400, 500)  # Minimum size remains unchanged

        # Apply maximized state if previously saved
        if self.settings.get("window_maximized", False):
            self.maximize()

        # Add state tracking to save window size/state changes
        self.setup_window_state_tracking()

        # Create UI elements in an idle handler to prevent allocation warnings
        self.setup_ui()

    def setup_window_state_tracking(self):
        """Set up tracking of window size and state to save in settings."""
        # Track window size changes
        self.size_controller = Gtk.EventControllerMotion.new()
        self.add_controller(self.size_controller)

        # Use a timeout to avoid saving on every tiny change
        self.size_change_timer = 0

        # Connect to window signals for state tracking
        self.connect("notify::default-width", self.on_window_size_changed)
        self.connect("notify::default-height", self.on_window_size_changed)
        self.connect("notify::maximized", self.on_window_maximized_changed)

    def on_window_size_changed(self, *args):
        """Handle window size changes."""
        # Use a timer to avoid excessive saving during resize
        if self.size_change_timer:
            GLib.source_remove(self.size_change_timer)

        # Only save if not maximized (as size is not relevant then)
        if not self.is_maximized():
            self.size_change_timer = GLib.timeout_add(500, self.save_window_size)

    def save_window_size(self):
        """Save current window size to settings."""
        width = self.get_width()
        height = self.get_height()

        if width > 0 and height > 0:
            self.settings["window_width"] = width
            self.settings["window_height"] = height
            self.save_settings()

        self.size_change_timer = 0
        return False  # Don't repeat timeout

    def on_window_maximized_changed(self, *args):
        """Handle window maximized state changes."""
        is_maximized = self.is_maximized()
        self.settings["window_maximized"] = is_maximized
        self.save_settings()

    def load_settings(self):
        """Load settings from JSON file."""
        try:
            if self.settings_file.exists():
                with open(self.settings_file, "r") as f:
                    return json.load(f)
        except Exception as e:
            logger.error(f"Error loading settings: {e}")

        # Default settings including window state
        return {
            "spectrum_style": 0,  # Default to STYLE_MODERN
            "window_width": 700,  # Default window width
            "window_height": 640,  # Default window height
            "window_maximized": False,  # Default maximize state
        }

    def save_settings(self):
        """Save current settings to JSON file."""
        try:
            with open(self.settings_file, "w") as f:
                json.dump(self.settings, f)
        except Exception as e:
            logger.error(f"Error saving settings: {e}")

    def on_spectrum_style_changed(self, style):
        """Callback for when the spectrum visualization style changes."""
        self.settings["spectrum_style"] = style
        self.save_settings()

    def setup_ui(self):
        # Main content box - set to expand and fill
        content_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        content_box.set_vexpand(True)
        content_box.set_hexpand(True)

        # Create an overlay for the loading spinner
        self.overlay = Gtk.Overlay()

        # Create header bar with GNOME style
        header_bar = Adw.HeaderBar.new()
        header_bar.set_show_end_title_buttons(True)

        # Add app icon directly to header bar (not as a button)
        icon_path = "/usr/share/icons/hicolor/scalable/apps/biglinux-noise-reduction-pipewire.svg"
        if os.path.exists(icon_path):
            try:
                file = Gio.File.new_for_path(icon_path)
                texture = Gdk.Texture.new_from_file(file)
                icon_image = Gtk.Image.new_from_paintable(texture)

                # Add margins to the icon image for proper spacing
                icon_image.set_pixel_size(25)
                icon_image.set_margin_start(10)
                icon_image.set_margin_end(6)
                icon_image.set_margin_top(4)
                icon_image.set_margin_bottom(4)

                # Add image directly to the header bar
                header_bar.pack_start(icon_image)
                logger.info("Added larger icon to header bar from: %s", icon_path)
            except Exception:
                logger.exception("Error loading icon for header bar")
                # Fallback to icon name
                icon_image = Gtk.Image.new_from_icon_name("applications-system")
                icon_image.set_pixel_size(64)  # Increased from 48
                icon_image.set_margin_start(10)
                header_bar.pack_start(icon_image)
        else:
            logger.warning("Header bar icon not found at: %s", icon_path)
            # Fallback to standard icon
            icon_image = Gtk.Image.new_from_icon_name("applications-system")
            icon_image.set_pixel_size(64)  # Increased from 48
            icon_image.set_margin_start(10)
            header_bar.pack_start(icon_image)

        # Set title with proper GNOME style
        title = Adw.WindowTitle.new(_("Microphone Settings"), "")
        header_bar.set_title_widget(title)

        # Add menu button to header
        menu_button = self.create_menu_button()
        header_bar.pack_end(menu_button)

        # Add spinner to header bar
        self.spinner = Gtk.Spinner()
        self.spinner.set_size_request(16, 16)
        self.spinner.set_visible(False)
        header_bar.pack_end(self.spinner)

        # Add header to content
        content_box.append(header_bar)

        # Create a scrolled window that expands properly
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)
        scrolled.set_hexpand(True)

        # Main content box with proper margins
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=24)
        main_box.set_margin_top(24)
        main_box.set_margin_bottom(24)
        main_box.set_margin_start(24)
        main_box.set_margin_end(24)
        main_box.set_vexpand(True)
        main_box.set_hexpand(True)

        # Sound waves section
        sound_card = Adw.PreferencesGroup.new()
        # sound_card.set_title("Sound Visualization")

        # Audio visualizer with card-style background
        visualizer_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        visualizer_box.set_margin_top(12)
        visualizer_box.set_margin_bottom(12)
        visualizer_box.add_css_class("card")
        visualizer_box.set_hexpand(True)

        # Use our enhanced AudioVisualizer which now only handles visualization
        # Pass the initial style from settings
        self.visualizer_widget = AudioVisualizer(
            initial_style=self.settings.get("spectrum_style", 0)
        )
        self.visualizer_widget.set_hexpand(True)
        # Connect the noise reducer service to the visualizer
        self.visualizer_widget.set_noise_reducer_service(self.service)
        # Connect the style change callback
        self.visualizer_widget.set_style_change_callback(self.on_spectrum_style_changed)
        visualizer_box.append(self.visualizer_widget)
        sound_card.add(visualizer_box)

        # Add sound card to main box
        main_box.append(sound_card)

        # Settings section - this is where we'll keep the actual noise reduction controls
        settings_group = Adw.PreferencesGroup.new()

        # Noise reduction toggle
        noise_row = Adw.ActionRow.new()
        noise_row.set_title(_("Background Noise Reduction"))
        noise_row.set_subtitle(_("Improve voice quality on calls and recordings"))
        noise_row.add_css_class("property")

        self.noise_switch = Gtk.Switch()
        self.noise_switch.set_valign(Gtk.Align.CENTER)

        # Get initial noise reduction status synchronously
        # to immediately set the correct switch state
        try:
            noise_status = subprocess.run(
                ["/bin/bash", self.service.actions_script, "status"],
                capture_output=True,
                text=True,
            ).stdout.strip()
            self.noise_switch.set_active(noise_status == "enabled")
        except Exception as e:
            logger.error(f"Error getting initial noise status: {e}")
            self.noise_switch.set_active(False)

        self.noise_switch.connect("state-set", self.on_noise_reduction_toggled)
        noise_row.add_suffix(self.noise_switch)
        noise_row.set_activatable_widget(self.noise_switch)
        settings_group.add(noise_row)

        # Bluetooth toggle
        bt_row = Adw.ActionRow.new()
        bt_row.set_title(_("Auto-activate Bluetooth Microphone"))
        bt_row.set_subtitle(_("May reduce audio quality"))
        bt_row.add_css_class("property")

        self.bt_switch = Gtk.Switch()
        self.bt_switch.set_valign(Gtk.Align.CENTER)
        if self.service.get_bluetooth_status() == "enabled":
            self.bt_switch.set_active(True)
        else:
            self.bt_switch.set_active(False)
        self.bt_switch.connect("state-set", self.on_bluetooth_toggled)
        bt_row.add_suffix(self.bt_switch)
        bt_row.set_activatable_widget(self.bt_switch)
        settings_group.add(bt_row)

        # Only add volume control option if we have a suitable command
        volume_control_command = self.get_volume_control_command()
        if volume_control_command:
            # Volume control button with GNOME style
            volume_row = Adw.ActionRow.new()
            volume_row.set_title(_("Volume Control"))
            volume_row.set_subtitle(
                _(
                    "Keep microphone volume below 70% and noise filter at 100% for best results."
                )
            )
            volume_row.add_css_class("property")

            volume_button = Gtk.Button.new_with_label(_("Open"))
            volume_button.set_valign(Gtk.Align.CENTER)
            volume_button.connect("clicked", self.on_volume_control_clicked)

            # Store the command to use when clicked - THIS WAS MISSING
            self._volume_control_command = volume_control_command

            volume_row.add_suffix(volume_button)
            volume_row.set_activatable_widget(volume_button)
            settings_group.add(volume_row)

            # Log that we found and added a volume control option
            logger.info(
                f"Added volume control option using: {' '.join(volume_control_command)}"
            )

        # Add settings card to main box
        main_box.append(settings_group)

        # Add main box to scrolled window
        scrolled.set_child(main_box)

        # Add scrolled window to content
        content_box.append(scrolled)

        # Create the loading overlay elements (initially hidden)
        self.loading_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.loading_box.set_valign(Gtk.Align.CENTER)
        self.loading_box.set_halign(Gtk.Align.CENTER)

        # Semi-transparent background
        self.loading_bg = Gtk.Box()
        self.loading_bg.set_hexpand(True)
        self.loading_bg.set_vexpand(True)
        self.loading_bg.add_css_class("dim-overlay")

        # Add a CSS provider for our custom styles
        css_provider = Gtk.CssProvider()
        css_provider.load_from_data(b"""
            .dim-overlay {
                background-color: rgba(0, 0, 0, 0.6);
            }
            
            .big-spinner {
                min-width: 64px;
                min-height: 64px;
            }
            
            .spinner-label {
                color: white;
                font-size: 16px;
                margin-top: 12px;
            }
        """)
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(),
            css_provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
        )

        # Large spinner
        self.loading_spinner = Gtk.Spinner()
        self.loading_spinner.add_css_class("big-spinner")
        self.loading_spinner.start()

        # Label
        self.loading_label = Gtk.Label.new(_("Starting noise reduction..."))
        self.loading_label.add_css_class("spinner-label")

        # Stack the spinner and label vertically
        self.loading_box.append(self.loading_spinner)
        self.loading_box.append(self.loading_label)

        # This will hold our actual content
        self.overlay.set_child(content_box)

        # This will be our overlay when needed
        # Initially not added to avoid showing it

        # Set main content to the overlay
        self.set_content(self.overlay)

        # Set initial switch states directly from service status
        self.initial_update_status()

    def show_loading_overlay(self):
        """Show a full-screen semi-transparent overlay with spinner."""
        # Ensure the overlay layers are added in the right order
        if self.loading_bg.get_parent() is None:
            self.overlay.add_overlay(self.loading_bg)

        if self.loading_box.get_parent() is None:
            self.overlay.add_overlay(self.loading_box)

        # Make them visible
        self.loading_bg.set_visible(True)
        self.loading_box.set_visible(True)
        self.loading_spinner.start()

    def hide_loading_overlay(self):
        """Hide the loading overlay."""
        if self.loading_bg.get_parent() is not None:
            self.overlay.remove_overlay(self.loading_bg)

        if self.loading_box.get_parent() is not None:
            self.overlay.remove_overlay(self.loading_box)

        self.loading_spinner.stop()

    def create_menu_button(self):
        # Create menu with GNOME style
        menu = Gio.Menu.new()

        # Add about action - fix: use win.about instead of app.about to match registration scope
        menu.append(_("About"), "win.about")

        # Create menu button
        menu_button = Gtk.MenuButton()
        menu_button.set_icon_name("open-menu-symbolic")
        menu_button.set_menu_model(menu)

        # Create about action
        about_action = Gio.SimpleAction.new("about", None)
        about_action.connect("activate", self.on_about_action)
        self.add_action(about_action)

        return menu_button

    def on_about_action(self, action, param):
        logger.debug("About dialog triggered")
        about = Adw.AboutWindow.new()
        about.set_application_name(_("Noise Reduction"))
        about.set_version("3.0")
        about.set_developer_name("BigLinux Team")
        about.set_license_type(Gtk.License.GPL_3_0)
        about.set_website("https://www.biglinux.com.br")
        about.set_issue_url(
            "https://github.com/biglinux/biglinux-noise-reduction-pipewire/issues"
        )
        about.set_application_icon("biglinux-noise-reduction-pipewire")
        about.set_copyright("© 2025 BigLinux Team")
        about.set_developers(["BigLinux Team"])

        # Load the icon directly for maximum compatibility
        icon_path = "/usr/share/icons/hicolor/scalable/apps/biglinux-noise-reduction-pipewire.svg"
        if os.path.exists(icon_path):
            try:
                # Create a paintable from the file using more direct GDK methods
                file = Gio.File.new_for_path(icon_path)
                icon_paintable = Gdk.Texture.new_from_file(file)
                about.set_application_icon_paintable(icon_paintable)
                logger.info("Icon loaded for About from: %s", icon_path)
            except Exception:
                logger.exception("Error loading icon for about dialog")
        else:
            logger.warning("Icon file not found: %s", icon_path)

        # Force dialog to be modal and ensure it's connected to the parent window
        about.set_modal(True)
        about.set_transient_for(self)
        about.present()

    def initial_update_status(self):
        """Non-async version of update_status() for initial setup"""
        try:
            # The noise switch already has its initial state set in setup_ui
            # This is just for periodic updates
            def run_async_in_thread():
                try:
                    loop = asyncio.new_event_loop()
                    asyncio.set_event_loop(loop)

                    noise_status = loop.run_until_complete(
                        self.service.get_noise_reduction_status()
                    )

                    # Update noise reduction switch if state has changed
                    current_state = self.noise_switch.get_active()
                    new_state = noise_status == "enabled"
                    if current_state != new_state:
                        GLib.idle_add(lambda: self.noise_switch.set_active(new_state))
                except Exception as e:
                    logger.error("Error in status update thread: %s", e)

            # Start periodic update check
            # Start the thread for first update
            threading.Thread(target=run_async_in_thread, daemon=True).start()

            # Set up a timer for periodic checks
            GLib.timeout_add(2000, self.update_status_wrapper)

        except Exception as e:
            logger.error("Error initializing status: %s", e)

    def update_status_wrapper(self):
        logger.debug("Scheduling periodic status update")
        # Start thread to run the async update_status function
        threading.Thread(target=self.run_update_status, daemon=True).start()
        return True  # Continue the timeout

    def run_update_status(self):
        """Run the update_status coroutine in a separate thread."""
        try:
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            try:
                # Run the update_status coroutine
                loop.run_until_complete(self.update_status())
            finally:
                loop.close()
        except Exception as e:
            logger.error("Error in run_update_status: %s", e)

    async def update_status(self) -> bool:
        """Periodic status update called by GLib timeout."""
        try:
            logger.debug("Fetching service status")

            # Get noise reduction status
            noise_status = await self.service.get_noise_reduction_status()

            # Don't get bluetooth status as we don't want to auto-update it
            # Only update the noise reduction status
            GLib.idle_add(lambda: self.update_ui_status(noise_status))

        except Exception:
            logger.exception("Error in update_status")

        return True

    def update_ui_status(self, noise_status, bt_status=None):
        """Update the UI based on service status."""
        try:
            # Update noise reduction switch
            self.noise_switch.set_active(noise_status == "enabled")

            # Don't automatically update bluetooth switch
            # Only do this if explicitly requested
            if (
                bt_status is not None
                and hasattr(self, "_update_bluetooth")
                and self._update_bluetooth
            ):
                logger.debug("Updating UI switch: bluetooth=%s", bt_status)
                self.bt_switch.set_active(bt_status == "enabled")
                self._update_bluetooth = False
        except Exception:
            logger.exception("Error updating UI status")

    def on_noise_reduction_toggled(self, switch, state):
        """Handle toggle of noise reduction switch."""
        # Show the overlay for both enabling and disabling noise reduction
        if state:
            # When enabling: use 4-second timeout
            self.loading_label.set_text(_("Starting noise reduction..."))
            self.show_loading_overlay()
            GLib.timeout_add(3000, self.hide_loading_overlay)
        else:
            # When disabling: use 1-second timeout
            self.loading_label.set_text(_("Disabling noise reduction..."))
            self.show_loading_overlay()
            GLib.timeout_add(1000, self.hide_loading_overlay)

        # Run the async operation in a separate thread
        def toggle_noise_reduction_thread():
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)

            try:
                if state:
                    loop.run_until_complete(self.service.start_noise_reduction())
                else:
                    loop.run_until_complete(self.service.stop_noise_reduction())

                # Update status after toggling
                noise_status = loop.run_until_complete(
                    self.service.get_noise_reduction_status()
                )
                GLib.idle_add(lambda: self.update_ui_status(noise_status))

            except Exception as e:
                logger.error("Error toggling noise reduction: %s", e)
            finally:
                loop.close()

        # Start the thread
        threading.Thread(target=toggle_noise_reduction_thread, daemon=True).start()

        # Always return False to let GTK handle the switch state
        return False

    def on_bluetooth_toggled(self, switch, state):
        """Handle toggle of bluetooth autoswitch."""
        try:
            if state:
                # Run enable-bluetooth command from actions.sh
                self.service.enable_bluetooth_autoswitch()
            else:
                self.service.disable_bluetooth_autoswitch()
        except Exception as e:
            logger.error("Error toggling bluetooth autoswitch: %s", e)

        # Always return False to let GTK handle the switch state
        return False

    def get_volume_control_command(self):
        """Determine the appropriate volume control command based on availability."""
        import shutil

        # Check for pavucontrol
        if shutil.which("pavucontrol"):
            return ["pavucontrol", "-t", "4"]

        # Check for pavucontrol-qt
        if shutil.which("pavucontrol-qt"):
            return ["pavucontrol-qt", "-t", "4"]

        # Check desktop environment for KDE
        if os.environ.get("XDG_CURRENT_DESKTOP", "").upper() == "KDE":
            if shutil.which("kcmshell6"):
                return ["kcmshell6", "kcm_pulseaudio"]

        # Check desktop environment for GNOME
        if os.environ.get("XDG_CURRENT_DESKTOP", "").upper() == "GNOME":
            if shutil.which("gnome-control-center"):
                return ["gnome-control-center", "sound"]

        # No suitable command found
        return None

    def on_volume_control_clicked(self, button):
        """Handle click on volume control button."""
        if hasattr(self, "_volume_control_command") and self._volume_control_command:
            try:
                # Use subprocess to run the command
                subprocess.Popen(self._volume_control_command)
                logger.info(
                    f"Launched volume control: {' '.join(self._volume_control_command)}"
                )
            except Exception as e:
                logger.error(f"Error launching volume control: {e}")


if __name__ == "__main__":
    logging.getLogger().setLevel(logging.DEBUG)  # enable debug in main
    app = NoiseReducerApp()
    app.set_application_id("br.com.biglinux.microphone")
    exit_status = app.run(sys.argv)
    sys.exit(exit_status)
