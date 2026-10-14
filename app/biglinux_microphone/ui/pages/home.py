"""The live panel: how the microphone sounds now and whether it is ready.

The settings live in the sidebar; this page shows their effect, offers the
delayed return of the voice and the notices that need a click.
"""

from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gtk

from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.base import Page, column, scrolled
from biglinux_microphone.ui.state import MODELS
from biglinux_microphone.ui.widgets.activity_meter import ActivityMeter
from biglinux_microphone.utils.i18n import _


class HomePage(Page):
    title = _("Home")
    id = "home"
    icon = "go-home-symbolic"
    in_sidebar = True

    def build(self) -> Gtk.Widget:
        self._loading = True
        self._snapshot = None
        body = column(spacing=10)
        body.set_margin_top(12)
        body.set_margin_bottom(12)
        self._notices = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, visible=False)
        body.append(self._notices)

        self._error = Adw.Banner(
            title="", button_label=_("Try again"), revealed=False, use_markup=False
        )
        self._error.connect(
            "button-clicked",
            weak_callback(self, lambda self, _b: self.ctx.state.reload()),
        )
        self._notices.append(self._error)

        self._source_notice = Adw.Banner(
            title=_("No microphone available"),
            button_label=_("Try again"),
            revealed=False,
        )
        self._source_notice.connect(
            "button-clicked", weak_callback(self, type(self)._source_notice_clicked)
        )
        self._notices.append(self._source_notice)

        self._runtime_notice = Adw.Banner(
            title="", button_label=_("Restore microphone"), revealed=False
        )
        self._runtime_notice.connect(
            "button-clicked", weak_callback(self, type(self)._restore)
        )
        self._notices.append(self._runtime_notice)

        body.append(self._status_card())

        # The level and the spectrum take the room the panel has.
        self._meter = ActivityMeter()
        self._meter.set_vexpand(True)
        self._meter_state = Gtk.Label(
            label=_("Speak to check your microphone"),
            xalign=0,
            wrap=True,
            css_classes=["caption"],
        )
        meter = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=8,
            vexpand=True,
            css_classes=["card", "live-card"],
        )
        meter.append(self._meter)
        meter.append(self._meter_state)
        body.append(meter)

        body.append(self._listen_actions())
        body.set_vexpand(True)
        body.add_css_class("live-panel")
        return scrolled(body, max_width=980)

    def _status_card(self) -> Gtk.Widget:
        # What the microphone is doing, and with what.
        self._status_icon = Gtk.Image(
            icon_name="audio-input-microphone-symbolic",
            pixel_size=32,
            valign=Gtk.Align.CENTER,
            css_classes=["status-badge"],
            accessible_role=Gtk.AccessibleRole.PRESENTATION,
        )
        self._status = Gtk.Label(
            label=_("Checking status…"), xalign=0, wrap=True, css_classes=["title-3"]
        )
        self._chain = Gtk.Label(
            xalign=0, wrap=True, use_markup=False, css_classes=["dim-label"]
        )
        text = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=2,
            valign=Gtk.Align.CENTER,
            hexpand=True,
        )
        text.append(self._status)
        text.append(self._chain)
        status = Gtk.Box(spacing=16, css_classes=["card", "live-card"])
        status.append(self._status_icon)
        status.append(text)
        return status

    def _listen_actions(self) -> Gtk.Widget:
        # An action, not a setting: the voice comes back two seconds later.
        self._listen_icon = Gtk.Image(
            icon_name="audio-headphones-symbolic",
            pixel_size=24,
            valign=Gtk.Align.CENTER,
            css_classes=["status-badge", "listen-badge"],
            accessible_role=Gtk.AccessibleRole.PRESENTATION,
        )
        title = Gtk.Label(label=_("Listen back"), xalign=0, css_classes=["heading"])
        hint = Gtk.Label(
            label=_("2 s later — use headphones"),
            xalign=0,
            wrap=True,
            css_classes=["dim-label"],
        )
        text = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=2,
            valign=Gtk.Align.CENTER,
            hexpand=True,
        )
        text.append(title)
        text.append(hint)
        self._listen_back = Gtk.Switch(valign=Gtk.Align.CENTER)
        self._listen_back.update_property(
            [Gtk.AccessibleProperty.LABEL, Gtk.AccessibleProperty.DESCRIPTION],
            [title.get_label(), hint.get_label()],
        )
        self._listen_back.connect(
            "notify::active", weak_callback(self, type(self)._listen_back_toggled)
        )
        card = Gtk.Box(spacing=16, css_classes=["card", "live-card"])
        card.append(self._listen_icon)
        card.append(text)
        card.append(self._listen_back)
        # The whole card flips the switch, as a row would.
        click = Gtk.GestureClick()
        click.connect(
            "released",
            weak_callback(
                self,
                lambda self, *_: (
                    self._listen_back.get_sensitive()
                    and self._listen_back.set_active(not self._listen_back.get_active())
                ),
            ),
        )
        text.add_controller(click)
        self._listen_card = card
        return card

    def _show_chain(self, snapshot, working: bool, failing: bool) -> None:
        self._status_icon.set_from_icon_name(
            "microphone-sensitivity-muted-symbolic"
            if snapshot.muted
            else "dialog-warning-symbolic"
            if failing
            else "audio-input-microphone-symbolic"
        )
        (
            self._status_icon.add_css_class
            if working
            else self._status_icon.remove_css_class
        )("working")
        description = next(
            (
                str(s.get("description") or s.get("name", ""))
                for s in snapshot.sources
                if s.get("name") == snapshot.current_source
            ),
            "",
        )
        model = MODELS.get(snapshot.chosen_model, ("", ""))[1]
        self._chain.set_text(
            " · ".join(
                part
                for part in (description, model if snapshot.filter_on else "")
                if part
            )
        )

    def _restore(self, _banner: Adw.Banner) -> None:
        if self._snapshot is not None and self._snapshot.muted:
            self.ctx.state.unmute()
        else:
            self.ctx.state.repair()

    def _source_notice_clicked(self, _banner: Adw.Banner) -> None:
        snapshot = self._snapshot
        if snapshot is not None and snapshot.loaded and snapshot.sources:
            self.ctx.navigate("source_picker")
        else:
            self.ctx.state.reload()

    def _listen_back_toggled(self, switch: Gtk.Switch, _param) -> None:
        (
            self._listen_icon.add_css_class
            if switch.get_active()
            else self._listen_icon.remove_css_class
        )("working")
        if not self._loading and switch.get_active() != self.ctx.shell.monitor_active:
            self.ctx.shell.set_monitor(switch.get_active())

    def on_monitor(self, active: bool) -> None:
        self._loading = True
        try:
            self._listen_back.set_active(active)
        finally:
            self._loading = False

    def on_hidden(self) -> None:
        self._meter.clear()

    def shutdown(self) -> None:
        self._meter.clear()

    def sync(self, snapshot) -> None:
        previous = self._snapshot
        self._snapshot = snapshot
        self._loading = True
        try:
            if (
                previous is not None
                and previous.capture_identity != snapshot.capture_identity
            ):
                self._meter.clear()
                self._meter_state.set_text(_("Speak to check your microphone"))
            self._error.set_revealed(bool(snapshot.error))
            if snapshot.error:
                self._error.set_title(str(snapshot.error)[:180])

            has_source = bool(snapshot.current_source) and any(
                source.get("name") == snapshot.current_source
                and source.get("available", True)
                for source in snapshot.sources
            )
            missing_default = bool(
                snapshot.loaded and snapshot.sources and not has_source
            )
            no_sources = bool(snapshot.loaded and not snapshot.sources)
            self._source_notice.set_revealed(no_sources or missing_default)
            if missing_default:
                self._source_notice.set_title(_("Choose a microphone to continue"))
                self._source_notice.set_button_label(_("Choose microphone"))
            else:
                self._source_notice.set_title(_("No microphone available"))
                self._source_notice.set_button_label(_("Try again"))
            self._listen_back.set_sensitive(
                snapshot.ready or self.ctx.shell.monitor_active
            )

            if snapshot.loaded:
                updating = self.ctx.state.is_pending(
                    "denoiser"
                ) or self.ctx.state.is_pending("__repair__")
                runtime = snapshot.runtime
                unavailable = not runtime.get("audio_available")
                unapplied = bool(
                    runtime.get("mic_enabled")
                    and (
                        not runtime.get("mic_running")
                        or runtime.get("denoising") != snapshot.filter_on
                    )
                )
                self._runtime_notice.set_revealed(
                    bool(has_source and (unavailable or unapplied or snapshot.muted))
                )
                self._runtime_notice.set_button_label(
                    ""
                    if updating
                    else (_("Unmute") if snapshot.muted else _("Restore microphone"))
                )
                if updating:
                    status = _("Updating microphone…")
                elif unavailable:
                    status = _("Audio service unavailable")
                elif snapshot.muted:
                    status = _("Your microphone is muted")
                elif unapplied:
                    status = _("The microphone filter is not ready")
                elif snapshot.filter_on:
                    status = (
                        _("Ready — starts when an application uses the microphone")
                        if runtime.get("mic_state") in {"idle", "suspended"}
                        else _("On — reducing background noise")
                    )
                else:
                    status = _("Off — background noise is not reduced")
                self._status.set_text(status)
                self._show_chain(
                    snapshot,
                    snapshot.filter_on
                    and not (unavailable or unapplied or snapshot.muted or updating),
                    unavailable or unapplied,
                )
                self._runtime_notice.set_title(status)
                if unavailable or unapplied or snapshot.muted or not has_source:
                    self._meter.clear()
                    self._meter_state.set_text(
                        _("Microphone unavailable")
                        if not snapshot.muted
                        else _("Microphone muted")
                    )
                elif previous is None or not previous.ready or previous.muted:
                    self._meter_state.set_text(_("Speak to check your microphone"))
            else:
                self._status.set_text(_("Loading…"))
        finally:
            self._notices.set_visible(
                any(
                    banner.get_revealed()
                    for banner in (
                        self._error,
                        self._source_notice,
                        self._runtime_notice,
                    )
                )
            )
            self._loading = False

    def on_bands(self, bands: list[float]) -> None:
        self._meter.set_bands(bands)

    def on_peak(self, peak: float) -> None:
        self._meter.set_value(peak)
        level = self._meter.level_db
        if level <= -60:
            text = _("No sound")
        elif level < -20:
            text = _("Sound detected — speak normally")
        elif level < -9:
            text = _("Good level")
        elif level < -0.5:
            text = _("Near the limit — lower the microphone volume")
        else:
            text = _("Too loud — lower the microphone volume")
        if self._meter_state.get_text() != text:
            self._meter_state.set_text(text)
