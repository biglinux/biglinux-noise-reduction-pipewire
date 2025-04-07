#!/usr/bin/env python3

import sys  # Used for sys.argv and sys.exit
import gi
import json  # Used for settings load/save
import os  # Used for file operations
import asyncio  # Used for async operations
import threading  # Used for threading operations
import cairo  # Used for drawing operations
from pathlib import Path  # Used for file path operations
import gettext  # Used for translations
import locale  # Used for translations

# Set up gettext for translations
locale.setlocale(locale.LC_ALL, "")
gettext.bindtextdomain("biglinux-noise-reduction", "/usr/share/locale")
gettext.textdomain("biglinux-noise-reduction")
_ = gettext.gettext

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, GLib, Gio, Gdk
from gi.repository import GdkPixbuf

from noise_reducer_service import NoiseReducerService
from audio_visualizer import AudioVisualizer


class NoiseReducerApp(Adw.Application):
    def __init__(self):
        super().__init__(
            application_id="com.biglinux.noise_reducer",
            flags=Gio.ApplicationFlags.DEFAULT_FLAGS,
        )
        self.connect("activate", self.on_activate)
        self.settings_file = (
            Path.home() / ".config" / "biglinux-noise-reducer" / "settings.json"
        )
        self.settings_file.parent.mkdir(parents=True, exist_ok=True)

        # Set proper color scheme via Adwaita style manager (to fix warnings)
        self.style_manager = Adw.StyleManager.get_default()
        self.style_manager.set_color_scheme(Adw.ColorScheme.PREFER_DARK)

    def on_activate(self, app):
        self.service = NoiseReducerService()
        # Pass the settings file to the window
        self.win = NoiseReducerWindow(
            application=app, service=self.service, settings_file=self.settings_file
        )
        self.win.present()


