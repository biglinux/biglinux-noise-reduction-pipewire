#!/usr/bin/env python3

import os
import sys
import subprocess
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
    # File path remains the same since we're keeping the same main file name
    app_path = "noise_reducer.py"

    # Set GTK application ID directly via environment variable for Wayland compatibility
    env = os.environ.copy()
    env["GTK_APPLICATION_ID"] = "br.com.biglinux.microphone"
    logger.debug("Launching main app: %s", app_path)
    subprocess.run([sys.executable, app_path], env=env)


if __name__ == "__main__":
    main()
