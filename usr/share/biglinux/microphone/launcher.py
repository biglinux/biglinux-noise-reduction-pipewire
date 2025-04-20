#!/usr/bin/env python3

import os
import sys
import subprocess
import importlib
import logging
import argparse


def main() -> None:
    """
    Simple launcher script that ensures all necessary modules are installed
    before launching the main application.
    """
    # configure logging early
    parser = argparse.ArgumentParser(
        description="Launch Noise Reducer UI with dependencies check"
    )
    parser.add_argument("--debug", action="store_true", help="Enable debug logging")
    args = parser.parse_args()

    level = logging.DEBUG if args.debug else logging.INFO
    logging.basicConfig(level=level, format="%(levelname)s:%(name)s: %(message)s")
    logger = logging.getLogger(__name__)

    logger.debug("Starting launcher")
    required_modules: list[str] = ["numpy"]

    # Check Python modules
    for module in required_modules:
        try:
            importlib.import_module(module)
        except ImportError:
            logger.info("Installing required module: %s", module)
            subprocess.check_call([
                sys.executable,
                "-m",
                "pip",
                "install",
                "--user",
                module,
            ])

    # Detect OS type and set appropriate package names and commands
    if os.path.exists("/etc/arch-release") or os.path.exists("/etc/manjaro-release"):
        # Arch Linux or Manjaro
        package_manager = "pacman"
        required_packages = [
            "gst-plugins-good",
            "pipewire-pulse",
            "gst-plugin-pipewire",
        ]
        check_cmd = ["pacman", "-Q"]
        install_hint = f"sudo pacman -S {' '.join(required_packages)}"
    else:
        # Assume Debian/Ubuntu
        package_manager = "apt"
        required_packages = ["gstreamer1.0-plugins-good", "gstreamer1.0-pipewire"]
        check_cmd = ["dpkg", "-s"]
        install_hint = f"sudo apt install {' '.join(required_packages)}"

    # Check if system packages are installed
    missing_packages = []
    try:
        for package in required_packages:
            cmd = check_cmd + [package]
            result = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            if result.returncode != 0:
                missing_packages.append(package)

        if missing_packages:
            logger.warning("Missing packages: %s", ", ".join(missing_packages))
            logger.info("You can install them with: %s", install_hint)

    except Exception as e:
        logger.error("Could not check system packages: %s", e)

    # Launch the main application
    script_dir = os.path.dirname(os.path.abspath(__file__))
    app_path = os.path.join(script_dir, "noise_reducer.py")

    if os.path.exists(app_path):
        # Set GTK application ID directly via environment variable for Wayland compatibility
        env = os.environ.copy()
        env["GDK_BACKEND"] = "wayland,x11"
        env["GTK_APPLICATION_ID"] = "br.com.biglinux.microphone"
        logger.debug("Launching main app: %s", app_path)
        subprocess.run([sys.executable, app_path], env=env)
    else:
        logger.error("Application file not found at %s", app_path)
        sys.exit(1)


if __name__ == "__main__":
    main()
