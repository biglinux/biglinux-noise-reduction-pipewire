"""The sidebar's settings as rows of a list; each opens a card to adjust it.

A row shows what it applies. Its card opens beside the sidebar, level with
the row and joined to it by a bridge, as the pointer reaches the row, and
holds a short description and the controls. A click, tap or Enter opens it
for the keyboard until Escape or a click elsewhere. A row that only leads to
a dialog opens that dialog instead.
"""

import gettext
import weakref

from gi.repository import Adw, GLib, GObject, Graphene, Gtk, Pango

from big_gtk_kit.hover_cards import HoverCards
from biglinux_microphone.utils.i18n import _

# Content moving under a still pointer (wheel, kinetic scroll) is not hover.
SCROLL_QUIET_US = 300_000


def weak_callback(method):
    """Let a native signal invoke a Python owner without keeping it alive."""
    reference = weakref.WeakMethod(method)

    def invoke(*args):
        callback = reference()
        return callback(*args) if callback is not None else None

    return invoke


class ChoiceTiles(Gtk.Grid):
    """Choices as tiles in a grid of ``columns``, with the ``selected`` API of
    a combo row.

    A label whose name and detail are joined by an em dash shows the detail
    under the name; a tile without one centers its name. Each tile is a radio for screen readers and the
    keyboard, its radio out of sight: the picked tile is outlined instead.
    """

    selected = GObject.Property(type=GObject.TYPE_UINT, maximum=GLib.MAXUINT32)

    def __init__(self, title, labels, columns=2):
        super().__init__(
            row_spacing=6,
            column_spacing=6,
            column_homogeneous=True,
            accessible_role=Gtk.AccessibleRole.GROUP,
        )
        self.update_property([Gtk.AccessibleProperty.LABEL], [title])
        self.buttons = []
        for index, label in enumerate(labels):
            name, _dash, detail = label.partition(" — ")
            button = option_tile(name, detail)
            if self.buttons:
                button.set_group(self.buttons[0])
            button.connect("toggled", weak_callback(self._on_toggled), index)
            self.attach(button, index % columns, index // columns, 1, 1)
            self.buttons.append(button)
        self.connect("notify::selected", weak_callback(self._show_selected))
        self._show_selected()

    def _on_toggled(self, button, index):
        if button.get_active() and self.selected != index:
            self.selected = index

    def _show_selected(self, *_args):
        if self.selected < len(self.buttons):
            self.buttons[self.selected].set_active(True)

    def get_selected(self):
        return self.selected

    def set_selected(self, index):
        # As Adw.ComboRow: setting the shown value again notifies no one.
        if self.selected != index:
            self.selected = index


def option_tile(name, detail="", button=None):
    """A radio drawn as a tile, its ``detail`` under its name; ``button``
    restyles one that already exists."""
    text = Gtk.Box(
        orientation=Gtk.Orientation.VERTICAL,
        spacing=2,
        hexpand=True,
        valign=Gtk.Align.CENTER,
    )
    text.append(
        Gtk.Label(
            label=name,
            xalign=0 if detail else 0.5,
            justify=Gtk.Justification.CENTER,
            wrap=True,
            css_classes=["option-name"],
        )
    )
    if detail:
        text.append(
            Gtk.Label(label=detail, xalign=0, wrap=True, css_classes=["option-detail"])
        )
    tile = button or Gtk.CheckButton()
    tile.set_child(text)
    tile.add_css_class("option")
    # Named by its name alone, its detail read after it: GTK would name it by
    # both labels run together.
    tile.update_property([Gtk.AccessibleProperty.LABEL], [name])
    if detail:
        tile.update_property([Gtk.AccessibleProperty.DESCRIPTION], [detail])
    return tile


class SettingRow(Gtk.ListBoxRow):
    """An icon badge, the setting's name and its value, with a card.

    ``body`` holds the controls shown in the row's card; a row without one
    has no card. Activating a switch row flips ``switch``, and one with
    ``on_activate`` runs it, instead of opening the card. A row with a
    ``reset`` callable offers it in the card.
    """

    unavailable = None  # why the row is grayed

    def __init__(
        self,
        icon,
        title,
        description,
        body,
        switch=None,
        on_activate=None,
        reset=None,
    ):
        super().__init__(css_classes=["setting-row"], activatable=True)
        self.title = title
        self.description = description
        self.body = body
        self.switch = switch
        self.on_activate = on_activate
        self.applied = False
        self.cards = None  # weak reference, set by SettingCards.add_row
        self.update_property([Gtk.AccessibleProperty.LABEL], [title])

        box = Gtk.Box(spacing=12)
        # Filled when the setting applies: the state shows in a shape too,
        # not only in the color of the value.
        self.icon = Gtk.Image(
            icon_name=icon,
            valign=Gtk.Align.CENTER,
            css_classes=["setting-badge"],
            accessible_role=Gtk.AccessibleRole.PRESENTATION,
        )
        box.append(self.icon)
        text = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=2,
            valign=Gtk.Align.CENTER,
            hexpand=True,
        )
        text.append(
            Gtk.Label(
                label=title,
                xalign=0,
                wrap=True,
                lines=2,
                ellipsize=Pango.EllipsizeMode.END,
                css_classes=["setting-title"],
            )
        )
        # A grayed row says here why, not in a tooltip alone.
        self.reason_label = Gtk.Label(
            xalign=0,
            wrap=True,
            visible=False,
            css_classes=["caption", "setting-reason"],
        )
        text.append(self.reason_label)
        box.append(text)
        self.value_label = Gtk.Label(
            xalign=1,
            justify=Gtk.Justification.RIGHT,
            wrap=True,
            lines=2,
            ellipsize=Pango.EllipsizeMode.END,
            # Asks for its unwrapped width, so a short value keeps to one line
            # and wraps only in a narrow sidebar.
            natural_wrap_mode=Gtk.NaturalWrapMode.NONE,
            valign=Gtk.Align.CENTER,
            css_classes=["setting-value"],
        )
        self._value = ""
        box.append(self.value_label)
        if on_activate is not None:
            box.append(
                Gtk.Image(
                    icon_name="go-next-symbolic",
                    valign=Gtk.Align.CENTER,
                    accessible_role=Gtk.AccessibleRole.PRESENTATION,
                )
            )
        self.set_child(box)

        self.card = self.reset_button = None
        if body is None:
            return
        self.update_property([Gtk.AccessibleProperty.HAS_POPUP], [True])
        card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, width_request=260)
        # The row beside the card already shows its name and value; the card
        # opens with what the row lacks.
        card.append(
            Gtk.Label(
                label=description or "",
                xalign=0,
                wrap=True,
                max_width_chars=36,
                hexpand=True,
                visible=bool(description),
                css_classes=["card-description"],
            )
        )
        controls = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=12,
            margin_top=12,
            margin_bottom=16,
            margin_start=12,
            margin_end=12,
        )
        controls.append(body)
        if reset is not None:
            self.reset_button = Gtk.Button(label=_("Reset"), halign=Gtk.Align.END)
            self.reset_button.connect("clicked", lambda _b: reset())
            controls.append(self.reset_button)
        card.append(controls)
        # A bounded width: a long line in the body wraps instead of widening
        # the card.
        self.card = Adw.Clamp(child=card, maximum_size=340, tightening_threshold=340)

    def set_unavailable(self, reason):
        """Greyed with ``reason`` under its name in place of its value, or
        usable again."""
        self.unavailable = reason
        cards = self.cards() if self.cards else None
        if reason and cards and self.card is not None:
            cards.hover_cards.close(self)
        self.set_sensitive(reason is None)
        self.set_tooltip_text(reason)
        self.reason_label.set_text(reason or "")
        self.reason_label.set_visible(reason is not None)
        self.value_label.set_visible(reason is None)
        self._describe()

    def _describe(self):
        # A grayed row shows its reason in place of its value, and says so.
        if self.unavailable:
            text = self.unavailable
        elif self.description:
            text = f"{self._value}. {self.description}"
        else:
            text = self._value
        self.update_property([Gtk.AccessibleProperty.DESCRIPTION], [text])

    def show_value(self, text, applied=False):
        """Show the value on the row and to screen readers."""
        self.applied = applied
        self._value = text
        self.value_label.set_text(text)
        (self.add_css_class if applied else self.remove_css_class)("applied")
        if self.reset_button is not None:
            self.reset_button.set_sensitive(applied)
        self._describe()
        cards = self.cards() if self.cards else None
        if cards:
            cards.row_changed(self)


