"""
Internationalization utility for BigLinux Microphone Settings.

Provides a centralized translation function using gettext.
"""

import gettext
import glob
import logging
import os

logger = logging.getLogger(__name__)

# The catalog domain must match the installed .mo name (the PKGBUILD compiles
# approved po/LINGUAS catalogs to biglinux-microphone.mo, the same GETTEXT_PACKAGE the Rust side uses).
DOMAIN = "biglinux-microphone"

# Installed, this file is <prefix>/lib/biglinux-microphone/biglinux_microphone/
# utils/i18n.py and the catalogs live in <prefix>/share/locale: /usr for the
# distribution package, /app inside the Flatpak.
_app_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LOCALEDIR = os.path.normpath(os.path.join(_app_root, "..", "..", "share", "locale"))

# In a checkout the same walk reaches app/, and the repository's generated
# locale/ sits one directory higher. Installed, that lands on <prefix>/lib/locale
# — glibc's own directory, which exists on every system and holds no catalog of
# ours. Merely finding a directory there is how the whole interface silently
# reverted to English once the package stopped installing into site-packages,
# so require the catalog itself to be present.
dev_localedir = os.path.join(os.path.dirname(_app_root), "locale")
if glob.glob(os.path.join(dev_localedir, "*", "LC_MESSAGES", DOMAIN + ".mo")):
    LOCALEDIR = dev_localedir


def setup_i18n():
    """Setup internationalization."""
    try:
        gettext.bindtextdomain(DOMAIN, LOCALEDIR)
        gettext.textdomain(DOMAIN)
        return gettext.gettext
    except Exception as e:  # noqa: BLE001
        logger.error(f"Failed to setup i18n: {e}")
        return lambda s: s


# Initialize bit
_ = setup_i18n()
