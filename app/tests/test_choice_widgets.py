"""The contract every choice in this window depends on.

Two things have to be true of a control that both *shows* a setting and
*changes* it, and neither is visible by looking at the screen: drawing the
saved value must not report it as a choice, and one click must produce exactly
one write. Get the first wrong and the window silently rewrites settings
nobody touched; get the second wrong and a grouped radio writes twice — once
for the option being unticked, once for the one being ticked.
"""

from __future__ import annotations

import unittest

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gdk, GLib, Gtk

from biglinux_microphone.ui.widgets.choice import ChoiceRow, render_segment, segmented

OPTIONS = (
    ("auto", "Automatic", "On during calls, off otherwise.", None),
    ("on", "Always on", "Adds a little delay.", None),
    ("off", "Off", "If you always wear headphones.", "audio-headphones-symbolic"),
)


class ChoiceRowContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        Adw.init()
        if Gdk.Display.get_default() is None:
            raise unittest.SkipTest("no display for GTK")

    def _row(self) -> tuple[ChoiceRow, list[str]]:
        """A row in a realized window, which a popover needs to pop up."""
        chosen: list[str] = []
        row = ChoiceRow("Echo cancellation", OPTIONS, chosen.append)
        group = Adw.PreferencesGroup()
        group.add(row)
        window = Gtk.Window(child=group)
        window.present()
        self._settle()
        self.addCleanup(window.destroy)
        return row, chosen

    @staticmethod
    def _settle() -> None:
        context = GLib.MainContext.default()
        while context.pending():
            context.iteration(False)

    def test_rendering_marks_one_option_and_writes_nothing(self) -> None:
        row, chosen = self._row()
        row.render("on")
        self.assertEqual(chosen, [], "drawing a reading is not a choice")
        marked = [value for value, check in row._checks.items() if check.get_active()]
        self.assertEqual(marked, ["on"])
        self.assertEqual(row.get_subtitle(), "Always on")

    def test_rendering_an_option_this_row_does_not_offer_marks_none(self) -> None:
        row, chosen = self._row()
        row.render("on")
        row.render("something-else")
        self.assertEqual(chosen, [])
        self.assertEqual(
            [value for value, check in row._checks.items() if check.get_active()], []
        )

    def test_choosing_writes_once_and_closes_the_panel(self) -> None:
        row, chosen = self._row()
        row.render("auto")
        row.emit("activated")
        self._settle()
        self.assertTrue(row.popover.get_visible(), "the row opens its own panel")
        row.choose("off")
        self._settle()
        self.assertEqual(chosen, ["off"], "one click, one write")
        self.assertFalse(row.popover.get_visible(), "choosing closes the panel")

    def test_every_option_is_a_radio(self) -> None:
        """One of several alternatives, and the platform says so.

        Grouping the check buttons is what turns them into radios for the
        screen reader; ungrouped they would be announced as independent
        tick-boxes, which describes a different control entirely.
        """
        row, _chosen = self._row()
        self.assertEqual(len(row._checks), len(OPTIONS))
        for check in row._checks.values():
            self.assertEqual(check.get_accessible_role(), Gtk.AccessibleRole.RADIO)

    def test_options_can_be_replaced_without_writing(self) -> None:
        row, chosen = self._row()
        row.set_options((("flat", "Flat", "", None), ("custom", "Custom", "", None)))
        row.render("custom")
        self.assertEqual(chosen, [])
        self.assertEqual(row.get_subtitle(), "Custom")


class SegmentedContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        Adw.init()
        if Gdk.Display.get_default() is None:
            raise unittest.SkipTest("no display for GTK")

    def test_rendering_does_not_report_a_choice(self) -> None:
        chosen: list[str] = []
        group = segmented(OPTIONS[:2], chosen.append)
        render_segment(group, "on")
        self.assertEqual(chosen, [])
        self.assertEqual(group.get_active_name(), "on")

    def test_a_real_change_reports_once(self) -> None:
        chosen: list[str] = []
        group = segmented(OPTIONS[:2], chosen.append)
        render_segment(group, "auto")
        group.set_active_name("on")
        self.assertEqual(chosen, ["on"])

    def test_a_value_outside_the_segments_leaves_none_active(self) -> None:
        chosen: list[str] = []
        group = segmented(OPTIONS[:2], chosen.append)
        render_segment(group, "auto")
        render_segment(group, "off")
        self.assertEqual(chosen, [])
        self.assertIsNone(group.get_active_name())


if __name__ == "__main__":
    unittest.main()
