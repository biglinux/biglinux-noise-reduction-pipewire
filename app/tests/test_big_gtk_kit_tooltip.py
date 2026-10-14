# SPDX-FileCopyrightText: 2026 BigLinux contributors
# SPDX-License-Identifier: MIT
"""The copied big_gtk_kit module; the card needs a display."""

import gc
import time
import weakref

from big_gtk_kit import tooltip
from gi.repository import GLib, Gtk


def pump():
    context = GLib.MainContext.default()
    for _ in range(50):
        while context.iteration(False):
            pass
        time.sleep(0.002)


def test_closed_windows_release_their_card_and_owner():
    """GObject weak notifications fire on finalization, which Python's
    collection of a wrapper alone does not prove."""
    tooltip.install()
    finalized = {"window": 0, "card": 0, "owner": 0}

    def count(name):
        def bump(*_args):
            finalized[name] += 1

        return bump

    cycles = 25
    for _ in range(cycles):
        owner = Gtk.Button(label="Save", tooltip_text="Save the document")
        window = Gtk.Window(child=owner)
        window.present()
        pump()
        engine = getattr(window, tooltip._ENGINE)
        tooltip._silence(owner)
        engine.owner = weakref.ref(owner)
        engine.show(instant=False)
        pump()
        card = engine.card
        assert card.get_visible()
        window.weak_ref(count("window"))
        card.weak_ref(count("card"))
        owner.weak_ref(count("owner"))
        del engine, card, owner
        window.destroy()
        del window
        gc.collect()
        pump()
    assert finalized == {"window": cycles, "card": cycles, "owner": cycles}


def test_only_real_markup_is_structured():
    assert tooltip._is_structured("<b>Volume</b>\n<small>50 %</small>")
    assert not tooltip._is_structured("Fish &amp; chips")
    assert not tooltip._is_structured("1 &lt; 2")
    assert not tooltip._is_structured(None)
