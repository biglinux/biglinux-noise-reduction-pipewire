from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gtk

from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.base import Page, column, scrolled
from biglinux_microphone.utils.i18n import _


class EchoRoutingPage(Page):
    title = _("Echo and routing")
    id = "echo_routing"
    icon = "audio-speakers-symbolic"

    def build(self) -> Gtk.Widget:
        self._loading = True
        body = column()
        group = Adw.PreferencesGroup(
            title=_("Echo cancellation"),
            description=_(
                "Prevents sound from your speakers returning through the microphone."
            ),
        )
        self._echo = Adw.ComboRow(title=_("Speaker echo"))
        self._echo.set_model(
            Gtk.StringList.new([_("Automatic"), _("Always on"), _("Off")])
        )
        self._echo.connect("notify::selected", weak_callback(self, type(self)._changed))
        group.add(self._echo)
        body.append(group)

        return scrolled(body, max_width=840)

    def _changed(self, row: Adw.ComboRow, _param) -> None:
        if self._loading:
            return
        values = ("auto", "on", "off")
        index = int(row.get_selected())
        if 0 <= index < len(values):
            self.ctx.state.set("echo", values[index])

    def sync(self, snapshot) -> None:
        if not snapshot.loaded:
            return
        self._loading = True
        try:
            self._echo.set_selected(
                {"auto": 0, "on": 1, "off": 2}.get(snapshot.echo_mode, 0)
            )
        finally:
            self._loading = False
