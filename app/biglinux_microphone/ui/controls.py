"""The sidebar: every setting as a row that shows its value and opens a card.

The rows follow the BigLinux converters: a choice is picked in the row's
card; measuring the voice is a task the shell shows in place of the live
panel. Like a page, the sidebar re-renders from every reading and writes the
backend only through the state.
"""

from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import GLib, Gtk

from biglinux_microphone import cli
from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.state import MODELS, calibration_window
from biglinux_microphone.ui.widgets.setting_cards import (
    ChoiceRow,
    ChoiceTiles,
    SettingCards,
    SettingRow,
    option_tile,
)
from biglinux_microphone.utils.async_utils import Debouncer
from biglinux_microphone.utils.i18n import _

# Off, then the light and the strong model. DFN3-LL stays usable from the command
# line; the window offers these two. CPU per minute of speech with the voice gate
# on (speech-enhancement-native README): DPDFNet 3.9-4.3x DFN3.
_NOISE_MODELS = (None, 11, 13)
# Named by what they do; the model behind each is in its description.
_NOISE_LABELS = (
    _("Off"),
    _("Light filter")
    + " — "
    + _(
        "DFN3: good for most situations, with a clean voice of good quality. "
        "The lightest, it works well on older computers."
    ),
    _("High quality")
    + " — "
    + _(
        "DPDFNet: the strongest cleanup, with a steady quiet background. Uses "
        "about four times the processing of DFN3."
    ),
)
_ECHO_VALUES = ("auto", "on", "off")
_SENSITIVITY_NAMES = (
    _("Very sensitive"),
    _("Sensitive"),
    _("Normal"),
    _("Strict"),
    _("Very strict"),
)
_BANDS = (31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000)
# The profile shown, its backend preset; "custom" edits the bands.
_PRESETS = (
    ("natural", _("Natural"), "flat"),
    ("podcast", _("Podcast"), "podcast"),
    ("custom", _("Custom"), "custom"),
)
#: Command-line tunings of DFN3 and the values that restore the recommended ones.
_RESTORE_TUNING = (
    ("dfn-snr-gate", "min_snr_override_db", None, "auto"),
    ("dfn-post-filter", "post_filter_beta", 0, "0"),
    ("dfn-startup-mute", "startup_mute_ms", 1000, "1000"),
)


def _caption(text: str) -> Gtk.Label:
    return Gtk.Label(
        label=text, xalign=0, wrap=True, max_width_chars=36, css_classes=["caption"]
    )


def _scale_box(title: str, scale: Gtk.Scale, value: Gtk.Label) -> Gtk.Box:
    heading = Gtk.Box(spacing=12)
    heading.append(Gtk.Label(label=title, xalign=0, hexpand=True))
    value.add_css_class("dim-label")
    value.add_css_class("numeric")
    heading.append(value)
    scale.update_property([Gtk.AccessibleProperty.LABEL], [title])
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
    box.append(heading)
    box.append(scale)
    return box


