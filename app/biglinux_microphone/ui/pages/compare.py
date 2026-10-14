"""Test the microphone: one private recording, before and after noise reduction.

When the running model can preview a recording, the sample is taken from the
device and offered with and without noise reduction. Otherwise (DPDFNet, or
noise reduction off) it is the plain sample of what applications hear.
"""

from __future__ import annotations

from gi.repository import Gtk

from biglinux_microphone import cli
from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.microphone_test import MicrophoneTestPage
from biglinux_microphone.utils.i18n import _


class ComparePage(MicrophoneTestPage):
    def build(self) -> Gtk.Widget:
        self._comparison: cli.SampleComparisonSession | None = None
        self._comparing = False
        return super().build()

    def _build_result(self) -> Gtk.Widget:
        widget = super()._build_result()
        self._choices = Gtk.Box(spacing=18, halign=Gtk.Align.CENTER, visible=False)
        self._original = Gtk.CheckButton(label=_("Original"))
        self._reduced = Gtk.CheckButton(
            label=_("With noise reduction"), group=self._original, active=True
        )
        for button in (self._original, self._reduced):
            button.connect("toggled", weak_callback(self, type(self)._select_version))
            self._choices.append(button)
        self._result_body.prepend(self._choices)
        return widget

    def _show_kind(self, comparing: bool) -> None:
        """Describe the sample the next recording will make."""
        self._comparing = comparing
        if comparing:
            self._intro_page.set_title(_("Compare the same recording"))
            self._intro_page.set_description(
                _(
                    "Speak normally for five seconds. Then listen to the same words with and without noise reduction."
                )
            )
            self._capturing_description.set_text(
                _("Recording your microphone, then preparing both versions…")
            )
            self._result_page.set_description(
                _(
                    "Choose a version and play it. Playback volumes are balanced for comparison."
                )
            )
        else:
            self._intro_page.set_title(_("Record a short sample"))
            self._intro_page.set_description(
                _(
                    "Speak normally for five seconds. Then you can hear exactly what applications receive."
                )
            )
            self._capturing_description.set_text(
                _("Recording what applications hear from your microphone.")
            )
            self._result_page.set_description(
                _(
                    "Listen to the short recording and check whether your voice sounds clear and natural."
                )
            )

    def sync(self, snapshot) -> None:
        super().sync(snapshot)
        if not snapshot.loaded:
            return
        # A sample being recorded or played keeps the kind it was made as.
        if self._recording is None and self._sample_path is None:
            self._show_kind(bool(snapshot.runtime.get("comparison_filter")))
        if self._comparing:
            # The comparison records the device and filters the copy itself.
            self._target = snapshot.current_source
            self._intro_detail.set_text(
                _(
                    "This comparison includes noise reduction only. Equalizer, echo cancellation and other voice effects are not included."
                )
            )

    def _new_recording(
        self,
    ) -> cli.MicrophoneSampleSession | cli.SampleComparisonSession:
        if not self._comparing:
            return super()._new_recording()
        return cli.SampleComparisonSession(
            self._target, self._snapshot.runtime["comparison_filter"]
        )

    def _recorded(self, path, session) -> None:
        self._choices.set_visible(self._comparing)
        if not self._comparing:
            super()._recorded(path, session)
            return
        # A comparison owns two files; only the session can remove both.
        if self._closed or self._recording is not session:
            session.cleanup()
            return
        self._comparison = session
        super()._recorded(path, session)
        self._select_version(
            self._original if self._original.get_active() else self._reduced
        )

    def _select_version(self, button: Gtk.CheckButton) -> None:
        if self._closed or not button.get_active() or self._comparison is None:
            return
        self._sample_path = (
            self._comparison.original_path
            if self._original.get_active()
            else self._comparison.path
        )
        self._set_play_button(False)

    def _set_play_button(self, playing: bool) -> None:
        super()._set_play_button(playing)
        self._original.set_sensitive(not playing)
        self._reduced.set_sensitive(not playing)
        if not playing and self._comparison is not None:
            label = (
                _("Play original")
                if self._original.get_active()
                else _("Play with noise reduction")
            )
            self._play.update_property([Gtk.AccessibleProperty.LABEL], [label])
            self._play.get_child().get_last_child().set_text(label)

    def _cleanup_sample(self) -> None:
        comparison, self._comparison = self._comparison, None
        if comparison is None:
            super()._cleanup_sample()
            return
        self._sample_path = None
        comparison.cleanup()
