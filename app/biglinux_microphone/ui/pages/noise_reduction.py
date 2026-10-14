"""Advanced noise-reduction page.

The person picks one of the three models directly. DFN3-only residual controls
are revealed from the *effective* model instead of from what a radio row
optimistically assumes happened.
"""

from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gtk

from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.base import Page, column, scrolled, slider_row
from biglinux_microphone.ui.state import MODELS
from biglinux_microphone.ui.widgets.threshold_editor import ThresholdEditor
from biglinux_microphone.utils.async_utils import Debouncer
from biglinux_microphone.utils.i18n import _

# CPU per minute of speech with the voice gate on, measured on a 2023 desktop and a
# 2011 laptop (speech-enhancement-native README): DFN3-LL 2.2-2.5x DFN3, DPDFNet 3.9-4.3x.
_MODEL_CHOICES = (
    (11, _("DFN3 — recommended"), _("The lightest. Works well on older computers.")),
    (
        12,
        MODELS[12][1],
        _(
            "A larger network that leaves less noise between words. Uses about twice "
            "the processing of DFN3, and has the shortest delay while “Silence when "
            "nobody speaks” is off."
        ),
    ),
    (
        13,
        MODELS[13][1],
        _(
            "The strongest cleanup, with a steady quiet background. Uses about four "
            "times the processing of DFN3."
        ),
    ),
)

#: Command-line tunings of DFN3 and the values that restore the recommended ones.
_RESTORE_TUNING = (
    ("dfn-snr-gate", "min_snr_override_db", None, "auto"),
    ("dfn-post-filter", "post_filter_beta", 0, "0"),
    ("dfn-startup-mute", "startup_mute_ms", 1000, "1000"),
)


