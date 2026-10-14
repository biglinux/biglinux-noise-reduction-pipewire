# SPDX-FileCopyrightText: 2026 BigLinux contributors
# SPDX-License-Identifier: MIT
"""The copied big_gtk_kit module; surfaces need a display."""

import gc
import time

import pytest
from big_gtk_kit import surface
from gi.repository import Gdk, GLib, Gtk


def pump():
    context = GLib.MainContext.default()
    for _ in range(50):
        while context.iteration(False):
            pass
        time.sleep(0.002)


# surface.py releases what GDK's Wayland popups keep. X11 popups keep their
# parent surface too (GTK 4.22.4) and nothing releases it yet.
@pytest.mark.skipif(
    Gdk.Display.get_default().__gtype__.name != "GdkWaylandDisplay",
    reason="the fix covers Wayland only",
)
def test_windows_that_showed_a_popover_release_their_surface():
    surface.install()
    freed = {"surfaces": 0}

    def bump(*_args):
        freed["surfaces"] += 1

    cycles = 25
    for _ in range(cycles):
        popover = Gtk.Popover(child=Gtk.Label(label="x"))
        button = Gtk.MenuButton(popover=popover)
        window = Gtk.Window(child=button)
        window.present()
        pump()
        window.get_surface().weak_ref(bump)
        button.popup()
        pump()
        button.popdown()
        pump()
        window.destroy()
        del popover, button, window
        gc.collect()
        pump()
    # GDK holds the last closed window's surface until another window comes.
    last = Gtk.Window()
    last.present()
    pump()
    last.destroy()
    pump()
    assert freed["surfaces"] == cycles
