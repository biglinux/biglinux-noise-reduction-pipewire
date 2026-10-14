# SPDX-FileCopyrightText: 2026 BigLinux contributors
# SPDX-License-Identifier: MIT
"""The copied big_gtk_kit module; dialogs need a display."""

import gc
import time

from big_gtk_kit import dialog
from gi.repository import Adw, GLib, Gtk


def pump():
    context = GLib.MainContext.default()
    for _ in range(50):
        while context.iteration(False):
            pass
        time.sleep(0.002)


def test_dialogs_closed_without_earlier_focus_finalize():
    Adw.init()
    dialog.install()
    window = Adw.Window(default_width=800, default_height=600)
    window.present()
    pump()
    finalized = {"entry": 0}

    def bump(*_args):
        finalized["entry"] += 1

    cycles = 25
    for _ in range(cycles):
        entry = Gtk.Entry()
        entry.weak_ref(bump)
        sheet = Adw.Dialog(child=entry)
        sheet.present(window)
        pump()
        entry.grab_focus()
        pump()
        sheet.force_close()
        del entry, sheet
        gc.collect()
        pump()
    window.destroy()
    assert finalized["entry"] == cycles