class ChoiceRow(SettingRow):
    """One of a few choices, named on the row and picked in its card.

    It has the ``selected`` API of a combo row. ``extra`` widgets follow the
    tiles in the card. With ``changes_result`` a choice other than the first
    counts as applied.
    """

    selected = GObject.Property(type=GObject.TYPE_UINT, maximum=GLib.MAXUINT32)

    def __init__(
        self,
        icon,
        title,
        description,
        labels,
        extra=(),
        columns=1,
        changes_result=False,
    ):
        self.changes_result = changes_result
        self.tiles = ChoiceTiles(title, labels, columns)
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        body.append(self.tiles)
        for widget in extra:
            body.append(widget)
        super().__init__(icon, title, description, body)
        self.labels = labels
        self.bind_property(
            "selected",
            self.tiles,
            "selected",
            GObject.BindingFlags.BIDIRECTIONAL | GObject.BindingFlags.SYNC_CREATE,
        )
        self.connect("notify::selected", weak_callback(self._show_selected))
        self._show_selected()

    def _show_selected(self, *_args):
        self.show_value(
            self.labels[self.selected].partition(" — ")[0],
            self.changes_result and self.selected != 0,
        )

    def focus_target(self):
        return self.tiles.buttons[self.selected]

    def get_selected(self):
        return self.selected

    def set_selected(self, index):
        # As Adw.ComboRow: setting the shown value again notifies no one.
        if self.selected != index:
            self.selected = index


