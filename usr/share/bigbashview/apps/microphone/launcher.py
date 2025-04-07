#!/usr/bin/env python3

import os
import sys
import subprocess


def main():
    """
    Simple launcher script that ensures all necessary modules are installed
    before launching the main application.
    """
    # Check for required modules and install if missing
    required_modules = ["numpy"]

    # Check Python modules
    for module in required_modules:
        try:
            __import__(module)
        except ImportError:
            print(f"Installing required module: {module}")
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
            print(
                f"Some required packages are not installed: {', '.join(missing_packages)}"
            )
            print(f"You can install them with: {install_hint}")

    except Exception as e:
        print(f"Could not check system packages: {e}")

    # Launch the main application
    script_dir = os.path.dirname(os.path.abspath(__file__))
    app_path = os.path.join(script_dir, "noise_reducer.py")

    if os.path.exists(app_path):
        subprocess.run([sys.executable, app_path])
    else:
        print(f"Error: Application file not found at {app_path}")
        sys.exit(1)


if __name__ == "__main__":
    main()
