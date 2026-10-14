"""Short record/playback microphone test.

Records five seconds from the source applications currently hear, then lets the
person play that exact sample back. `ComparePage` builds the sidebar's test on
it; continuous delayed monitoring is the sidebar's Listen back switch.
"""

from __future__ import annotations

import contextlib
import logging
import threading
from pathlib import Path

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


class MicrophoneTestPage(Page):
    title = _("Test microphone")
    id = "mic_test"
    icon = "audio-input-microphone-symbolic"

    def build(self) -> Gtk.Widget:
        self._snapshot = None
        self._target = ""
        self._recording: (
            cli.MicrophoneSampleSession | cli.SampleComparisonSession | None
        ) = None
        self._playback: cli.SamplePlaybackSession | None = None
        self._sample_path: Path | None = None
        self._tick_id = 0
        self._elapsed = 0
        self._cancelled = False
        self._closed = False
        self._cleanup_threads: list[threading.Thread] = []

        self._stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE)
        self._stack.add_named(self._build_intro(), "intro")
        self._stack.add_named(self._build_capturing(), "capturing")
        self._stack.add_named(self._build_result(), "result")
        self._stack.add_named(self._build_error(), "error")
        self._stack.set_visible_child_name("intro")
        return self._stack

    def _frame(self, child: Gtk.Widget) -> Gtk.Widget:
        return scrolled(child, max_width=700)

    def _build_intro(self) -> Gtk.Widget:
        body = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=18,
            margin_top=0,
            margin_bottom=28,
            margin_start=24,
            margin_end=24,
        )
        page = Adw.StatusPage(
            css_classes=["compact"],
            icon_name="audio-input-microphone-symbolic",
            title=_("Record a short sample"),
            description=_(
                "Speak normally for five seconds. Then you can hear exactly what applications receive."
            ),
        )
        self._intro_page = page
        body.append(
            Adw.Banner(
                title=_("Use headphones before playing the sample to avoid echo."),
                revealed=True,
            )
        )
        self._intro_detail = Gtk.Label(
            label="",
            xalign=0.0,
            wrap=True,
            css_classes=["dim-label"],
        )
        body.append(self._intro_detail)
        self._record_button = action_button(
            _("Record sample"), "media-record-symbolic", primary=True
        )
        self._record_button.connect(
            "clicked", weak_callback(self, lambda self, _b: self._start_recording())
        )
        body.append(self._record_button)
        page.set_child(Adw.Clamp(maximum_size=700, child=body))
        return page

    def _build_capturing(self) -> Gtk.Widget:
        body = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=16,
            margin_top=48,
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
        self._capturing_description = Gtk.Label(
            label=_("Recording what applications hear from your microphone."),
            xalign=0.5,
            wrap=True,
            justify=Gtk.Justification.CENTER,
            css_classes=["dim-label"],
        )
        body.append(self._capturing_description)
        icon = Gtk.Image.new_from_icon_name("media-record-symbolic")
        icon.set_pixel_size(64)
        icon.add_css_class("accent")
        body.append(icon)
        self._progress = Gtk.ProgressBar(hexpand=True, css_classes=["pass-progress"])
        body.append(self._progress)
        self._time = Gtk.Label(
            label=_("%(elapsed)d of %(total)d seconds")
            % {"elapsed": 0, "total": cli.MIC_TEST_SECONDS},
            css_classes=["dim-label", "numeric"],
        )
        body.append(self._time)
        cancel = action_button(_("Cancel"))
        cancel.connect(
            "clicked", weak_callback(self, lambda self, _b: self._cancel_recording())
        )
        cancel.set_halign(Gtk.Align.CENTER)
        body.append(cancel)
        return self._frame(body)

    def _build_result(self) -> Gtk.Widget:
        body = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=18,
            margin_top=0,
            margin_bottom=28,
            margin_start=24,
            margin_end=24,
        )
        self._result_body = body
        page = Adw.StatusPage(
            css_classes=["compact"],
            icon_name="audio-headphones-symbolic",
            title=_("Your sample is ready"),
            description=_(
                "Listen to the short recording and check whether your voice sounds clear and natural."
            ),
        )
        self._result_page = page
        self._play = action_button(
            _("Play sample"), "media-playback-start-symbolic", primary=True
        )
        self._play.connect(
            "clicked", weak_callback(self, lambda self, _b: self._toggle_playback())
        )
        body.append(self._play)
        again = action_button(_("Record again"), "view-refresh-symbolic")
        again.connect("clicked", weak_callback(self, lambda self, _b: self._restart()))
        done = action_button(_("Done"), "object-select-symbolic")
        done.connect(
            "clicked", weak_callback(self, lambda self, _b: self.ctx.navigate("home"))
        )
        body.append(adaptive_button_row(again, done))
        page.set_child(Adw.Clamp(maximum_size=700, child=body))
        return page

    def _build_error(self) -> Gtk.Widget:
        body = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=14,
            margin_top=0,
            margin_bottom=28,
            margin_start=24,
            margin_end=24,
        )
        self._error_page = Adw.StatusPage(
            css_classes=["compact"],
            icon_name="dialog-warning-symbolic",
            title=_("I could not record the microphone"),
            description=_("Check that the microphone is available and try again."),
        )
        retry = action_button(_("Try again"), "view-refresh-symbolic", primary=True)
        retry.connect("clicked", weak_callback(self, lambda self, _b: self._restart()))
        body.append(retry)
        self._error_page.set_child(Adw.Clamp(maximum_size=700, child=body))
        return self._error_page

    def sync(self, snapshot) -> None:
        previous = self._snapshot
        self._snapshot = snapshot
        if (
            self._recording is not None
            and previous is not None
            and (
                snapshot.capture_identity != previous.capture_identity
                or not snapshot.ready
            )
        ):
            self._stop_all(cleanup=True)
            self._show_error(
                _("The microphone changed or became unavailable. Record a new sample.")
            )
        if not snapshot.loaded:
            self._record_button.set_sensitive(False)
            return
        self._target = snapshot.capture_target
        available = any(
            s.get("name") == snapshot.current_source for s in snapshot.sources
        )
        self._record_button.set_sensitive(
            bool(
                snapshot.ready
                and available
                and not snapshot.muted
                and not self._cleaning()
            )
        )
        if not self._target:
            self._intro_detail.set_text(
                _("The microphone is not ready. Return to Home to check its status.")
            )
        elif self._target == "mic-biglinux":
            self._intro_detail.set_text(
                _("The sample includes the current noise reduction and voice settings.")
            )
        else:
            self._intro_detail.set_text(
                _("Noise reduction is off, so the sample uses the microphone directly.")
            )

    def on_shown(self) -> None:
        if self._sample_path is None and self._recording is None:
            self._stack.set_visible_child_name("intro")

    def on_hidden(self) -> None:
        self._stop_all(cleanup=True)
        self._stack.set_visible_child_name("intro")

    def _start_recording(self) -> None:
        if self._closed or self._cleaning():
            return
        if (
            not self._target
            or not self._record_button.get_sensitive()
            or self._recording is not None
        ):
            if not self._target:
                self.ctx.toast(_("No microphone is selected"))
            return
        self._stop_playback()
        self._cleanup_sample()
        self._cancelled = False
        self._elapsed = 0
        self._progress.set_fraction(0.0)
        self._time.set_text(
            _("%(elapsed)d of %(total)d seconds")
            % {"elapsed": 0, "total": cli.MIC_TEST_SECONDS}
        )
        try:
            self._recording = self._new_recording()
        except (cli.BackendError, OSError) as exc:
            self._show_error(str(exc))
            return
        self._stack.set_visible_child_name("capturing")
        self._tick_id = GLib.timeout_add_seconds(1, self._tick)
        session = self._recording
        self.ctx.state.call(
            session.wait,
            lambda path: self._recorded(path, session),
            lambda message: self._record_failed(message, session),
        )

    def _new_recording(
        self,
    ) -> cli.MicrophoneSampleSession | cli.SampleComparisonSession:
        return cli.start_microphone_sample(self._target, cli.MIC_TEST_SECONDS)

    def _tick(self) -> bool:
        if self._recording is None:
            self._tick_id = 0
            return GLib.SOURCE_REMOVE
        self._elapsed = min(cli.MIC_TEST_SECONDS, self._elapsed + 1)
        self._progress.set_fraction(min(0.98, self._elapsed / cli.MIC_TEST_SECONDS))
        self._time.set_text(
            _("%(elapsed)d of %(total)d seconds")
            % {"elapsed": self._elapsed, "total": cli.MIC_TEST_SECONDS}
        )
        return GLib.SOURCE_CONTINUE

    def _recorded(self, path: Path, session) -> None:
        if self._closed or self._recording is not session:
            with contextlib.suppress(OSError):
                Path(path).unlink()
            return
        self._recording = None
        self._stop_tick()
        self._progress.set_fraction(1.0)
        self._sample_path = Path(path)
        self._stack.set_visible_child_name("result")

    def _record_failed(self, message: str, session) -> None:
        if self._closed or self._recording is not session:
            return
        self._recording = None
        self._stop_tick()
        self._show_error(str(message))

    def _cancel_recording(self) -> None:
        session = self._recording
        if session is None:
            self._stack.set_visible_child_name("intro")
            return
        self._cancelled = True
        self._recording = None
        self._stop_tick()
        self._retire(session, remove_sample=True)
        self._stack.set_visible_child_name("intro")

    def _toggle_playback(self) -> None:
        if self._closed or self._cleaning():
            return
        if self._playback is not None:
            self._stop_playback()
            return
        path = self._sample_path
        if path is None:
            return
        try:
            self._playback = cli.start_sample_playback(path)
        except (cli.BackendError, OSError) as exc:
            self._show_error(str(exc))
            return
        self._set_play_button(True)
        session = self._playback
        self.ctx.state.call(
            session.wait,
            lambda result: self._playback_done(result, session),
            lambda message: self._playback_failed(message, session),
        )

    def _playback_done(self, _result, session) -> None:
        if self._closed or self._playback is not session:
            return
        self._playback = None
        self._set_play_button(False)

    def _playback_failed(self, message: str, session) -> None:
        if self._closed or self._playback is not session:
            return
        self._playback = None
        self._set_play_button(False)
        self.ctx.toast(str(message)[:200])

    def _set_play_button(self, playing: bool) -> None:
        title = _("Stop playback") if playing else _("Play sample")
        self._play.update_property([Gtk.AccessibleProperty.LABEL], [title])
        child = self._play.get_child()
        if not isinstance(child, Gtk.Box):
            return
        image = child.get_first_child()
        label = image.get_next_sibling() if image is not None else None
        if isinstance(image, Gtk.Image):
            image.set_from_icon_name(
                "media-playback-stop-symbolic"
                if playing
                else "media-playback-start-symbolic"
            )
        if isinstance(label, Gtk.Label):
            label.set_text(title)

    def _stop_playback(self) -> None:
        playback, self._playback = self._playback, None
        if playback is not None:
            self._retire(playback)
        self._set_play_button(False)

    def _restart(self) -> None:
        self._stop_playback()
        self._cleanup_sample()
        self._stack.set_visible_child_name("intro")

    def _show_error(self, message: str) -> None:
        self._error_page.set_description(
            GLib.markup_escape_text(str(message)[:220])
            or _("Check that the microphone is available and try again.")
        )
        self._stack.set_visible_child_name("error")

    def _cleanup_sample(self) -> None:
        path, self._sample_path = self._sample_path, None
        if path is not None:
            with contextlib.suppress(OSError):
                path.unlink()

    def _stop_tick(self) -> None:
        if self._tick_id:
            GLib.source_remove(self._tick_id)
            self._tick_id = 0

    def _stop_all(self, *, cleanup: bool) -> None:
        recording, self._recording = self._recording, None
        if recording is not None:
            self._cancelled = True
            self._retire(recording, remove_sample=True)
        self._stop_playback()
        self._stop_tick()
        if cleanup:
            self._cleanup_sample()

    def _cleaning(self) -> bool:
        self._cleanup_threads = [t for t in self._cleanup_threads if t.is_alive()]
        return bool(self._cleanup_threads)

    def _retire(self, session, *, remove_sample: bool = False) -> None:
        """Reap bounded child processes without blocking GTK or losing cleanup.

        Non-daemon threads finish private-file cleanup even if the window and
        main loop close. Identity-checked callbacks cannot alter newer sessions.
        """
        self._record_button.set_sensitive(False)

        def work() -> None:
            try:
                session.cancel()
            except Exception:
                logger.exception("Cannot stop microphone sample process")
            finally:
                try:
                    if remove_sample:
                        session.cleanup()
                except Exception:
                    logger.exception("Cannot remove microphone test sample")
                GLib.timeout_add(25, refresh)

        def refresh() -> bool:
            if self._closed:
                return GLib.SOURCE_REMOVE
            if self._cleaning():
                return GLib.SOURCE_CONTINUE
            if self._snapshot is not None:
                self.sync(self._snapshot)
            return GLib.SOURCE_REMOVE

        thread = threading.Thread(
            target=work, name="bigmic-sample-cleanup", daemon=False
        )
        self._cleanup_threads.append(thread)
        thread.start()

    def shutdown(self) -> None:
        self._closed = True
        self._stop_all(cleanup=True)
