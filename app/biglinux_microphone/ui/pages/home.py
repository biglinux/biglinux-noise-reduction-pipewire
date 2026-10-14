"""Beginner-first home page."""

from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gtk

from biglinux_microphone import cli
from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.base import Page, column, scrolled, slider_row
from biglinux_microphone.ui.pages.common import (
    action_button,
    adaptive_button_row,
    attach_chevron,
)
from biglinux_microphone.ui.state import MODELS, calibration_window
from biglinux_microphone.ui.widgets.activity_meter import ActivityMeter
from biglinux_microphone.utils.async_utils import Debouncer
from biglinux_microphone.utils.i18n import _

_MODEL_ORDER = tuple(MODELS)


class HomePage(Page):
    title = _("Home")
    id = "home"
    icon = "go-home-symbolic"
    in_sidebar = True

    def build(self) -> Gtk.Widget:
        self._loading = True
        self._snapshot = None
        self._volume_debounce = Debouncer(180)
        body = column(spacing=10)
        body.set_margin_top(10)
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

        self._meter = ActivityMeter()
        body.append(self._meter)
        self._meter_state = Gtk.Label(
            label=_("Speak to check your microphone"),
            xalign=0,
            wrap=True,
            css_classes=["caption"],
        )
        body.append(self._meter_state)

        main = Adw.PreferencesGroup()
        self._noise = Adw.SwitchRow(
            title=_("Noise reduction"), subtitle=_("Checking status…")
        )
        self._noise.add_prefix(Gtk.Image.new_from_icon_name("audio-x-generic-symbolic"))
        self._noise.connect(
            "notify::active", weak_callback(self, type(self)._noise_toggled)
        )
        main.add(self._noise)

        self._mode = Adw.ComboRow(
            title=_("Noise-reduction model"),
            model=Gtk.StringList.new(
                [_("DFN3 (recommended)"), MODELS[12][1], MODELS[13][1]]
            ),
        )
        self._mode.connect(
            "notify::selected", weak_callback(self, type(self)._mode_changed)
        )
        main.add(self._mode)

        self._source = Adw.ActionRow(
            title=_("Microphone in use"),
            subtitle=_("Looking for microphones…"),
            use_markup=False,
        )
        self._source.add_prefix(
            Gtk.Image.new_from_icon_name("audio-input-microphone-symbolic")
        )
        attach_chevron(self._source)
        self._source.connect(
            "activated",
            weak_callback(self, lambda self, _r: self.ctx.navigate("source_picker")),
        )
        main.add(self._source)
        body.append(main)

        volume = Adw.PreferencesGroup()
        self._volume = Adw.ActionRow(
            title=_("Voice volume"), subtitle=_("Checking your microphone…")
        )
        self._volume.set_focusable(False)
        self._volume.add_prefix(
            Gtk.Image.new_from_icon_name("audio-volume-high-symbolic")
        )
        self._adjust_again = Gtk.Button(
            label=_("Adjust volume"), valign=Gtk.Align.CENTER
        )
        self._adjust_again.connect(
            "clicked",
            weak_callback(self, lambda self, _: self.ctx.navigate("calibrate")),
        )
        self._volume.add_suffix(self._adjust_again)
        volume.add(self._volume)
        self._volume_scale = Gtk.Scale.new_with_range(
            Gtk.Orientation.HORIZONTAL, 0, cli.MAX_CAPTURE_PERCENT, 1
        )
        self._volume_scale.set_draw_value(False)
        self._volume_scale.connect(
            "value-changed", weak_callback(self, type(self)._volume_changed)
        )
        self._volume_value = Gtk.Label(label="", css_classes=["numeric"])
        self._volume_row = slider_row(
            _("Microphone volume"), self._volume_scale, self._volume_value
        )
        volume.add(self._volume_row)
        self._automatic = Adw.ActionRow(
            title=_("Automatic volume"),
            subtitle=_("Follows your voice. Open to change its limits."),
            visible=False,
        )
        attach_chevron(self._automatic)
        self._automatic.connect(
            "activated",
            weak_callback(self, lambda self, _: self.ctx.navigate("voice_volume")),
        )
        volume.add(self._automatic)
        body.append(volume)

        self._primary = action_button(
            _("Test microphone"), "audio-input-microphone-symbolic", primary=True
        )
        self._primary.connect(
            "clicked", weak_callback(self, type(self)._primary_clicked)
        )
        body.append(adaptive_button_row(self._primary))

        return scrolled(body, max_width=820)

    def _noise_toggled(self, row: Adw.SwitchRow, _param) -> None:
        if self._loading:
            return
        self.ctx.state.set("denoiser", "on" if row.get_active() else "off")

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

    def _mode_changed(self, row, _param) -> None:
        if not self._loading:
            self.ctx.state.set("model", MODELS[_MODEL_ORDER[row.get_selected()]][0])

    def _volume_changed(self, scale) -> None:
        value = round(scale.get_value())
        self._volume_value.set_text(f"{value}%")
        if self._loading or self._snapshot is None:
            return
        source = self._snapshot.current_source
        self._volume_debounce.call(lambda: self.ctx.state.set_volume(source, value))

    def _primary_clicked(self, _button) -> None:
        if self._primary.get_sensitive():
            self.ctx.navigate("mic_test")

    def on_hidden(self) -> None:
        self._volume_debounce.cancel()
        self._meter.clear()

    def shutdown(self) -> None:
        self._volume_debounce.cancel()
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
            self._noise.set_sensitive(has_source and snapshot.loaded)
            self._source.set_sensitive(bool(snapshot.loaded))
            self._primary.set_sensitive(snapshot.ready and not snapshot.muted)
            self._adjust_again.set_sensitive(snapshot.ready and not snapshot.muted)
            self._mode.set_sensitive(snapshot.loaded and has_source)
            automatic = calibration_window(snapshot.calibration()) is not None
            self._volume_row.set_visible(not automatic)
            self._automatic.set_visible(automatic)
            if automatic:
                self._volume_debounce.cancel()
            if (
                previous is not None
                and previous.current_source != snapshot.current_source
            ):
                self._volume_debounce.cancel()
            self._volume_scale.set_sensitive(
                snapshot.loaded and has_source and snapshot.volume is not None
            )
            if snapshot.volume is not None and not self._volume_debounce.pending:
                self._volume_scale.set_value(snapshot.volume)
                self._volume_value.set_text(f"{snapshot.volume}%")
            if not self.ctx.state.is_pending("model"):
                self._mode.set_selected(_MODEL_ORDER.index(snapshot.chosen_model))

            if snapshot.loaded:
                updating = self.ctx.state.is_pending(
                    "denoiser"
                ) or self.ctx.state.is_pending("__repair__")
                if not self.ctx.state.is_pending("denoiser"):
                    self._noise.set_active(snapshot.filter_on)
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
                    self._primary.set_sensitive(False)
                    self._adjust_again.set_sensitive(False)
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
                self._noise.set_subtitle(status)
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
                if snapshot.current_source:
                    description = next(
                        (
                            s.get("description", s.get("name", ""))
                            for s in snapshot.sources
                            if s.get("name") == snapshot.current_source
                        ),
                        snapshot.current_source,
                    )
                    self._source.set_subtitle(description)
                else:
                    self._source.set_subtitle(_("No microphone available"))

                calibrated = bool(snapshot.calibration().get("volume"))
                self._volume.set_subtitle(
                    _("Adjusted for your voice")
                    if calibrated
                    else _("Not adjusted yet — measure once for this microphone")
                )
            else:
                self._noise.set_subtitle(_("Loading…"))
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
