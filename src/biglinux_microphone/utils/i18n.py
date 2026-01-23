#!/usr/bin/env python3
# -*- coding: utf-8 -*-
#
# i18n.py - Utilities for translation support
#
import gettext
import os
from typing import Callable

# Determine locale directory (works in AppImage and system install)
locale_dir = "/usr/share/locale"  # Default for system install

# Check if we're in an AppImage
if "APPIMAGE" in os.environ or "APPDIR" in os.environ:
    # Running from AppImage
    # i18n.py is in: src/biglinux_microphone/utils/i18n.py
    # We need to get to: usr/share/locale
    script_dir = os.path.dirname(os.path.abspath(__file__))  # src/biglinux_microphone/utils
    biglinux_microphone_dir = os.path.dirname(script_dir)  # src/biglinux_microphone
    src_dir = os.path.dirname(biglinux_microphone_dir)  # src
    appdir_root = os.path.dirname(src_dir)  # AppDir root (squashfs-root)
    appimage_locale = os.path.join(appdir_root, "usr", "share", "locale")  # usr/share/locale

    if os.path.isdir(appimage_locale):
        locale_dir = appimage_locale

# Configure the translation text domain for biglinux-noise-reduction-pipewire
gettext.bindtextdomain("biglinux-noise-reduction-pipewire", locale_dir)
gettext.textdomain("biglinux-noise-reduction-pipewire")

# Export _ directly as the translation function with explicit type
_: Callable[[str], str] = gettext.gettext