class NoiseReducerWindow(Adw.ApplicationWindow):
    def __init__(self, *args, service=None, settings_file=None, **kwargs):
        super().__init__(*args, **kwargs)
        self.service = service
        self.settings = {}
        self.settings_file = settings_file

        # Load saved settings
        self.load_settings()

        # Set up the window
        self.set_default_size(630, 700)  # Increased from 500x550
        self.set_size_request(400, 550)  # Increased from 360x500

        # Create UI elements in an idle handler to prevent allocation warnings
        GLib.idle_add(self.setup_ui_when_ready)

    def setup_ui_when_ready(self):
        """Set up UI in an idle handler to fix allocation warnings"""
        self.setup_ui()

        # Start status polling with a proper wrapper after UI is ready
        GLib.timeout_add_seconds(3, self.update_status_wrapper)
        return False  # remove idle handler

    def load_settings(self):
        try:
            # Check if settings_file is defined and exists
            if (
                hasattr(self, "settings_file")
                and self.settings_file
                and self.settings_file.exists()
            ):
                with open(self.settings_file, "r") as f:
                    self.settings = json.load(f)
            else:
                # Initialize default settings
                self.settings = {
                    "noise_reduction_enabled": False,
                    "bluetooth_autoswitch_enabled": False,
                }
        except Exception as e:
            print(f"Error loading settings: {e}")
            self.settings = {
                "noise_reduction_enabled": False,
                "bluetooth_autoswitch_enabled": False,
            }

    def save_settings(self):
        try:
            if hasattr(self, "settings_file") and self.settings_file:
                with open(self.settings_file, "w") as f:
                    json.dump(self.settings, f)
            else:
                print("Warning: settings_file not defined")
        except Exception as e:
            print(f"Error saving settings: {e}")

    def setup_ui(self):
        # Create toast overlay for notifications
        self.toast_overlay = Adw.ToastOverlay.new()

        # Main content box - set to expand and fill
        content_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        content_box.set_vexpand(True)
        content_box.set_hexpand(True)

        # Create header bar with GNOME style
        header_bar = Adw.HeaderBar.new()
        header_bar.set_show_end_title_buttons(True)

        # Add app icon directly to header bar (not as a button)
        icon_path = "/usr/share/icons/hicolor/scalable/apps/biglinux-noise-reduction-pipewire.svg"
        if os.path.exists(icon_path):
            try:
                # Load icon using GdkPixbuf at a larger size (64x64) - increased from 48x48
                pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_size(icon_path, 64, 64)
                icon_image = Gtk.Image.new_from_pixbuf(pixbuf)

                # Add margins to the icon image for proper spacing
                icon_image.set_pixel_size(25)
                icon_image.set_margin_start(10)
                icon_image.set_margin_end(6)
                icon_image.set_margin_top(4)
                icon_image.set_margin_bottom(4)

                # Add image directly to the header bar
                header_bar.pack_start(icon_image)
                print(f"Added larger icon to header bar from: {icon_path}")
            except Exception as e:
                print(f"Error loading icon for header bar: {e}")
                # Fallback to icon name
                icon_image = Gtk.Image.new_from_icon_name("applications-system")
                icon_image.set_pixel_size(64)  # Increased from 48
                icon_image.set_margin_start(10)
                header_bar.pack_start(icon_image)
        else:
            print(f"Header bar icon not found at: {icon_path}")
            # Fallback to standard icon
            icon_image = Gtk.Image.new_from_icon_name("applications-system")
            icon_image.set_pixel_size(64)  # Increased from 48
            icon_image.set_margin_start(10)
            header_bar.pack_start(icon_image)

        # Set title with proper GNOME style
        title = Adw.WindowTitle.new(_("Noise Reduction"), "")
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
        self.visualizer_widget = AudioVisualizer()
        self.visualizer_widget.set_hexpand(True)
        # Connect the noise reducer service to the visualizer
        self.visualizer_widget.set_noise_reducer_service(self.service)
        visualizer_box.append(self.visualizer_widget)
        sound_card.add(visualizer_box)

        # Add sound card to main box
        main_box.append(sound_card)

        # Settings section - this is where we'll keep the actual noise reduction controls
        settings_group = Adw.PreferencesGroup.new()
        settings_group.set_title(_("Microphone Settings"))
        settings_group.set_description(
            _("Configure noise reduction and bluetooth settings")
        )

        # Noise reduction toggle with GNOME style
        noise_row = Adw.ActionRow.new()
        noise_row.set_title(_("Noise Reduction"))
        noise_row.set_subtitle(
            _(
                "Remove background noise and sounds that interfere with recordings and online calls"
            )
        )
        noise_row.add_css_class("property")

        self.noise_switch = Gtk.Switch()
        self.noise_switch.set_valign(Gtk.Align.CENTER)
        self.noise_switch.connect("state-set", self.on_noise_reduction_toggled)
        noise_row.add_suffix(self.noise_switch)
        noise_row.set_activatable_widget(self.noise_switch)
        settings_group.add(noise_row)

        # Bluetooth toggle with GNOME style
        bt_row = Adw.ActionRow.new()
        bt_row.set_title(_("Bluetooth Autoswitch"))
        bt_row.set_subtitle(
            _(
                "Automatically activate Bluetooth microphone when requested. Audio quality may decrease."
            )
        )
        bt_row.add_css_class("property")

        self.bt_switch = Gtk.Switch()
        self.bt_switch.set_valign(Gtk.Align.CENTER)
        self.bt_switch.connect("state-set", self.on_bluetooth_toggled)
        bt_row.add_suffix(self.bt_switch)
        bt_row.set_activatable_widget(self.bt_switch)
        settings_group.add(bt_row)

        # Add settings card to main box
        main_box.append(settings_group)

        # Add main box to scrolled window
        scrolled.set_child(main_box)

        # Add scrolled window to content
        content_box.append(scrolled)

        # Add content to toast overlay
        self.toast_overlay.set_child(content_box)

        # Set initial switch states - FIX: Use non-async function for initial state
        self.initial_update_status()

        # Set content
        self.set_content(self.toast_overlay)

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
        # Show about dialog with GNOME style - add debugging
        print("About dialog triggered")
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
                print(f"Icon loaded from: {icon_path}")
            except Exception as e:
                print(f"Error loading icon for about dialog: {e}")
        else:
            print(f"Icon file not found: {icon_path}")

        # Force dialog to be modal and ensure it's connected to the parent window
        about.set_modal(True)
        about.set_transient_for(self)
        about.present()

    def show_loading(self, show=True):
        self.spinner.set_visible(show)
        if show:
            self.spinner.start()
        else:
            self.spinner.stop()

    def show_toast(self, text):
        toast = Adw.Toast.new(text)
        toast.set_timeout(3)
        self.toast_overlay.add_toast(toast)

    def initial_update_status(self):
        """Non-async version of update_status() for initial setup"""
        # Set switches to default state until real status is retrieved
        self.noise_switch.set_active(False)
        self.bt_switch.set_active(False)

        # Use a separate thread to run the async code
        def run_async_in_thread():
            try:
                # Create a new event loop for this thread
                loop = asyncio.new_event_loop()
                asyncio.set_event_loop(loop)

                # Run the coroutines in this thread
                noise_status = loop.run_until_complete(
                    self.service.get_noise_reduction_status()
                )
                bt_status = loop.run_until_complete(self.service.get_bluetooth_status())

                # Update UI in the main thread
                GLib.idle_add(self.update_ui_status, noise_status, bt_status)
            except Exception as e:
                print(f"Error in status update thread: {e}")

        # Start the thread
        threading.Thread(target=run_async_in_thread, daemon=True).start()

    def update_status_wrapper(self):
        """Non-async wrapper for update_status to avoid coroutine warning."""
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
            print(f"Error in run_update_status: {e}")

    async def update_status(self):
        """Periodic status update called by GLib timeout."""
        try:
            # Indicate loading state
            self.show_loading(True)

            # Get current status values
            noise_status = await self.service.get_noise_reduction_status()
            bt_status = await self.service.get_bluetooth_status()

            # Update UI in the main thread
            GLib.idle_add(lambda: self.update_ui_status(noise_status, bt_status))

            # Hide loading indicator
            self.show_loading(False)

        except Exception as e:
            print(f"Error in update_status: {e}")
            self.show_loading(False)

        return True

    def update_ui_status(self, noise_status, bt_status):
        """Update the UI based on service status."""
        try:
            # Properly interpret the status strings and update switches
            self.noise_switch.set_active(noise_status == "enabled")
            self.bt_switch.set_active(bt_status == "enabled")

            # Update settings
            self.settings["noise_reduction_enabled"] = noise_status == "enabled"
            self.settings["bluetooth_autoswitch_enabled"] = bt_status == "enabled"
            self.save_settings()
        except Exception as e:
            print(f"Error updating UI status: {e}")

    def on_noise_reduction_toggled(self, switch, state):
        """Handle toggle of noise reduction switch."""
        # Show loading indicator
        self.show_loading(True)

        # Run the async operation in a separate thread
        def toggle_noise_reduction_thread():
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)

            try:
                if state:
                    loop.run_until_complete(self.service.start_noise_reduction())
                    GLib.idle_add(lambda: self.show_toast(_("Noise reduction enabled")))
                else:
                    loop.run_until_complete(self.service.stop_noise_reduction())
                    GLib.idle_add(
                        lambda: self.show_toast(_("Noise reduction disabled"))
                    )

                # Update status after toggling
                noise_status = loop.run_until_complete(
                    self.service.get_noise_reduction_status()
                )
                bt_status = loop.run_until_complete(self.service.get_bluetooth_status())
                GLib.idle_add(lambda: self.update_ui_status(noise_status, bt_status))

            except Exception as e:
                print(f"Error toggling noise reduction: {e}")
            finally:
                GLib.idle_add(lambda: self.show_loading(False))
                loop.close()

        # Start the thread
        threading.Thread(target=toggle_noise_reduction_thread, daemon=True).start()

        # Always return False to let GTK handle the switch state
        return False

    def on_bluetooth_toggled(self, switch, state):
        """Handle toggle of bluetooth autoswitch."""
        # Show loading indicator
        self.show_loading(True)

        # Run the async operation in a separate thread
        def toggle_bluetooth_thread():
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)

            try:
                if state:
                    loop.run_until_complete(self.service.enable_bluetooth_autoswitch())
                    GLib.idle_add(
                        lambda: self.show_toast(_("Bluetooth autoswitch enabled"))
                    )
                else:
                    loop.run_until_complete(self.service.disable_bluetooth_autoswitch())
                    GLib.idle_add(
                        lambda: self.show_toast(_("Bluetooth autoswitch disabled"))
                    )

                # Update status after toggling
                noise_status = loop.run_until_complete(
                    self.service.get_noise_reduction_status()
                )
                bt_status = loop.run_until_complete(self.service.get_bluetooth_status())
                GLib.idle_add(lambda: self.update_ui_status(noise_status, bt_status))

            except Exception as e:
                print(f"Error toggling bluetooth autoswitch: {e}")
            finally:
                GLib.idle_add(lambda: self.show_loading(False))
                loop.close()

        # Start the thread
        threading.Thread(target=toggle_bluetooth_thread, daemon=True).start()

        # Always return False to let GTK handle the switch state
        return False


if __name__ == "__main__":
    app = NoiseReducerApp()
    exit_status = app.run(sys.argv)
    sys.exit(exit_status)
