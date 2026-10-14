"""Entry point for the BigLinux Microphone noise-reduction UI."""

from __future__ import annotations

import argparse
import logging
import sys
from typing import NoReturn

import gi

from biglinux_microphone.utils.i18n import _

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

logger = logging.getLogger(__name__)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=_("BigLinux Microphone - real-time microphone noise reduction"),
    )
    parser.add_argument("--debug", action="store_true", help=_("Enable debug logging"))
    parser.add_argument(
        "--version", action="store_true", help=_("Show version and exit")
    )
    return parser.parse_args()


def main() -> NoReturn:
    """Configure logging, parse arguments and run the GTK application."""
    args = _parse_args()

    if args.version:
        from biglinux_microphone import __version__

        print(f"BigLinux Microphone {__version__}")
        sys.exit(0)

    logging.basicConfig(
        level=logging.DEBUG if args.debug else logging.INFO,
        format="%(levelname)s:%(name)s: %(message)s",
    )

    from biglinux_microphone.application import MicrophoneApplication

    app = MicrophoneApplication()
    sys.exit(app.run(sys.argv[:1]))


if __name__ == "__main__":
    main()