class NoiseReductionPage(Page):
    title = _("Noise reduction")
    id = "noise_reduction"
    icon = "audio-x-generic-symbolic"

    def build(self) -> Gtk.Widget:
        self._loading = True
        self._curve_debounce = Debouncer(180)
        body = column()

        self._master = Adw.SwitchRow(
            title=_("Noise reduction"),
            subtitle=_("Applications hear your voice with less background noise."),
        )
        self._master.add_prefix(
            Gtk.Image.new_from_icon_name("audio-x-generic-symbolic")
        )
        self._master.connect(
            "notify::active", weak_callback(self, type(self)._on_master)
        )
        master_group = Adw.PreferencesGroup()
        master_group.add(self._master)
        body.append(master_group)

        models = Adw.PreferencesGroup(
            title=_("Noise-reduction model"),
            description=_("Heavier models clean more and use more processing."),
        )
        self._model_buttons: dict[int, Gtk.CheckButton] = {}
        first: Gtk.CheckButton | None = None
        for model, title, description in _MODEL_CHOICES:
            row = Adw.ActionRow(title=title, subtitle=description)
            button = Gtk.CheckButton(valign=Gtk.Align.CENTER)
            if first is None:
                first = button
            else:
                button.set_group(first)
            button.update_property([Gtk.AccessibleProperty.LABEL], [title])
            button.connect("toggled", weak_callback(self, type(self)._on_model), model)
            row.add_prefix(button)
            row.set_activatable_widget(button)
            self._model_buttons[model] = button
            models.add(row)
        body.append(models)

        self._effective = Gtk.Label(
            label="",
            xalign=0,
            wrap=True,
            visible=False,
            css_classes=["caption", "dim-label"],
        )
        body.append(self._effective)

        gate = Adw.PreferencesGroup()
        self._voice_gate = Adw.SwitchRow(
            title=_("Silence when nobody speaks"),
            subtitle=_(
                "Mutes leftover noise while you are not talking. Your voice then "
                "arrives about 0.07 s later."
            ),
        )
        self._voice_gate.connect(
            "notify::active", weak_callback(self, type(self)._on_voice_gate)
        )
        gate.add(self._voice_gate)
        # A dropdown suffix rather than an Adw.ComboRow: the combo row ellipsizes
        # the chosen level next to a help line that wraps.
        sensitivity_row = Adw.ActionRow(
            title=_("Voice detection"),
            subtitle=_(
                "Sensitive keeps quiet voices and lets more noise through; strict "
                "mutes more noise and may cut faint words."
            ),
            subtitle_lines=0,
        )
        self._voice_sensitivity = Gtk.DropDown(
            model=Gtk.StringList.new(
                [
                    _("Very sensitive"),
                    _("Sensitive"),
                    _("Normal"),
                    _("Strict"),
                    _("Very strict"),
                ]
            ),
            valign=Gtk.Align.CENTER,
        )
        self._voice_sensitivity.update_property(
            [Gtk.AccessibleProperty.LABEL], [sensitivity_row.get_title()]
        )
        self._voice_sensitivity.connect(
            "notify::selected", weak_callback(self, type(self)._on_voice_sensitivity)
        )
        sensitivity_row.add_suffix(self._voice_sensitivity)
        sensitivity_row.set_activatable_widget(self._voice_sensitivity)
        self._sensitivity_row = sensitivity_row
        gate.add(sensitivity_row)
        body.append(gate)

        tuning = Adw.PreferencesGroup(title=_("Intensity"))
        self._strength = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 100, 1)
        self._strength_value = Gtk.Label(label="100%")
        self._strength.connect(
            "value-changed", weak_callback(self, type(self)._on_strength)
        )
        tuning.add(
            slider_row(
                _("How much noise to remove"),
                self._strength,
                self._strength_value,
                subtitle=_("Lower values preserve more of the original sound."),
            )
        )
        self._tuning_group = tuning
        body.append(tuning)

        self._dpdf_info = Adw.Banner(
            title=_("The current model already handles residual noise internally."),
            revealed=False,
        )
        body.append(self._dpdf_info)

        self._residual = Adw.PreferencesGroup(
            title=_("Noise that still gets through"),
            description=_("DFN3 can silence very quiet residual sounds between words."),
        )
        self._residual_switch = Adw.SwitchRow(
            title=_("Silence residual noise by frequency"),
            subtitle=_("Sounds below the line will be silenced."),
        )
        self._residual_switch.connect(
            "notify::active", weak_callback(self, type(self)._on_residual_toggle)
        )
        self._residual.add(self._residual_switch)

        self._editor_reveal = Gtk.Revealer(
            transition_type=Gtk.RevealerTransitionType.SLIDE_DOWN
        )
        self._editor = None
        self._editor_box = None
        self._residual.add(self._editor_reveal)
        body.append(self._residual)

        self._manual = Adw.PreferencesGroup(visible=False)
        manual_row = Adw.ActionRow(
            title=_("Manual adjustments active"),
            subtitle=_(
                "Values set on the command line replace the recommended ones for DFN3."
            ),
        )
        restore = Gtk.Button(label=_("Restore"), valign=Gtk.Align.CENTER)
        restore.connect("clicked", weak_callback(self, type(self)._restore_tuning))
        manual_row.add_suffix(restore)
        self._manual.add(manual_row)
        body.append(self._manual)
        self._snapshot = None
        return scrolled(body, max_width=900)

    def _ensure_editor(self) -> None:
        if self._editor is not None:
            return
        editor_box = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=8,
            css_classes=["card", "threshold-card"],
        )
        editor_box.append(
            Gtk.Label(
                label=_(
                    "Move the line above residual noise, but keep it below the quiet parts of your voice."
                ),
                xalign=0.0,
                wrap=True,
                css_classes=["dim-label"],
            )
        )
        self._editor = ThresholdEditor(weak_callback(self, type(self)._curve_changed))
        editor_box.append(self._editor)
        actions = Gtk.Box(spacing=8)
        reset = Gtk.Button(label=_("Restore recommended"), css_classes=["pill"])
        reset.connect(
            "clicked", weak_callback(self, lambda self, _b: self._reset_curve())
        )
        actions.append(reset)
        editor_box.append(actions)
        self._editor_box = editor_box

    def _show_editor(self, show: bool) -> None:
        if show:
            self._ensure_editor()
            self._editor_reveal.set_child(self._editor_box)
        self._editor_reveal.set_reveal_child(show)
        if not show:
            self._editor_reveal.set_child(None)
        if not self._loading:
            self.ctx.shell._sync_capture()

    def _on_master(self, row: Adw.SwitchRow, _param) -> None:
        if self._loading:
            return
        self.ctx.state.set("denoiser", "on" if row.get_active() else "off")

    def _on_model(self, button: Gtk.CheckButton, model: int) -> None:
        if self._loading or not button.get_active():
            return
        self.ctx.state.set("model", MODELS[model][0])

    def _on_voice_gate(self, row: Adw.SwitchRow, _param) -> None:
        if self._loading:
            return
        self.ctx.state.set("voice-gate", "on" if row.get_active() else "off")

    def _on_voice_sensitivity(self, dropdown: Gtk.DropDown, _param) -> None:
        if self._loading:
            return
        self.ctx.state.set("voice-sensitivity", str(dropdown.get_selected() + 1))

    def _restore_tuning(self, _button: Gtk.Button) -> None:
        nr = self._snapshot.noise_reduction if self._snapshot else {}
        for key, field, recommended, value in _RESTORE_TUNING:
            if nr.get(field, recommended) != recommended:
                self.ctx.state.set(key, value)

    def _on_strength(self, scale: Gtk.Scale) -> None:
        value = int(scale.get_value())
        self._strength_value.set_text(f"{value}%")
        if self._loading:
            return
        self.ctx.state.set_debounced("mic-intensity", str(value), 250)

    def _on_residual_toggle(self, row: Adw.SwitchRow, _param) -> None:
        if self._loading:
            return
        active = row.get_active()
        self._curve_debounce.cancel()
        self.ctx.state.cancel_pending("silence-curve")
        self._show_editor(active)
        if active:
            snapshot = self._snapshot
            curve = (
                snapshot.noise_reduction.get("silence_curve_db") if snapshot else None
            )
            if not isinstance(curve, list) or len(curve) != 10:
                # Conservative starting point: effectively off until the user
                # raises an anchor.  Enabling the editor must never mute a voice.
                curve = [-60.0] * 10
            self._editor.set_curve(curve)
            self._push_curve(curve)
        else:
            self._curve_debounce.cancel()
            self.ctx.state.set("silence-curve", "off")

    def _curve_changed(self, curve: list[float]) -> None:
        if self._loading:
            return
        self._curve_debounce.call(lambda: self._push_curve(curve))

    def _push_curve(self, curve: list[float]) -> None:
        csv = ",".join(f"{float(value):.1f}" for value in curve[:10])
        self.ctx.state.set("silence-curve", csv)

    def _reset_curve(self) -> None:
        self._curve_debounce.cancel()
        self.ctx.state.cancel_pending("silence-curve")
        self._ensure_editor()
        curve = [-60.0] * 10
        self._editor.set_curve(curve)
        self._push_curve(curve)

    def sync(self, snapshot) -> None:
        self._snapshot = snapshot
        if not snapshot.loaded:
            return
        self._loading = True
        try:
            if not self.ctx.state.is_pending("denoiser"):
                self._master.set_active(snapshot.filter_on)
            self._tuning_group.set_sensitive(snapshot.filter_on)
            nr = snapshot.noise_reduction
            strength = max(0, min(100, round(float(nr.get("strength", 1.0)) * 100)))
            if not self.ctx.state.is_pending("mic-intensity"):
                self._strength.set_value(strength)
            self._strength_value.set_text(f"{int(self._strength.get_value())}%")

            chosen = snapshot.chosen_model
            if not self.ctx.state.is_pending("model"):
                self._model_buttons[chosen].set_active(True)

            # Only worth a line while the running model is not yet the chosen one.
            label = snapshot.model_label
            self._effective.set_visible(bool(label) and snapshot.model != chosen)
            if label:
                self._effective.set_text(_("Model in use: %s") % label)

            self._voice_gate.set_sensitive(snapshot.filter_on)
            if not self.ctx.state.is_pending("voice-gate"):
                self._voice_gate.set_active(snapshot.voice_gate)
            # The level tunes the gate's detector, so it acts only while the gate runs.
            self._sensitivity_row.set_sensitive(
                snapshot.filter_on and snapshot.voice_gate
            )
            if not self.ctx.state.is_pending("voice-sensitivity"):
                self._voice_sensitivity.set_selected(snapshot.voice_sensitivity - 1)
            self._manual.set_visible(
                MODELS[chosen][0] != "dpdfnet" and snapshot.manual_tuning
            )

            dfn = snapshot.model_family == "dfn3"
            dpdf = snapshot.model_family == "dpdfnet"
            self._residual.set_visible(dfn)
            self._dpdf_info.set_revealed(dpdf)
            curve = nr.get("silence_curve_db")
            active_curve = isinstance(curve, list) and len(curve) == 10
            self._residual_switch.set_active(active_curve)
            self._show_editor(dfn and active_curve)
            if (
                dfn
                and active_curve
                and not self._curve_debounce.pending
                and not self.ctx.state.is_pending("silence-curve")
            ):
                self._editor.set_curve(curve)
        finally:
            self._loading = False

    def on_bands(self, bands: list[float]) -> None:
        if self._editor is not None and self._editor_reveal.get_reveal_child():
            self._editor.set_bands(bands)

    def shutdown(self) -> None:
        self._curve_debounce.cancel()
