from __future__ import annotations

import math

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gtk

from biglinux_microphone import cli
from biglinux_microphone.ui.pages.base import Page, column, scrolled
from biglinux_microphone.ui.state import MODELS
from biglinux_microphone.utils.i18n import _


class DiagnosticsPage(Page):
    title = _("Diagnostics")
    id = "diagnostics"
    icon = "dialog-information-symbolic"

    def build(self) -> Gtk.Widget:
        body = column()
        self._status = Adw.Banner(
            title=_("Checking the audio system…"), revealed=True, use_markup=False
        )
        body.append(self._status)

        group = Adw.PreferencesGroup(title=_("Audio system details"))
        group.set_description(
            _(
                "Latency is an estimate of processing only; it excludes your device and applications."
            )
        )
        self._rows: dict[str, Adw.ActionRow] = {}
        for key, title in (
            ("chosen", _("Model selected")),
            ("model", _("Model in use")),
            ("microphone", _("Microphone")),
            ("latency", _("Estimated latency")),
        ):
            row = Adw.ActionRow(title=title, subtitle=_("Unknown"), use_markup=False)
            group.add(row)
            self._rows[key] = row
        body.append(group)
        # Do not show a disabled promise for functionality the current backend
        # cannot fulfil.  A real copy/report action can be added when the backend
        # exposes one atomic diagnostics payload.
        self._snapshot = None
        self._shown = False
        self._generation = 0
        self._signature = None
        return scrolled(body, max_width=880)

    def sync(self, snapshot) -> None:
        self._snapshot = snapshot
        if not snapshot.loaded:
            self._generation += 1
            self._status.set_title(
                str(snapshot.error)[:160]
                if snapshot.error
                else _("Checking the audio system…")
            )
            self._rows["latency"].set_subtitle(_("Unknown"))
            return
        has_source = any(
            s.get("name") == snapshot.current_source for s in snapshot.sources
        )
        status = (
            _("Settings loaded — listen back to check the sound.")
            if has_source
            else _("No microphone available")
        )
        self._status.set_title(str(snapshot.error)[:160] if snapshot.error else status)
        signature = (
            snapshot.current_source,
            snapshot.model,
            snapshot.filter_on,
            snapshot.echo_mode,
        )
        if signature != self._signature:
            self._signature = signature
            self._rows["latency"].set_subtitle(_("Unknown"))
            if self._shown:
                self._request_latency()
        self._rows["chosen"].set_subtitle(MODELS[snapshot.chosen_model][1])
        self._rows["model"].set_subtitle(snapshot.model_label or _("Unknown"))
        label = next(
            (
                s.get("description", s.get("name", ""))
                for s in snapshot.sources
                if s.get("name") == snapshot.current_source
            ),
            snapshot.current_source or _("None"),
        )
        self._rows["microphone"].set_subtitle(str(label))

    def on_shown(self) -> None:
        self._shown = True
        self._request_latency()

    def _request_latency(self) -> None:
        if self._snapshot is None or not self._snapshot.loaded:
            return
        self._generation += 1
        generation = self._generation
        self._rows["latency"].set_subtitle(_("Unknown"))
        model = (
            MODELS[self._snapshot.model][0] if self._snapshot.model in MODELS else ""
        )
        self.ctx.state.call(
            lambda: cli.latency(model),
            lambda report: self._latency(report, generation),
            lambda _m: None,
        )

    def on_hidden(self) -> None:
        self._shown = False
        self._generation += 1

    def shutdown(self) -> None:
        self.on_hidden()

    def _latency(self, report: dict, generation: int) -> None:
        if not self._shown or generation != self._generation:
            return
        value = report.get("total_ms")
        if type(value) in (int, float) and math.isfinite(value) and value >= 0:
            self._rows["latency"].set_subtitle(_("%.1f ms") % float(value))
