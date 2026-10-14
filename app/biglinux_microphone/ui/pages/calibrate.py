"""Focused, single-pass microphone volume calibration."""

from __future__ import annotations

import logging
import threading

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, GLib, Gtk

from biglinux_microphone import cli
from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.base import Page, scrolled
from biglinux_microphone.ui.pages.common import action_button, adaptive_button_row
from biglinux_microphone.utils.i18n import _

logger = logging.getLogger(__name__)

PASS_SECONDS = 8


class CalibratePage(Page):
    id = "calibrate"
    icon = "audio-volume-high-symbolic"
    in_sidebar = False

    def build(self) -> Gtk.Widget:
        self.title = _("Adjust volume")
        self._running = False
        self._closed = False
        self._cancel_thread: threading.Thread | None = None
        self._cancelled = False
        self._session = None
        self._tick_id = 0
        self._elapsed = 0
        self._source = ""
        self._original_volume: int | None = None
        self._snapshot = None

        self._stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE)
        self._stack.add_named(self._build_prepare(), "prepare")
        self._stack.add_named(self._build_listening(), "listening")
        self._stack.add_named(self._build_result(), "result")
        self._stack.add_named(self._build_error(), "error")
        self._stack.set_visible_child_name("prepare")
        return self._stack

    def _frame(self, child: Gtk.Widget) -> Gtk.Widget:
        return scrolled(child, max_width=760)

    def _build_prepare(self) -> Gtk.Widget:
        body = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=20,
            margin_top=36,
            margin_bottom=28,
            margin_start=22,
            margin_end=22,
        )
        body.append(
            Gtk.Label(
                label=_("Adjust the volume for your voice"),
                xalign=0.0,
                wrap=True,
                css_classes=["title-1"],
            )
        )
        body.append(
            Gtk.Label(
                label=_(
                    "Speak normally for a few seconds. The program will set the volume for this microphone."
                ),
                xalign=0.0,
                wrap=True,
                css_classes=["dim-label"],
            )
        )
        group = Adw.PreferencesGroup(title=_("Microphone"))
        self._prepare_mic = Adw.ActionRow(
            title=_("Checking…"), subtitle="", use_markup=False
        )
        self._prepare_mic.add_prefix(
            Gtk.Image.new_from_icon_name("audio-input-microphone-symbolic")
        )
        group.add(self._prepare_mic)
        body.append(group)
        tip = Adw.Banner(
            title=_(
                "Speak at the volume and distance you normally use in calls and recordings."
            ),
            revealed=True,
        )
        body.append(tip)
        start = action_button(_("Start"), "media-record-symbolic", primary=True)
        start.connect("clicked", weak_callback(self, lambda self, _b: self._start()))
        start.set_halign(Gtk.Align.FILL)
        start.set_sensitive(False)
        self._start_button = start
        body.append(start)
        return self._frame(body)

    def _build_listening(self) -> Gtk.Widget:
        body = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=16,
            margin_top=46,
            margin_bottom=28,
            margin_start=24,
            margin_end=24,
        )
        body.append(
            Gtk.Label(
                label=_("Speak normally"),
                xalign=0.5,
                wrap=True,
                css_classes=["title-1"],
            )
        )
        body.append(
            Gtk.Label(
                label=_(
                    "The program is listening to your voice to set the ideal volume for this microphone."
                ),
                xalign=0.5,
                wrap=True,
                justify=Gtk.Justification.CENTER,
                css_classes=["dim-label"],
            )
        )
        icon = Gtk.Image.new_from_icon_name("audio-input-microphone-symbolic")
        icon.set_pixel_size(72)
        icon.add_css_class("accent")
        body.append(icon)
        self._hearing = Gtk.Label(
            label=_("Listening to your voice"), css_classes=["heading"]
        )
        body.append(self._hearing)
        self._progress = Gtk.ProgressBar(hexpand=True, css_classes=["pass-progress"])
        body.append(self._progress)
        self._time = Gtk.Label(
            label=_("%(elapsed)d of %(total)d seconds")
            % {"elapsed": 0, "total": PASS_SECONDS},
            css_classes=["dim-label", "numeric"],
        )
        body.append(self._time)
        body.append(
            Adw.Banner(
                title=_("Speak at the volume and distance you normally use."),
                revealed=True,
            )
        )
        self._cancel = action_button(_("Cancel"))
        self._cancel.connect(
            "clicked", weak_callback(self, lambda self, _b: self.cancel())
        )
        self._cancel.set_halign(Gtk.Align.CENTER)
        body.append(self._cancel)
        return self._frame(body)

    def _build_result(self) -> Gtk.Widget:
        body = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=20,
            margin_top=42,
            margin_bottom=28,
            margin_start=22,
            margin_end=22,
        )
        success = Gtk.Image.new_from_icon_name("object-select-symbolic")
        success.set_pixel_size(72)
        success.add_css_class("success")
        body.append(success)
        body.append(
            Gtk.Label(
                label=_("Your microphone is adjusted"),
                xalign=0.5,
                wrap=True,
                css_classes=["title-1"],
            )
        )
        body.append(
            Gtk.Label(
                label=_("The volume was set for your voice and this microphone."),
                xalign=0.5,
                wrap=True,
                justify=Gtk.Justification.CENTER,
                css_classes=["dim-label"],
            )
        )
        group = Adw.PreferencesGroup()
        ready = Adw.ActionRow(
            title=_("Ready to use"),
            subtitle=_("You can test your voice or finish now."),
        )
        ready.add_prefix(Gtk.Image.new_from_icon_name("audio-volume-high-symbolic"))
        group.add(ready)
        body.append(group)
        done = action_button(_("Done"), "object-select-symbolic", primary=True)
        done.connect(
            "clicked", weak_callback(self, lambda self, _b: self.ctx.navigate("home"))
        )
        again = action_button(_("Adjust again"), "view-refresh-symbolic")
        again.connect(
            "clicked", weak_callback(self, lambda self, _b: self._show_prepare())
        )
        body.append(adaptive_button_row(done, again))

        self._professional = Adw.PreferencesGroup(
            title=_("For live streams and long recordings"), visible=False
        )
        professional = Adw.ActionRow(
            title=_("Keep voice volume uniform"),
            subtitle=_("Compensates when you move farther from the microphone."),
        )
        professional.add_prefix(
            Gtk.Image.new_from_icon_name("audio-volume-high-symbolic")
        )
        professional.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
        professional.set_activatable(True)
        professional.connect(
            "activated",
            weak_callback(self, lambda self, _r: self.ctx.navigate("voice_volume")),
        )
        self._professional.add(professional)
        body.append(self._professional)
        return self._frame(body)

    def _build_error(self) -> Gtk.Widget:
        page = Adw.StatusPage(
            icon_name="dialog-warning-symbolic",
            title=_("I could not adjust the microphone"),
            description=_(
                "Check that the right microphone is selected and try again while speaking normally."
            ),
        )
        box = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=12,
            margin_top=30,
            margin_bottom=30,
            margin_start=24,
            margin_end=24,
        )
        box.append(page)
        again = action_button(_("Try again"), "view-refresh-symbolic", primary=True)
        again.connect(
            "clicked", weak_callback(self, lambda self, _b: self._show_prepare())
        )
        box.append(again)
        return self._frame(box)

    def sync(self, snapshot) -> None:
        self._snapshot = snapshot
        if self._closed:
            return
        if self._running:
            # These coordinates are rollback state for the active pass, not
            # live telemetry. A hotplug/default change must never replace them.
            if snapshot.loaded and snapshot.current_source != self._source:
                self.cancel(navigate=False)
            return
        available = snapshot.loaded and any(
            s.get("name") == snapshot.current_source for s in snapshot.sources
        )
        self._start_button.set_sensitive(available)
        if snapshot.loaded:
            self._source = snapshot.current_source
            self._original_volume = snapshot.volume
            description = next(
                (
                    s.get("description", s.get("name", ""))
                    for s in snapshot.sources
                    if s.get("name") == snapshot.current_source
                ),
                snapshot.current_source or _("No microphone available"),
            )
            self._prepare_mic.set_title(str(description))
            self._prepare_mic.set_subtitle(_("Microphone currently selected"))
            self._professional.set_visible(bool(snapshot.capabilities.get("leveler")))

    def on_shown(self) -> None:
        if not self._running and self._stack.get_visible_child_name() != "result":
            self._show_prepare()

    def on_hidden(self) -> None:
        # A navigation action must not leave the backend pass detached.
        if self._running:
            self.cancel(navigate=False)

    def _show_prepare(self) -> None:
        if self._closed:
            return
        if self._snapshot is not None:
            self.sync(self._snapshot)
        self._stop_tick()
        self._stack.set_visible_child_name("prepare")

    def _start(self) -> None:
        if self._closed or not self._start_button.get_sensitive():
            return
        if self._running or not self._source:
            if not self._source:
                self.ctx.toast(_("No microphone is selected"))
            return
        self._cancelled = False
        self._elapsed = 0
        self._progress.set_fraction(0.0)
        self._time.set_text(
            _("%(elapsed)d of %(total)d seconds")
            % {"elapsed": 0, "total": PASS_SECONDS}
        )
        self._hearing.set_text(_("Listening to your voice"))
        self._cancel.set_sensitive(True)
        try:
            self._session = cli.start_calibration("quick")
        except Exception as exc:
            self.ctx.toast(str(exc)[:200])
            self._stack.set_visible_child_name("error")
            return
        self._running = True
        self._stack.set_visible_child_name("listening")
        self._tick_id = GLib.timeout_add_seconds(1, self._tick)
        session = self._session
        self.ctx.state.call(
            session.wait,
            lambda result: self._finished(result, session),
            lambda message: self._failed(message, session),
        )

    def _tick(self) -> bool:
        if not self._running:
            self._tick_id = 0
            return GLib.SOURCE_REMOVE
        self._elapsed = min(PASS_SECONDS, self._elapsed + 1)
        self._progress.set_fraction(min(0.97, self._elapsed / PASS_SECONDS))
        self._time.set_text(
            _("%(elapsed)d of %(total)d seconds")
            % {"elapsed": self._elapsed, "total": PASS_SECONDS}
        )
        if self._elapsed >= PASS_SECONDS - 1:
            self._hearing.set_text(_("Almost done — keep talking"))
        return GLib.SOURCE_CONTINUE

    def _finished(self, result: dict, session=None) -> None:
        if (
            self._closed
            or self._cancelled
            or (session is not None and session is not self._session)
        ):
            return
        self._running = False
        self._session = None
        self._stop_tick()
        if not result.get("heard_speech"):
            self._stack.set_visible_child_name("error")
            return
        self._progress.set_fraction(1.0)
        self.ctx.state.reload()
        self._stack.set_visible_child_name("result")

    def _failed(self, message: str, session=None) -> None:
        if (
            self._closed
            or self._cancelled
            or (session is not None and session is not self._session)
        ):
            return
        self._running = False
        self._session = None
        self._stop_tick()
        self.ctx.toast(str(message)[:200])
        self._stack.set_visible_child_name("error")

    def cancel(self, *, navigate: bool = True) -> None:
        if self._closed or self._cancelled:
            return
        if not self._running:
            if navigate:
                self.ctx.navigate("home")
            return
        self._cancelled = True
        self._cancel.set_sensitive(False)
        self._hearing.set_text(_("Cancelling…"))
        session = self._session
        source = self._source
        original = self._original_volume

        def stop_and_restore() -> None:
            error = ""
            try:
                if session is not None:
                    session.cancel()
                if source and isinstance(original, int):
                    cli.set_source_volume(source, original)
            except (
                Exception
            ) as exc:  # cleanup failure is important enough to tell the person
                error = str(exc)

            def done() -> bool:
                if self._closed or self._session is not session:
                    return GLib.SOURCE_REMOVE
                self._running = False
                self._session = None
                self._stop_tick()
                if error:
                    self.ctx.toast(
                        _("Could not fully restore the previous microphone level: %s")
                        % error[:120]
                    )
                self.ctx.state.reload()
                if navigate:
                    self.ctx.navigate("home")
                else:
                    self._show_prepare()
                return False

            GLib.idle_add(done)

        self._cancel_thread = threading.Thread(
            target=stop_and_restore, name="bigmic-calibration-cancel", daemon=False
        )
        self._cancel_thread.start()

    def _stop_tick(self) -> None:
        if self._tick_id:
            GLib.source_remove(self._tick_id)
            self._tick_id = 0

    def shutdown(self) -> None:
        self._closed = True
        self._stop_tick()
        worker = getattr(self, "_cancel_thread", None)
        if self._running and self._cancelled and worker is not None:
            # The existing non-daemon worker already owns rollback. Python
            # waits for it on normal exit; joining here would freeze GTK.
            self._running = False
            self._session = None
            return
        if not self._running:
            return
        self._cancelled = True
        session, self._session = self._session, None
        source, original = self._source, self._original_volume
        self._running = False

        def restore_on_exit() -> None:
            try:
                if session is not None:
                    session.cancel()
                if source and isinstance(original, int):
                    cli.set_source_volume(source, original)
            except Exception:
                logger.exception("Failed to restore microphone volume during shutdown")

        # Close/Quit must release the GTK main loop immediately, but a daemon
        # thread could disappear before restoring the hardware level. The
        # process waits for this bounded worker even after its window closes.
        self._cancel_thread = threading.Thread(
            target=restore_on_exit, name="bigmic-calibration-close", daemon=False
        )
        self._cancel_thread.start()
