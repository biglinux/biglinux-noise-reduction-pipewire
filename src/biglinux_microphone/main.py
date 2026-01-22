#!/usr/bin/env python3
"""
Entry point for BigLinux Microphone Settings application.

This module provides the main() function that initializes and runs
the GTK4/Libadwaita application.
"""

import argparse
import logging
import sys
from typing import NoReturn

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")


from biglinux_microphone.application import MicrophoneApplication

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(levelname)s:%(name)s: %(message)s",
)
logger = logging.getLogger(__name__)


def parse_args() -> argparse.Namespace:
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(
        description="BigLinux Microphone Settings - Configure microphone with AI noise reduction"
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="Enable debug logging",
    )
    parser.add_argument(
        "--version",
        action="store_true",
        help="Show version and exit",
    )
    return parser.parse_args()


def main() -> NoReturn:
    """
    Main entry point for the application.

    Initializes logging, parses arguments, and starts the GTK application.
    """
    args = parse_args()

    if args.version:
        from biglinux_microphone import __version__

        print(f"BigLinux Microphone Settings v{__version__}")
        sys.exit(0)

    if args.debug:
        logging.getLogger().setLevel(logging.DEBUG)
        logger.debug("Debug logging enabled")

    logger.info("Starting BigLinux Microphone Settings")

    # Create and run application
    app = MicrophoneApplication()
    exit_status = app.run(sys.argv)
    sys.exit(exit_status)


if __name__ == "__main__":
    main()
