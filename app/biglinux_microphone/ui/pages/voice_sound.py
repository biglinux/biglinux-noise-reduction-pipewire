from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gtk

from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.base import Page, column, scrolled
from biglinux_microphone.utils.i18n import _

_BANDS = (31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000)
_PRESETS = (
    ("natural", _("Natural"), "flat", "audio-card-symbolic"),
    ("clarity", _("More clarity"), "presence", "star-new-symbolic"),
    ("warm", _("Fuller voice"), "warm", "audio-speakers-symbolic"),
    ("podcast", _("Podcast"), "podcast", "audio-input-microphone-symbolic"),
    ("custom", _("Custom"), "custom", "preferences-system-symbolic"),
)


class VoiceSoundPage(Page):
    title = _("Voice sound")
    id = "voice_sound"
    icon = "audio-speakers-symbolic"

    def build(self) -> Gtk.Widget:
        self._loading = True
        self._custom_bands = [0.0] * 10
        body = column()

        group = Adw.PreferencesGroup(
            title=_("Voice profile"),
            description=_("Choose a ready-made sound, or customise your voice."),
        )
        flow = Gtk.FlowBox(
            selection_mode=Gtk.SelectionMode.NONE,
            homogeneous=True,
            row_spacing=8,
            column_spacing=8,
            min_children_per_line=1,
            max_children_per_line=5,
        )
        self._preset_buttons: dict[str, Gtk.ToggleButton] = {}
        first: Gtk.ToggleButton | None = None
        for ui_id, label, backend_id, icon in _PRESETS:
            content = Gtk.Box(
                orientation=Gtk.Orientation.VERTICAL,
                spacing=5,
                margin_top=8,
                margin_bottom=8,
            )
            content.append(Gtk.Image.new_from_icon_name(icon))
            content.append(
                Gtk.Label(label=label, wrap=True, justify=Gtk.Justification.CENTER)
            )
            button = Gtk.ToggleButton(child=content, css_classes=["preset-button"])
            button.update_property([Gtk.AccessibleProperty.LABEL], [label])
            button.set_size_request(120, 72)
            if first is None:
                first = button
            else:
                button.set_group(first)
            button.connect(
                "toggled", weak_callback(self, type(self)._on_preset), ui_id, backend_id
            )
            self._preset_buttons[ui_id] = button
            flow.insert(button, -1)
            button.get_parent().set_focusable(False)
        group.add(flow)
        body.append(group)

        self._custom_reveal = Gtk.Revealer(
            transition_type=Gtk.RevealerTransitionType.SLIDE_DOWN
        )
        self._eq_group = None
        self._scales: list[Gtk.Scale] = []
        self._values: list[Gtk.Label] = []
        body.append(self._custom_reveal)
        return scrolled(body, max_width=980)

    def _ensure_equalizer(self) -> None:
        if self._eq_group is not None:
            return
        eq_group = Adw.PreferencesGroup(
            title=_("Equalizer"),
            description=_(
                "Up boosts a band, down reduces it. 0 dB leaves it unchanged."
            ),
        )
        bars = Gtk.FlowBox(
            selection_mode=Gtk.SelectionMode.NONE,
            min_children_per_line=1,
            max_children_per_line=2,
            column_spacing=7,
            row_spacing=10,
            homogeneous=True,
            css_classes=["card", "eq-bands"],
        )
        self._scales: list[Gtk.Scale] = []
        self._values: list[Gtk.Label] = []
        band_group = None
        for index, hz in enumerate(_BANDS):
            # Keep the ten controls balanced: 10 on wide pages, 5 + 5 on
            # narrow pages, never 9 + 1. Each scale retains a touch target.
            if index % 5 == 0:
                band_group = Gtk.Box(spacing=7, homogeneous=True, hexpand=True)
                bars.insert(band_group, -1)
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4, hexpand=True)
            value = Gtk.Label(
                label="0 dB", css_classes=["caption", "dim-label", "numeric"]
            )
            scale = Gtk.Scale.new_with_range(Gtk.Orientation.VERTICAL, -40, 40, 1)
            scale.set_inverted(True)
            scale.set_draw_value(False)
            # Keep the editor compact.  ``vexpand`` made ten scales stretch
            # to the full page height on desktop displays, hiding their labels
            # and making a simple equalizer look like a mixing console.
            scale.set_vexpand(False)
            scale.set_size_request(48, 180)
            scale.add_mark(0, Gtk.PositionType.LEFT, None)
            label = f"{hz // 1000}k" if hz >= 1000 else str(hz)
            scale.update_property(
                [Gtk.AccessibleProperty.LABEL], [_("%s Hz gain") % str(hz)]
            )
            scale.connect(
                "value-changed",
                weak_callback(self, type(self)._on_band),
                len(self._scales),
            )
            box.append(value)
            box.append(scale)
            box.append(Gtk.Label(label=label, css_classes=["caption", "dim-label"]))
            band_group.append(box)
            self._scales.append(scale)
            self._values.append(value)
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        content.append(bars)
        reset = Adw.ButtonRow(
            title=_("Restore custom equalizer"), start_icon_name="edit-undo-symbolic"
        )
        reset.connect("activated", weak_callback(self, lambda self, _r: self._reset()))
        reset_list = Gtk.ListBox(
            selection_mode=Gtk.SelectionMode.NONE, css_classes=["boxed-list"]
        )
        reset_list.append(reset)
        content.append(reset_list)
        eq_group.add(content)
        self._eq_group = eq_group

    def _show_custom(self, show: bool) -> None:
        if show:
            self._ensure_equalizer()
            self._custom_reveal.set_child(self._eq_group)
        self._custom_reveal.set_reveal_child(show)
        if not show:
            # Detached controls neither influence narrow measurements nor get
            # painted by software renderers while the revealer is collapsed.
            self._custom_reveal.set_child(None)

    def _on_preset(self, button: Gtk.ToggleButton, ui_id: str, backend_id: str) -> None:
        if self._loading or not button.get_active():
            return
        self.ctx.state.cancel_pending("mic-eq-bands")
        if ui_id == "custom":
            self._show_custom(True)
            self._loading = True
            try:
                for scale, value in zip(self._scales, self._custom_bands, strict=True):
                    scale.set_value(value)
            finally:
                self._loading = False
            values = ",".join(
                str(int(v)) if float(v).is_integer() else f"{v:.1f}"
                for v in self._custom_bands
            )
            self.ctx.state.set("mic-eq-bands", values)
            self.ctx.state.set("mic-equalizer", "on")
            return
        self._show_custom(False)
        if ui_id == "natural":
            self.ctx.state.set("mic-eq-preset", backend_id)
            self.ctx.state.set("mic-equalizer", "off")
        else:
            self.ctx.state.set("mic-eq-preset", backend_id)
            self.ctx.state.set("mic-equalizer", "on")

    def _on_band(self, scale: Gtk.Scale, index: int) -> None:
        value = float(scale.get_value())
        self._values[index].set_text(f"{int(value):+d} dB")
        if self._loading:
            return
        self._custom_bands[index] = value
        values = ",".join(str(int(s.get_value())) for s in self._scales)
        self.ctx.state.set_debounced("mic-eq-bands", values, 220)

    def _reset(self) -> None:
        self.ctx.state.cancel_pending("mic-eq-bands")
        self._loading = True
        try:
            self._custom_bands = [0.0] * 10
            for scale in self._scales:
                scale.set_value(0)
        finally:
            self._loading = False
        self.ctx.state.set("mic-eq-bands", ",".join("0" for _ in _BANDS))

    def sync(self, snapshot) -> None:
        if not snapshot.loaded or self.ctx.state.is_pending("mic-eq-bands"):
            return
        eq = snapshot.equalizer
        preset = str(eq.get("preset", "flat"))
        enabled = bool(eq.get("enabled"))
        bands = eq.get("bands")
        if preset == "custom" and isinstance(bands, list) and len(bands) == 10:
            self._custom_bands = [float(v) for v in bands]

        if not enabled or preset == "flat":
            ui_id = "natural"
        elif preset == "presence":
            ui_id = "clarity"
        elif preset == "warm":
            ui_id = "warm"
        elif preset == "podcast":
            ui_id = "podcast"
        elif preset == "custom":
            ui_id = "custom"
        else:
            # Older presets remain audible and editable; present them as custom
            # rather than lying that a different named profile is active.
            ui_id = "custom"
            if isinstance(bands, list) and len(bands) == 10:
                self._custom_bands = [float(v) for v in bands]

        self._loading = True
        try:
            self._preset_buttons[ui_id].set_active(True)
            self._show_custom(ui_id == "custom")
            if ui_id == "custom":
                for i, (scale, value) in enumerate(
                    zip(self._scales, self._custom_bands, strict=True)
                ):
                    scale.set_value(value)
                    self._values[i].set_text(f"{int(value):+d} dB")
        finally:
            self._loading = False
