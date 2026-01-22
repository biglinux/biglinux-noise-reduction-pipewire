"""
Pytest configuration for BigLinux Microphone tests.

Provides fixtures and configuration for all test modules.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

# Add src to path for imports
src_path = Path(__file__).parent.parent / "src"
if str(src_path) not in sys.path:
    sys.path.insert(0, str(src_path))


@pytest.fixture
def temp_config_dir(tmp_path: Path) -> Path:
    """Provide a temporary configuration directory."""
    config_dir = tmp_path / "biglinux-microphone"
    config_dir.mkdir(parents=True, exist_ok=True)
    return config_dir
