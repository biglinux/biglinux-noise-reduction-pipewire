#!/usr/bin/env python3
"""
Internationalization utility for BigLinux Microphone Settings.

Provides a centralized translation function using gettext.
"""

import gettext
import logging
import os

logger = logging.getLogger(__name__)

# Constants
DOMAIN = "biglinux-noise-reduction-pipewire"
LOCALEDIR = "/usr/share/locale"

# Development localedir check
dev_localedir = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(__file__)))),
    "locale",
)
if os.path.isdir(dev_localedir):
    LOCALEDIR = dev_localedir


def setup_i18n():
    """Setup internationalization."""
    try:
        gettext.bindtextdomain(DOMAIN, LOCALEDIR)
        gettext.textdomain(DOMAIN)
        return gettext.gettext
    except Exception as e:
        logger.error(f"Failed to setup i18n: {e}")
        return lambda s: s


# Initialize bit
_ = setup_i18n()


def get_translator():
    """Get the translation function."""
    return _
