# SPDX-FileCopyrightText: 2026 BigLinux contributors
# SPDX-License-Identifier: MIT
"""``AdwDialog``s that are released when they close; see ``src/dialog.rs``.

A dialog opened over a window where nothing had the focus is given back no focus
when it closes (libadwaita 1.9), so the focus stays inside the closing sheet and
GTK 4.22 keeps the dialog's whole widget tree alive. Taking the focus out of the
dialog as it closes lets it go.
"""

import gi

gi.require_version("Adw", "1")
gi.require_version("Gtk", "4.0")
from gi.repository import Adw, GObject, Gtk

_installed = False


def install():
    """Take the keyboard focus out of every ``AdwDialog`` as it closes."""
    global _installed
    if _installed:
        return
    _installed = True
    # A class's signals exist once the class is initialised.
    GObject.TypeClass.get(Adw.Dialog.__gtype__)
    GObject.add_emission_hook(Adw.Dialog, "closed", _on_closed)


def _on_closed(dialog):
    root = dialog.get_root()
    if isinstance(root, Gtk.Window):
        focus = root.get_focus()
        if focus is not None and focus.is_ancestor(dialog):
            root.set_focus(None)
    return True