class Controls:
    """The sidebar's groups of setting rows, and the dialogs two of them open."""

    def __init__(self, context, scrolled: Gtk.ScrolledWindow, layer) -> None:
        self.ctx = context
        self._loading = True
        self._snapshot = None
        self._volume_debounce = Debouncer(180)
        self._custom_bands = [0.0] * 10
        self._eq_scales: list[Gtk.Scale] = []
        self._eq_values: list[Gtk.Label] = []
        self._source_signature = None
        self._source_tiles: dict[str, Gtk.CheckButton] = {}
        self._choosing = False
        self.cards = SettingCards(scrolled, layer)
        self.widget = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=18,
            margin_top=12,
            margin_bottom=12,
            margin_start=12,
            margin_end=12,
        )
        self._build_cleaning()
        self._build_devices()
        self._build_voice()
        self._loading = False

    def _group(self) -> Gtk.ListBox:
        box, listbox = self.cards.add_group(None)
        self.widget.append(box)
        return listbox

    def _build_cleaning(self) -> None:
        rows = self._group()
        # Off and the models in one place: picking a model turns the reduction on.
        self.noise = ChoiceRow(
            "weather-fog-symbolic",
            _("Noise reduction"),
            _("Applications hear your voice with less background noise."),
            _NOISE_LABELS,
            changes_result=True,
        )
        self.noise.connect(
            "notify::selected", weak_callback(self, type(self)._on_noise)
        )
        self.cards.add_row(rows, self.noise)

        self._strength = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 100, 1)
        self._strength.set_draw_value(False)
        self._strength_value = Gtk.Label(label="100%")
        self._strength.connect(
            "value-changed", weak_callback(self, type(self)._on_strength)
        )
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        body.append(
            _scale_box(
                _("How much noise to remove"), self._strength, self._strength_value
            )
        )
        self._manual = Gtk.Box(spacing=12, visible=False)
        self._manual.append(
            _caption(
                _(
                    "Values set on the command line replace the recommended ones for DFN3."
                )
            )
        )
        restore = Gtk.Button(label=_("Restore"), valign=Gtk.Align.CENTER)
        restore.connect("clicked", weak_callback(self, type(self)._restore_tuning))
        self._manual.append(restore)
        body.append(self._manual)
        self.intensity = SettingRow(
            "power-profile-performance-symbolic",
            _("Intensity"),
            _("Lower values preserve more of the original sound."),
            body,
        )
        self.intensity.focus_target = weak_callback(self, lambda self: self._strength)
        self.cards.add_row(rows, self.intensity)

        self.cards.add_row(rows, self._gate_row())

    def _build_devices(self) -> None:
        rows = self._group()
        self.echo = ChoiceRow(
            "audio-speakers-symbolic",
            _("Echo cancellation"),
            _("Prevents sound from your speakers returning through the microphone."),
            [_("Automatic"), _("Always on"), _("Off")],
        )
        self.echo.connect("notify::selected", weak_callback(self, type(self)._on_echo))
        self.cards.add_row(rows, self.echo)

        # Rebuilt only when the sources change, so a volume reading never takes
        # the keyboard focus from a tile.
        self._sources = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=6,
            accessible_role=Gtk.AccessibleRole.GROUP,
        )
        self._sources.update_property(
            [Gtk.AccessibleProperty.LABEL], [_("Microphone in use")]
        )
        self.source = SettingRow(
            "audio-input-microphone-symbolic",
            _("Microphone in use"),
            None,
            self._sources,
        )
        self.source.focus_target = weak_callback(
            self,
            lambda self: next(
                (t for t in self._source_tiles.values() if t.get_active()),
                self._sources,
            ),
        )
        self.cards.add_row(rows, self.source)

    def _build_voice(self) -> None:
        rows = self._group()
        # Custom shows the ten bands in the card itself, which widens for them.
        # Shown at once, so the open card is placed again at its new size.
        self._equalizer = self._equalizer_bands()
        self._equalizer.set_visible(False)
        self.voice_sound = ChoiceRow(
            "audio-x-generic-symbolic",
            _("Voice sound"),
            _("Choose a ready-made sound, or customise your voice."),
            [label for _id, label, _backend in _PRESETS],
            extra=[self._equalizer],
            columns=3,
            changes_result=True,
        )
        self.voice_sound.card.set_maximum_size(620)
        self.voice_sound.card.set_tightening_threshold(620)
        self.voice_sound.connect(
            "notify::selected", weak_callback(self, type(self)._on_preset)
        )
        self.cards.add_row(rows, self.voice_sound)

        self.cards.add_row(rows, self._volume_row())

    def _equalizer_bands(self) -> Gtk.Box:
        bars = Gtk.Box(spacing=4, homogeneous=True)
        for index, hz in enumerate(_BANDS):
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
            value = Gtk.Label(
                label="0 dB", css_classes=["caption", "dim-label", "numeric"]
            )
            scale = Gtk.Scale.new_with_range(Gtk.Orientation.VERTICAL, -40, 40, 1)
            scale.set_inverted(True)
            scale.set_draw_value(False)
            scale.set_size_request(-1, 150)
            scale.add_mark(0, Gtk.PositionType.LEFT, None)
            scale.update_property(
                [Gtk.AccessibleProperty.LABEL], [_("%s Hz gain") % str(hz)]
            )
            scale.connect(
                "value-changed", weak_callback(self, type(self)._on_band), index
            )
            box.append(value)
            box.append(scale)
            box.append(
                Gtk.Label(
                    label=f"{hz // 1000}k" if hz >= 1000 else str(hz),
                    css_classes=["caption", "dim-label"],
                )
            )
            bars.append(box)
            self._eq_scales.append(scale)
            self._eq_values.append(value)
        reset = Gtk.Button(label=_("Restore custom equalizer"), halign=Gtk.Align.START)
        reset.connect("clicked", weak_callback(self, lambda self, _b: self._reset_eq()))
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        content.append(
            _caption(_("Up boosts a band, down reduces it. 0 dB leaves it unchanged."))
        )
        content.append(bars)
        content.append(reset)
        return content

    def _gate_row(self) -> SettingRow:
        # On and off as tiles, as the other switches, and how readily the
        # detector hears a voice under them.
        self._gate_tiles = ChoiceTiles(
            _("Noise that still gets through"), [_("Off"), _("On")], columns=2
        )
        self._gate_tiles.connect(
            "notify::selected", weak_callback(self, type(self)._on_gate_tiles)
        )
        self._sensitivity = Gtk.Scale(
            adjustment=Gtk.Adjustment(
                lower=1, upper=5, step_increment=1, page_increment=1
            ),
            round_digits=0,
            draw_value=False,
        )
        for level in range(1, 6):
            self._sensitivity.add_mark(level, Gtk.PositionType.BOTTOM, None)
        self._sensitivity_value = Gtk.Label()
        self._sensitivity.connect(
            "value-changed", weak_callback(self, type(self)._on_sensitivity)
        )
        self._detection = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        self._detection.append(
            _scale_box(_("Voice detection"), self._sensitivity, self._sensitivity_value)
        )
        self._detection.append(
            _caption(
                _(
                    "Sensitive keeps quiet voices and lets more noise through; strict "
                    "mutes more noise and may cut faint words."
                )
            )
        )
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        body.append(self._gate_tiles)
        body.append(self._detection)
        self.voice_gate = SettingRow(
            "microphone-sensitivity-muted-symbolic",
            _("Noise that still gets through"),
            _("Mutes leftover noise while you are not talking."),
            body,
            switch=Gtk.Switch(),
        )
        self.voice_gate.focus_target = weak_callback(
            self, lambda self: self._gate_tiles.buttons[self._gate_tiles.get_selected()]
        )
        self.voice_gate.switch.connect(
            "notify::active", weak_callback(self, type(self)._on_voice_gate)
        )
        return self.voice_gate

    def _volume_row(self) -> SettingRow:
        self._volume_status = _caption("")
        self._volume_scale = Gtk.Scale.new_with_range(
            Gtk.Orientation.HORIZONTAL, 0, cli.MAX_CAPTURE_PERCENT, 1
        )
        self._volume_scale.set_draw_value(False)
        self._volume_scale.connect(
            "value-changed", weak_callback(self, type(self)._on_volume)
        )
        self._volume_value = Gtk.Label()
        self._manual_volume = _scale_box(
            _("Microphone volume"), self._volume_scale, self._volume_value
        )
        follow_label = Gtk.Label(label=_("Follow my voice"), xalign=0, hexpand=True)
        self._automatic = Gtk.Switch(valign=Gtk.Align.CENTER)
        self._automatic.update_property(
            [Gtk.AccessibleProperty.LABEL], [follow_label.get_label()]
        )
        self._automatic.connect(
            "notify::active", weak_callback(self, type(self)._on_automatic)
        )
        follow = Gtk.Box(spacing=12)
        follow.append(follow_label)
        follow.append(self._automatic)
        self._low = Gtk.SpinButton.new_with_range(0, cli.MAX_CAPTURE_PERCENT - 1, 1)
        self._high = Gtk.SpinButton.new_with_range(1, cli.MAX_CAPTURE_PERCENT, 1)
        limits = Gtk.Grid(column_spacing=12, row_spacing=6)
        for row, (label, spin) in enumerate(
            (
                (_("Minimum volume (%)"), self._low),
                (_("Maximum volume (%)"), self._high),
            )
        ):
            name = Gtk.Label(label=label, xalign=0, hexpand=True)
            spin.update_property([Gtk.AccessibleProperty.LABEL], [label])
            spin.connect("value-changed", weak_callback(self, type(self)._on_limits))
            limits.attach(name, 0, row, 1, 1)
            limits.attach(spin, 1, row, 1, 1)
        self._apply = Gtk.Button(
            label=_("Apply volume limits"),
            halign=Gtk.Align.END,
            css_classes=["suggested-action"],
        )
        self._apply.connect(
            "clicked", weak_callback(self, lambda self, _b: self._save_limits())
        )
        self._limits = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self._limits.append(limits)
        self._limits.append(self._apply)
        self._adjust = Gtk.Button(label=_("Adjust volume"), halign=Gtk.Align.START)
        self._adjust.connect(
            "clicked",
            weak_callback(self, lambda self, _b: self.ctx.navigate("calibrate")),
        )
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        for widget in (
            self._volume_status,
            self._manual_volume,
            follow,
            self._limits,
            self._adjust,
        ):
            body.append(widget)
        self.volume = SettingRow(
            "audio-volume-high-symbolic",
            _("Microphone volume"),
            None,
            body,
        )
        return self.volume

    def _on_noise(self, row, _param) -> None:
        if self._loading or self._snapshot is None:
            return
        model = _NOISE_MODELS[row.get_selected()]
        if model is None:
            self.ctx.state.set("denoiser", "off")
            return
        if model != self._snapshot.chosen_model:
            self.ctx.state.set("model", MODELS[model][0])
        if not self._snapshot.filter_on:
            self.ctx.state.set("denoiser", "on")

    def _on_echo(self, row, _param) -> None:
        if not self._loading:
            self.ctx.state.set("echo", _ECHO_VALUES[row.get_selected()])

    def _on_strength(self, scale: Gtk.Scale) -> None:
        value = int(scale.get_value())
        self._strength_value.set_text(f"{value}%")
        self.intensity.show_value(f"{value}%", value != 100)
        if not self._loading:
            self.ctx.state.set_debounced("mic-intensity", str(value), 250)

    def _restore_tuning(self, _button: Gtk.Button) -> None:
        nr = self._snapshot.noise_reduction if self._snapshot else {}
        for key, field, recommended, value in _RESTORE_TUNING:
            if nr.get(field, recommended) != recommended:
                self.ctx.state.set(key, value)

    def _on_gate_tiles(self, tiles, _param) -> None:
        self.voice_gate.switch.set_active(tiles.get_selected() == 1)

    def _on_voice_gate(self, switch: Gtk.Switch, _param) -> None:
        active = switch.get_active()
        self._gate_tiles.set_selected(int(active))
        self._show_gate()
        if not self._loading:
            self.ctx.state.set("voice-gate", "on" if active else "off")

    def _on_sensitivity(self, scale: Gtk.Scale) -> None:
        self._show_gate()
        if not self._loading:
            self.ctx.state.set_debounced(
                "voice-sensitivity", str(round(scale.get_value())), 250
            )

    def _show_gate(self) -> None:
        active = self.voice_gate.switch.get_active()
        name = _SENSITIVITY_NAMES[round(self._sensitivity.get_value()) - 1]
        self._sensitivity_value.set_text(name)
        self._detection.set_sensitive(active)
        self.voice_gate.show_value(name if active else _("Off"), active)

    def _on_preset(self, row, _param) -> None:
        ui_id, _label, backend = _PRESETS[row.get_selected()]
        self._show_equalizer(ui_id == "custom")
        if self._loading:
            return
        self.ctx.state.cancel_pending("mic-eq-bands")
        if ui_id == "custom":
            self.ctx.state.set("mic-eq-bands", self._bands_text(self._custom_bands))
        else:
            self.ctx.state.set("mic-eq-preset", backend)
        self.ctx.state.set("mic-equalizer", "off" if ui_id == "natural" else "on")

    @staticmethod
    def _bands_text(bands) -> str:
        return ",".join(
            str(int(v)) if float(v).is_integer() else f"{v:.1f}" for v in bands
        )

    def _on_volume(self, scale: Gtk.Scale) -> None:
        value = round(scale.get_value())
        self._volume_value.set_text(f"{value}%")
        if self._loading or self._snapshot is None:
            return
        source = self._snapshot.current_source
        self._volume_debounce.call(lambda: self.ctx.state.set_volume(source, value))

    def _on_automatic(self, switch: Gtk.Switch, _param) -> None:
        self._limits.set_visible(switch.get_active())
        self._manual_volume.set_visible(not switch.get_active())
        if self._loading or self._snapshot is None:
            return
        if switch.get_active():
            self._save_limits()
        else:
            self.ctx.state.set_level_range(self._snapshot.current_source, None)

    def _on_limits(self, *_args) -> None:
        self._apply.set_sensitive(self._low.get_value() < self._high.get_value())

    def _save_limits(self) -> None:
        low, high = int(self._low.get_value()), int(self._high.get_value())
        if self._snapshot is not None and low < high:
            self.ctx.state.set_level_range(self._snapshot.current_source, (low, high))

    def _show_equalizer(self, show: bool) -> None:
        if self._equalizer.get_visible() != show:
            self._equalizer.set_visible(show)
            # The card grows or shrinks to the bands, once they are laid out.
            GLib.idle_add(
                weak_callback(
                    self,
                    lambda self: (
                        self.cards.hover_cards.refit(self.voice_sound) and False
                    ),
                )
            )
        if show:
            self._loading, loading = True, self._loading
            try:
                for scale, value in zip(
                    self._eq_scales, self._custom_bands, strict=True
                ):
                    scale.set_value(value)
            finally:
                self._loading = loading

    def _on_band(self, scale: Gtk.Scale, index: int) -> None:
        value = float(scale.get_value())
        self._eq_values[index].set_text(f"{int(value):+d} dB")
        if self._loading:
            return
        self._custom_bands[index] = value
        self.ctx.state.set_debounced(
            "mic-eq-bands", self._bands_text(self._custom_bands), 220
        )

    def _reset_eq(self) -> None:
        self.ctx.state.cancel_pending("mic-eq-bands")
        self._custom_bands = [0.0] * 10
        self._show_equalizer(True)
        self.ctx.state.set("mic-eq-bands", self._bands_text(self._custom_bands))

    def sync(self, snapshot) -> None:
        previous = self._snapshot
        self._snapshot = snapshot
        if not snapshot.loaded:
            return
        has_source = any(
            source.get("name") == snapshot.current_source
            and source.get("available", True)
            for source in snapshot.sources
        )
        no_microphone = None if has_source else _("No microphone available")
        self._loading = True
        try:
            if not (
                self.ctx.state.is_pending("denoiser")
                or self.ctx.state.is_pending("model")
            ):
                # DFN3-LL, chosen elsewhere, shows as the light model by name.
                self.noise.set_selected(
                    0
                    if not snapshot.filter_on
                    else 2
                    if snapshot.chosen_model == 13
                    else 1
                )
                if snapshot.filter_on and snapshot.chosen_model == 12:
                    self.noise.show_value(MODELS[12][1], True)
            self.noise.set_unavailable(no_microphone)
            self.echo.set_selected(
                {"auto": 0, "on": 1, "off": 2}.get(snapshot.echo_mode, 0)
            )
            self._sync_sources(snapshot)
            self._sync_cleaning(snapshot)
            self._sync_voice_sound(snapshot)
            self._sync_volume(snapshot, previous, has_source)
        finally:
            self._loading = False

    def _sync_sources(self, snapshot) -> None:
        current = next(
            (s for s in snapshot.sources if s.get("name") == snapshot.current_source),
            None,
        )
        self.source.show_value(
            str(current.get("description") or current.get("name", ""))
            if current
            else _("No microphone available")
        )
        signature = (
            snapshot.current_source,
            tuple(
                (
                    s.get("name"),
                    s.get("description"),
                    s.get("virtual"),
                    s.get("available"),
                )
                for s in snapshot.sources
            ),
        )
        if signature == self._source_signature:
            return
        self._source_signature = signature
        child = self._sources.get_first_child()
        while child is not None:
            following = child.get_next_sibling()
            self._sources.remove(child)
            child = following
        self._source_tiles = {}
        first = None
        for source in snapshot.sources:
            name = str(source.get("name", ""))
            if not source.get("available", True):
                detail = _("Input disconnected")
            elif source.get("virtual"):
                detail = _("These inputs may already apply sound effects.")
            else:
                detail = ""
            tile = option_tile(str(source.get("description") or name), detail)
            if first is None:
                first = tile
            else:
                tile.set_group(first)
            tile.set_active(name == snapshot.current_source)
            tile.set_sensitive(source.get("available", True))
            tile.connect("toggled", weak_callback(self, type(self)._choose), name)
            self._sources.append(tile)
            self._source_tiles[name] = tile
        if not snapshot.sources:
            self._sources.append(
                _caption(
                    _(
                        "Connect a microphone. For Bluetooth, enable the headset microphone in your system sound settings."
                    )
                )
            )

    def _choose(self, tile: Gtk.CheckButton, name: str) -> None:
        snapshot = self._snapshot
        if (
            self._loading
            or self._choosing
            or not tile.get_active()
            or snapshot is None
            or name == snapshot.current_source
        ):
            return
        self._choosing = True
        self._sources.set_sensitive(False)

        def completed(message=None) -> None:
            self._choosing = False
            self._sources.set_sensitive(True)
            # The tiles follow the next reading, whatever the outcome.
            self._source_signature = None
            self.ctx.state.reload()
            if message is not None:
                self.ctx.toast(str(message)[:200])

        self.ctx.state.call(
            lambda: cli.set_default_source(name),
            lambda _result: completed(),
            completed,
        )

    def _sync_cleaning(self, snapshot) -> None:
        off = None if snapshot.filter_on else _("Noise reduction is off")
        nr = snapshot.noise_reduction
        if not self.ctx.state.is_pending("mic-intensity"):
            self._strength.set_value(
                max(0, min(100, round(float(nr.get("strength", 1.0)) * 100)))
            )
        self._on_strength(self._strength)
        self._manual.set_visible(
            MODELS[snapshot.chosen_model][0] != "dpdfnet" and snapshot.manual_tuning
        )
        if not self.ctx.state.is_pending("voice-gate"):
            self.voice_gate.switch.set_active(snapshot.voice_gate)
        if not self.ctx.state.is_pending("voice-sensitivity"):
            self._sensitivity.set_value(snapshot.voice_sensitivity)
        self._show_gate()
        for row in (self.intensity, self.voice_gate):
            row.set_unavailable(off)

    def _sync_voice_sound(self, snapshot) -> None:
        if self.ctx.state.is_pending("mic-eq-bands"):
            return
        eq = snapshot.equalizer
        preset = str(eq.get("preset", "flat")) if eq.get("enabled") else "flat"
        backends = [backend for _id, _label, backend in _PRESETS]
        bands = eq.get("bands")
        if preset not in backends:
            # Older presets stay audible and editable as custom bands rather
            # than named as a profile they are not.
            preset = "custom"
        if preset == "custom" and isinstance(bands, list) and len(bands) == 10:
            self._custom_bands = [float(v) for v in bands]
        self.voice_sound.set_selected(backends.index(preset))
        self._show_equalizer(preset == "custom")

    def _sync_volume(self, snapshot, previous, has_source: bool) -> None:
        self.volume.set_unavailable(
            None if has_source else _("No microphone available")
        )
        limits = calibration_window(snapshot.calibration())
        self._automatic.set_active(limits is not None)
        self._on_automatic(self._automatic, None)
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
        if snapshot.volume is not None and not self._volume_debounce.pending:
            self._volume_scale.set_value(snapshot.volume)
            self._volume_value.set_text(f"{snapshot.volume}%")
        self._adjust.set_sensitive(snapshot.ready and not snapshot.muted)
        if limits is not None:
            self.volume.show_value(_("Automatic"), True)
            self._volume_status.set_text(
                _("Follows your voice between %d%% and %d%%")
                % (round(limits[0] * 100), round(limits[1] * 100))
            )
        else:
            self.volume.show_value(
                f"{snapshot.volume}%" if snapshot.volume is not None else ""
            )
            self._volume_status.set_text(
                _("Adjusted for your voice")
                if snapshot.calibration().get("volume")
                else _("Not adjusted yet — measure once for this microphone")
            )

    def shutdown(self) -> None:
        self._volume_debounce.cancel()
        self.cards.cleanup()
