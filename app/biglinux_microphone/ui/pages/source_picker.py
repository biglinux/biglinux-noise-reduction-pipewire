from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gtk

from biglinux_microphone import cli
from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.base import Page, column, scrolled
from biglinux_microphone.utils.i18n import _


class SourcePickerPage(Page):
    id = "source_picker"
    icon = "audio-input-microphone-symbolic"
    in_sidebar = False

    def build(self) -> Gtk.Widget:
        self.title = _("Select microphone")
        self._body = column()
        self._status = Adw.StatusPage(
            icon_name="audio-input-microphone-symbolic",
            title=_("No microphones available"),
            description=_(
                "Connect a microphone. For Bluetooth, enable the headset microphone in your system sound settings."
            ),
            visible=False,
        )
        self._body.append(self._status)
        self._groups = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)
        self._body.append(self._groups)
        self._snapshot = None
        self._source_signature = None
        self._shown = False
        self._closed = False
        self._choosing = False
        self._generation = 0
        self._rows = {}
        return scrolled(self._body, max_width=760)

    def sync(self, snapshot) -> None:
        self._snapshot = snapshot
        signature = (
            snapshot.loaded,
            snapshot.current_source,
            tuple(
                (
                    s.get("name", ""),
                    s.get("description", ""),
                    s.get("virtual"),
                    s.get("available"),
                )
                for s in snapshot.sources
            ),
        )
        # A volume/settings update must not destroy the row holding keyboard focus.
        if signature != self._source_signature:
            self._source_signature = signature
            self._rebuild(snapshot)

    def on_shown(self) -> None:
        self._shown = True
        self._generation += 1
        self.ctx.state.reload()

    def on_hidden(self) -> None:
        self._shown = False
        self._generation += 1

    def shutdown(self) -> None:
        self._closed = True
        self.on_hidden()

    def _rebuild(self, snapshot) -> None:
        root = self._groups.get_root()
        focus = root.get_focus() if isinstance(root, Gtk.Window) else None
        focused_name = next(
            (
                name
                for name, row in self._rows.items()
                if focus is row or (focus is not None and focus.is_ancestor(row))
            ),
            None,
        )
        self._rows = {}
        child = self._groups.get_first_child()
        while child is not None:
            nxt = child.get_next_sibling()
            self._groups.remove(child)
            child = nxt
        sources = list(snapshot.sources)
        self._status.set_visible(snapshot.loaded and not sources)
        self._groups.set_visible(bool(sources))
        if not sources:
            return

        current = Adw.PreferencesGroup(title=_("Microphone in use"))
        others = Adw.PreferencesGroup(title=_("Other microphones available"))
        virtual = Adw.PreferencesGroup(
            title=_("Microphones from other applications"),
            description=_("These inputs may already apply sound effects."),
        )
        current_found = False
        for source in sources:
            name = str(source.get("name", ""))
            label = str(source.get("description") or name)
            row = Adw.ActionRow(title=label, use_markup=False)
            self._rows[name] = row
            row.add_prefix(
                Gtk.Image.new_from_icon_name("audio-input-microphone-symbolic")
            )
            if name == snapshot.current_source:
                row.add_suffix(Gtk.Image.new_from_icon_name("object-select-symbolic"))
                row.set_subtitle(_("In use"))
                current.add(row)
                current_found = True
            else:
                row.set_activatable(True)
                row.connect("activated", weak_callback(self, type(self)._choose), name)
                (virtual if source.get("virtual") else others).add(row)
            if not source.get("available", True):
                row.set_subtitle(_("Input disconnected"))
                row.set_sensitive(False)
        if current_found:
            self._groups.append(current)
        # Adw.PreferencesGroup has no simple row count accessor; only add when
        # there is at least one non-current source.
        if any(
            s.get("name") != snapshot.current_source and not s.get("virtual")
            for s in sources
        ):
            self._groups.append(others)
        if any(
            s.get("name") != snapshot.current_source and s.get("virtual")
            for s in sources
        ):
            self._groups.append(virtual)
        if focused_name in self._rows:
            self._rows[focused_name].grab_focus()

    def _choose(self, _row: Adw.ActionRow, name: str) -> None:
        if not name or self._closed or self._choosing:
            return
        self._choosing = True
        self._groups.set_sensitive(False)
        generation = self._generation

        def completed(message=None) -> None:
            self._choosing = False
            if self._closed:
                return
            self._groups.set_sensitive(True)
            self.ctx.state.reload()
            if self._shown and generation == self._generation:
                if message is None:
                    self.ctx.navigate("home")
                else:
                    self.ctx.toast(str(message)[:200])

        self.ctx.state.call(
            lambda: cli.set_default_source(name),
            lambda _result: completed(),
            completed,
        )
