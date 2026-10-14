#!/usr/bin/python3 -I
"""Start BigLinux Microphone from its private, Python-version-neutral path.

The Arch/BigLinux package installs the pure-Python front-end below
``/usr/lib/biglinux-microphone`` instead of a versioned ``site-packages``
directory.  The launcher is symlinked from ``/usr/bin`` and resolves that
private directory at runtime, so a Python minor upgrade does not strand the
application sources in an old ``pythonX.Y`` tree.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import NoReturn

PROGRAM = "biglinux-microphone"
MINIMUM_PYTHON = (3, 10)


def _fail(message: str, exit_code: int = 127) -> NoReturn:
    print(f"{PROGRAM}: {message}", file=sys.stderr)
    raise SystemExit(exit_code)


def _application_root() -> Path:
    """Return the package-owned private directory containing this launcher."""
    return Path(__file__).resolve().parent


def main() -> NoReturn:
    if sys.version_info < MINIMUM_PYTHON:
        required = ".".join(map(str, MINIMUM_PYTHON))
        running = f"{sys.version_info.major}.{sys.version_info.minor}"
        _fail(f"Python {required} or newer is required; the active interpreter is Python {running}.")

    app_root = _application_root()
    package_init = app_root / "biglinux_microphone" / "__init__.py"
    if not package_init.is_file():
        _fail(
            "the installation is incomplete: "
            f"{package_init} was not found. Reinstall the package."
        )

    # Isolated mode (-I) intentionally omits the script directory and ignores
    # PYTHONPATH/user site-packages. Add only the root-owned application tree.
    sys.path.insert(0, str(app_root))

    try:
        from biglinux_microphone.main import main as application_main
    except ModuleNotFoundError as exc:
        if exc.name == "gi":
            _fail(
                "PyGObject is unavailable for the active Python interpreter. "
                "Update or reinstall python-gobject together with the system Python."
            )
        if exc.name == "biglinux_microphone":
            _fail("the application files could not be loaded. Reinstall the package.")
        raise

    application_main()
    raise AssertionError("application main() returned unexpectedly")


if __name__ == "__main__":
    main()
