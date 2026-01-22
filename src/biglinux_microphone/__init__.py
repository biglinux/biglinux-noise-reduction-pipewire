"""
BigLinux Microphone Settings Application.

A modern GTK4/Libadwaita application for configuring microphone
settings with AI-powered noise reduction, stereo enhancement,
and parametric equalization.

Usage:
    # From installed package
    python -m biglinux_microphone

    # Direct execution (development)
    cd src/biglinux_microphone
    python __init__.py
"""

import sys
from pathlib import Path

__version__ = "5.0.0"
__author__ = "BigLinux Team"
__license__ = "GPL-3.0"

# Support direct execution without installation
# Add parent directory to path so imports work
_current_file = Path(__file__).resolve()
_package_dir = _current_file.parent
_src_dir = _package_dir.parent

if str(_src_dir) not in sys.path:
    sys.path.insert(0, str(_src_dir))

from biglinux_microphone.main import main

__all__ = ["main", "__version__"]


if __name__ == "__main__":
    sys.exit(main())
