from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gtk

from biglinux_microphone import cli
from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.base import Page, column, scrolled
from biglinux_microphone.ui.state import calibration_window
from biglinux_microphone.utils.i18n import _


class VoiceVolumePage(Page):
    title = _("Voice volume")
    id = "voice_volume"
    icon = "audio-volume-high-symbolic"

    def build(self) -> Gtk.Widget:
        self._loading = True
        self._snapshot = None
        body = column()

        group = Adw.PreferencesGroup()
        self._status = Adw.ActionRow(title=_("Checking your microphone…"), subtitle="")
        self._status.set_focusable(False)
        self._status.add_prefix(
            Gtk.Image.new_from_icon_name("audio-volume-high-symbolic")
        )
        self._adjust = Gtk.Button(label=_("Adjust volume"), valign=Gtk.Align.CENTER)
        self._adjust.set_sensitive(False)
        self._adjust.connect(
            "clicked",
            weak_callback(self, lambda self, _b: self.ctx.navigate("calibrate")),
        )
        self._status.add_suffix(self._adjust)
        group.add(self._status)
        body.append(group)

        group = Adw.PreferencesGroup(
            title=_("Automatic volume"),
            description=_("Adjusts microphone volume within your chosen limits."),
        )
        self._automatic = Adw.SwitchRow(title=_("Follow my voice"))
        self._automatic.connect(
            "notify::active", weak_callback(self, type(self)._toggle_automatic)
        )
        group.add(self._automatic)
        self._low = Adw.SpinRow.new_with_range(0, cli.MAX_CAPTURE_PERCENT - 1, 1)
        self._low.set_title(_("Minimum volume (%)"))
        self._high = Adw.SpinRow.new_with_range(1, cli.MAX_CAPTURE_PERCENT, 1)
        self._high.set_title(_("Maximum volume (%)"))
        for row in (self._low, self._high):
            row.connect(
                "notify::value", weak_callback(self, type(self)._limits_changed)
            )
            group.add(row)
        self._apply = Gtk.Button(
            label=_("Apply volume limits"),
            halign=Gtk.Align.END,
            css_classes=["suggested-action"],
        )
        self._apply.connect(
            "clicked", weak_callback(self, lambda self, _: self._save_limits())
        )
        body.append(group)
        body.append(self._apply)
        return scrolled(body, max_width=840)

    def _toggle_automatic(self, row, _param) -> None:
        if self._loading:
            return
        if row.get_active():
            self._save_limits()
        else:
            self.ctx.state.set_level_range(self._snapshot.current_source, None)

    def _limits_changed(self, _row, _param) -> None:
        self._apply.set_sensitive(
            self._automatic.get_sensitive()
            and self._automatic.get_active()
            and self._low.get_value() < self._high.get_value()
        )

    def _save_limits(self) -> None:
        low, high = int(self._low.get_value()), int(self._high.get_value())
        if self._snapshot is not None and low < high:
            self.ctx.state.set_level_range(self._snapshot.current_source, (low, high))

    def sync(self, snapshot) -> None:
        previous = self._snapshot
        self._snapshot = snapshot
        available = snapshot.loaded and any(
            s.get("name") == snapshot.current_source and s.get("available", True)
            for s in snapshot.sources
        )
        self._adjust.set_sensitive(available)
        self._automatic.set_sensitive(available)
        if not snapshot.loaded:
            return
        limits = calibration_window(snapshot.calibration())
        self._loading = True
        try:
            self._automatic.set_active(limits is not None)
            if (
                previous is None
                or previous.current_source != snapshot.current_source
                or previous.calibration() != snapshot.calibration()
            ):
                here = snapshot.volume if snapshot.volume is not None else 100
                low, high = (
                    (round(limits[0] * 100), round(limits[1] * 100))
                    if limits is not None
                    else (
                        max(0, min(here - 20, cli.MAX_CAPTURE_PERCENT - 1)),
                        min(cli.MAX_CAPTURE_PERCENT, max(here + 20, 1)),
                    )
                )
                self._low.set_value(low)
                self._high.set_value(high)
            for row in (self._low, self._high):
                row.set_sensitive(available)
            self._limits_changed(None, None)
        finally:
            self._loading = False
        if not available:
            self._status.set_title(_("No microphone available"))
            self._status.set_subtitle(_("Choose a microphone to continue"))
        elif limits is not None:
            self._status.set_title(_("Automatic volume is on"))
            self._status.set_subtitle(
                _("Follows your voice between %d%% and %d%%")
                % (round(limits[0] * 100), round(limits[1] * 100))
            )
        elif snapshot.calibration().get("volume"):
            self._status.set_title(_("Adjusted for your voice"))
            self._status.set_subtitle(_("The volume was set for this microphone."))
        else:
            self._status.set_title(_("Not adjusted yet"))
            self._status.set_subtitle(
                _("Measure your voice once to set the ideal volume.")
            )
