"""Explained-choice control built from native libadwaita/GTK rows.

Each option shows both its label and consequence. Rendering saved state never
invokes the write callback.
"""

from __future__ import annotations

from collections.abc import Callable, Sequence

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, GObject, Gtk

from biglinux_microphone.ui.callbacks import weak_callback

#: One option: the value the backend stores, the words for it, the consequence
#: of picking it, and an optional symbolic icon.
Option = tuple[str, str, str, str | None]

#: Width of the panel. Wide enough for a subtitle to read as a sentence rather
#: than as four words per line.
PANEL_WIDTH = 320


class ChoiceRow(Adw.ActionRow):
    """A setting, its current value, and a panel of options that explain
    themselves.

    `on_choose(value)` fires only for a choice somebody made. Rendering never
    calls it.
    """

    __gtype_name__ = "BigMicChoiceRow"

    def __init__(
        self,
        title: str,
        options: Sequence[Option],
        on_choose: Callable[[str], None],
        *,
        subtitle: str = "",
        heading: str = "",
        footer: Gtk.Widget | None = None,
    ) -> None:
        super().__init__(
            title=title, subtitle=subtitle, activatable=True, title_lines=0
        )
        self._on_choose = on_choose
        self._rendering = False
        self._checks: dict[str, Gtk.CheckButton] = {}
        self._options: list[Option] = []
        #: What the subtitle says when nothing is chosen yet, or when the saved
        #: value is not one of the options on offer here.
        self._fallback_subtitle = subtitle

        self.add_suffix(Gtk.Image.new_from_icon_name("pan-down-symbolic"))
        self.update_property([Gtk.AccessibleProperty.HAS_POPUP], [True])

        self._list = Gtk.ListBox(
            selection_mode=Gtk.SelectionMode.NONE, css_classes=["boxed-list"]
        )
        panel = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=12,
            margin_top=12,
            margin_bottom=12,
            margin_start=12,
            margin_end=12,
            width_request=PANEL_WIDTH,
        )
        if heading:
            panel.append(Gtk.Label(label=heading, xalign=0.0, css_classes=["heading"]))
        panel.append(self._list)
        if footer is not None:
            panel.append(footer)

        self.popover = Gtk.Popover(child=panel, position=Gtk.PositionType.BOTTOM)
        self.popover.set_parent(self)
        self.popover.update_property([Gtk.AccessibleProperty.LABEL], [heading or title])
        self.connect("activated", weak_callback(self, type(self)._open))

        self.set_options(options)

    def _open(self, _row: Adw.ActionRow) -> None:
        # A popover needs a surface to sit on, and asking for one before this
        # row is inside a window crashes the process rather than failing.
        if self.get_root() is not None:
            self.popover.popup()

    # ── options ───────────────────────────────────────────────────────────

    def set_options(self, options: Sequence[Option]) -> None:
        """Rebuild the panel. For lists that come from the backend — presets,
        saved set-ups, microphones — and change while the window is open."""
        self._rendering = True
        try:
            while (row := self._list.get_first_child()) is not None:
                self._list.remove(row)
            self._checks.clear()
            self._options = list(options)
            first: Gtk.CheckButton | None = None
            for value, title, subtitle, icon in self._options:
                row = Adw.ActionRow(
                    title=title,
                    subtitle=subtitle,
                    activatable=True,
                    title_lines=0,
                    subtitle_lines=0,
                )
                if icon:
                    row.add_prefix(Gtk.Image.new_from_icon_name(icon))
                check = Gtk.CheckButton(valign=Gtk.Align.CENTER, can_focus=False)
                # The words are on the row, not on the button, so the button
                # is what a screen reader reaches and it would otherwise be
                # announced as an unnamed option.
                check.update_property(
                    [Gtk.AccessibleProperty.LABEL, Gtk.AccessibleProperty.DESCRIPTION],
                    [title, subtitle],
                )
                if first is None:
                    first = check
                else:
                    check.set_group(first)
                check.connect(
                    "toggled", weak_callback(self, type(self)._on_toggled), value
                )
                row.add_suffix(check)
                # One signal path: clicking anywhere on the row is the check
                # being clicked, so there is no second way to choose.
                row.set_activatable_widget(check)
                self._list.append(row)
                self._checks[value] = check
        finally:
            self._rendering = False

    # ── rendering, which never writes ─────────────────────────────────────

    def render(self, value: str) -> None:
        """Show what is saved. Draws the mark and the summary; writes nothing."""
        self._rendering = True
        try:
            check = self._checks.get(value)
            if check is not None:
                check.set_active(True)
            else:
                # A value this row does not offer (set elsewhere, or not read
                # yet): leave every mark off rather than pointing at the wrong
                # one. Grouped checks cannot all be cleared through the group,
                # so each is cleared directly.
                for other in self._checks.values():
                    other.set_active(False)
            self.set_subtitle(self._summary(value))
        finally:
            self._rendering = False

    def _summary(self, value: str) -> str:
        for option_value, title, _subtitle, _icon in self._options:
            if option_value == value:
                return title
        return self._fallback_subtitle

    # ── choosing ──────────────────────────────────────────────────────────

    def _on_toggled(self, check: Gtk.CheckButton, value: str) -> None:
        if self._rendering or not check.get_active():
            return
        self.set_subtitle(self._summary(value))
        self.popover.popdown()
        self._on_choose(value)

    def choose(self, value: str) -> None:
        """Choose as a person would, through the same signal. For tests."""
        check = self._checks.get(value)
        if check is not None:
            check.set_active(True)

    # ── teardown ──────────────────────────────────────────────────────────

    def do_unroot(self) -> None:
        # A popover parented to a widget outlives that widget's removal from
        # the tree unless it is unparented here, and GTK says so loudly.
        if self.popover is not None:
            self.popover.unparent()
            self.popover = None
        Adw.ActionRow.do_unroot(self)


def segmented(
    options: Sequence[Option], on_choose: Callable[[str], None]
) -> Adw.ToggleGroup:
    """Two or three options, side by side, for a choice worth showing without
    being opened. `Adw.ToggleGroup` rather than linked buttons: it is the
    platform's own segmented control, so arrow keys move between the options
    and a screen reader reads them as one group of alternatives."""
    group = Adw.ToggleGroup(halign=Gtk.Align.CENTER)
    for value, title, subtitle, icon in options:
        toggle = Adw.Toggle(name=value, label=title)
        if icon:
            toggle.set_icon_name(icon)
        if subtitle:
            toggle.set_tooltip(subtitle)
        group.add(toggle)
    group.connect("notify::active-name", _segment_changed, on_choose)
    return group


def _segment_changed(
    group: Adw.ToggleGroup, _param: GObject.ParamSpec, on_choose: Callable[[str], None]
) -> None:
    name = group.get_active_name()
    if name:
        on_choose(name)


def render_segment(group: Adw.ToggleGroup, value: str) -> None:
    """Show which option is saved, without reporting it as a choice.

    The handler is blocked rather than guarded by a flag: the block is what
    GObject offers for exactly this, and a flag is one more thing that can be
    left set when something in between raises.
    """
    group.handler_block_by_func(_segment_changed)
    try:
        if group.get_toggle_by_name(value) is not None:
            group.set_active_name(value)
        else:
            # Chosen somewhere with more options than this control offers.
            group.set_active(Gtk.INVALID_LIST_POSITION)
    finally:
        group.handler_unblock_by_func(_segment_changed)