class StepRow(SettingRow):
    """An effect in a few named steps, picked in its card; the first is off.
    It has the ``selected`` API of a combo row."""

    def __init__(self, icon, title, description, labels):
        self.steps = ChoiceTiles(title, labels)
        super().__init__(icon, title, description, self.steps)
        self.labels = labels
        self.steps.connect("notify::selected", weak_callback(self._show))
        self._show()

    def _show(self, *_args):
        step = self.steps.get_selected()
        self.show_value(self.labels[step], step > 0)

    def focus_target(self):
        return self.steps.buttons[self.steps.get_selected()]

    def get_selected(self):
        return self.steps.get_selected()

    def set_selected(self, index):
        self.steps.set_selected(index)


class SwitchRow(SettingRow):
    """An effect that is on or off, picked in its card's two tiles;
    activating the row flips it. ``switch`` holds the state."""

    def __init__(self, icon, title, description):
        self.tiles = ChoiceTiles(title, [_("Off"), _("On")], columns=2)
        super().__init__(icon, title, description, self.tiles, switch=Gtk.Switch())
        self.switch.connect("notify::active", weak_callback(self._show))
        self.tiles.connect("notify::selected", weak_callback(self._on_picked))
        self._show()

    def _on_picked(self, *_args):
        self.switch.set_active(self.tiles.get_selected() == 1)

    def _show(self, *_args):
        active = self.switch.get_active()
        self.tiles.set_selected(int(active))
        self.show_value(_("On") if active else _("Off"), active)

    def focus_target(self):
        return self.tiles.buttons[self.tiles.get_selected()]

    def get_active(self):
        return self.switch.get_active()

    def set_active(self, active):
        self.switch.set_active(active)


class SettingCards:
    """The groups of rows in the sidebar and the cards they open."""

    def __init__(self, scrolled, layer):
        self.scrolled = scrolled
        rtl = scrolled.get_default_direction() == Gtk.TextDirection.RTL
        self.side = Gtk.PositionType.LEFT if rtl else Gtk.PositionType.RIGHT
        # A card rises from its row: a bridge as tall as the row's ring joins
        # them.
        self.hover_cards = HoverCards(layer, self.side, bridge=True)
        self.hover_cards.can_open = weak_callback(self._can_open)
        self.hover_cards.edge = weak_callback(self._sidebar_edge)
        self.groups = []  # (rows, count label)
        self._scrolled_at = 0
        owner = weakref.proxy(self)
        self._scroll_handler = scrolled.get_vadjustment().connect(
            "value-changed", lambda _a: owner._note_scroll()
        )

    def add_group(self, title):
        """A group of rows, titled if ``title``; returns its box and list."""
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        count = Gtk.Label(css_classes=["dim-label", "caption"])
        if title:
            header = Gtk.Box(spacing=8)
            header.append(
                Gtk.Label(label=title, xalign=0, hexpand=True, css_classes=["heading"])
            )
            header.append(count)
            box.append(header)
        listbox = Gtk.ListBox(
            selection_mode=Gtk.SelectionMode.NONE,
            activate_on_single_click=True,
            css_classes=["boxed-list"],
        )
        listbox.connect("row-activated", weak_callback(self._on_activated))
        box.append(listbox)
        self.groups.append(([], count))
        listbox.group_index = len(self.groups) - 1
        return box, listbox

    def add_row(self, listbox, row):
        row.cards = weakref.ref(self)
        row.group_index = listbox.group_index
        self.groups[listbox.group_index][0].append(row)
        listbox.append(row)
        if row.card is not None:
            # A click gives the keyboard to the chosen tile, or the card's one
            # control.
            focus = getattr(row, "focus_target", lambda: row.body)
            self.hover_cards.attach(
                row, row, row.card, row.title, ["setting-card"], focus=focus
            )
        self.row_changed(row)

    def row_changed(self, row):
        rows, count = self.groups[row.group_index]
        # A row hidden until its feature is found counts once it shows.
        active = sum(r.applied for r in rows if r.get_visible())
        count.set_text(
            gettext.ngettext("{count} active", "{count} active", active).format(
                count=active
            )
            if active
            else ""
        )

    def _on_activated(self, _listbox, row):
        if row.switch is not None:
            row.switch.set_active(not row.switch.get_active())
        elif row.on_activate is not None:
            self.close()
            row.on_activate()
        elif row.card is not None:
            self.hover_cards.open_for_keyboard(row)

    def _can_open(self, _row):
        return GLib.get_monotonic_time() - self._scrolled_at >= SCROLL_QUIET_US

    def _sidebar_edge(self, _row):
        """Cards open just past the sidebar's edge, over the content."""
        root = self.scrolled.get_root()
        x = -6 if self.side == Gtk.PositionType.LEFT else self.scrolled.get_width() + 6
        found, edge = self.scrolled.compute_point(root, Graphene.Point().init(x, 0))
        return edge.x if found else 0

    def _note_scroll(self):
        self._scrolled_at = GLib.get_monotonic_time()
        # The pointer's card points where its row was.
        self.hover_cards.close_hover()

    def close(self, *_args):
        self.hover_cards.close_all()

    def cleanup(self):
        self.hover_cards.unparent_all()
        self.scrolled.get_vadjustment().disconnect(self._scroll_handler)
