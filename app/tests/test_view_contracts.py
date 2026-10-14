"""Headless GTK contracts for the beginner-first microphone UI.

The Rust backend is stubbed: the tests check navigation and progressive
disclosure without touching PipeWire or the user's audio.
"""

from __future__ import annotations

import unittest
from dataclasses import replace
from unittest.mock import patch

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib

from biglinux_microphone import cli
from biglinux_microphone.ui.view import MicrophoneView


def _settings(*, model: int = 13, calibrated: bool = False, **noise_reduction) -> dict:
    calibration = (
        {"devices": {"mic": {"volume": 1.25}}} if calibrated else {"devices": {}}
    )
    return {
        "mic_bypass": False,
        "noise_reduction": {
            "enabled": True,
            "strength": 1.0,
            "model": model,
            "silence_depth": 20,
            **noise_reduction,
        },
        "equalizer": {"preset": "flat", "bands": [0.0] * 10, "enabled": False},
        "echo_cancel": {"mode": "auto"},
        "mic_calibration": calibration,
        "capabilities": {},
    }


class ViewContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        Adw.init()
        if Gdk.Display.get_default() is None:
            raise unittest.SkipTest("no display for GTK")

    def _view(
        self, settings=None, *, sources=None, current_source="mic"
    ) -> MicrophoneView:
        settings = settings or _settings()
        if sources is None:
            sources = [{"name": "mic", "description": "Test Mic", "volume": 125}]
        stubs = (
            patch.object(
                cli,
                "load_status",
                side_effect=lambda: {
                    "settings": settings,
                    "audio_available": True,
                    "mic_running": True,
                    "mic_enabled": True,
                    "denoising": settings["noise_reduction"]["enabled"],
                    "effective_model": settings["noise_reduction"]["model"],
                    "mic_serial": "42",
                },
            ),
            patch.object(cli, "list_sources", return_value=sources),
            patch.object(cli, "get_default_source", return_value=current_source),
            patch.object(cli, "get_source_volume", return_value=125),
            patch.object(
                cli,
                "eq_presets",
                return_value=["flat", "presence", "warm", "podcast", "custom"],
            ),
            patch.object(MicrophoneView, "_start_spectrum"),
        )
        for stub in stubs:
            stub.start()
            self.addCleanup(stub.stop)
        view = MicrophoneView(on_toast=lambda _msg: None)
        self.addCleanup(view.shutdown)
        view._cli_worker.submit(lambda: None).result(timeout=2)
        context = GLib.MainContext.default()
        for _ in range(30):
            if not context.pending():
                break
            context.iteration(False)
        return view

    def test_sidebar_has_three_groups(self) -> None:
        view = self._view()
        controls = view._controls
        groups = [rows for rows, _count in controls.cards.groups]
        self.assertEqual(
            groups,
            [
                [
                    controls.noise,
                    controls.intensity,
                    controls.voice_gate,
                ],
                [controls.echo, controls.source],
                [controls.voice_sound, controls.volume],
            ],
        )
        self.assertEqual([page.id for page in view._pages], ["home"])
        self.assertEqual(view._visible_id(), "home")

    def test_tasks_return_to_panel(self) -> None:
        view = self._view()
        view.activate_action("view.diagnostics", None)
        self.assertEqual(view._visible_id(), "diagnostics")
        view.navigate("home")
        self.assertEqual(view._visible_id(), "home")
        self.assertFalse(view._back.get_visible())

    def test_panel_hides_model_name(self) -> None:
        view = self._view(_settings(model=13, calibrated=False))
        status = view._home._status.get_text()
        self.assertIn("reducing background noise", status)
        self.assertNotIn("DPDF", status)
        self.assertEqual(view._controls.noise.value_label.get_text(), "High quality")
        self.assertEqual(view._home._chain.get_text(), "Test Mic · DPDFNet")
        self.assertIn("Not adjusted", view._controls._volume_status.get_text())

    def test_panel_explains_missing_microphone(self) -> None:
        view = self._view(sources=[], current_source="")
        home = view._home
        self.assertTrue(home._source_notice.get_revealed())
        self.assertEqual(home._source_notice.get_title(), "No microphone available")
        for row in (view._controls.noise, view._controls.volume):
            self.assertFalse(row.get_sensitive())
            self.assertEqual(row.unavailable, "No microphone available")

    def test_rows_report_the_saved_settings(self) -> None:
        # DFN3-LL, chosen elsewhere, shows as the light model by its name.
        for model, index, name in (
            (11, 1, "Light filter"),
            (12, 1, "DFN3-LL"),
            (13, 2, "High quality"),
        ):
            with self.subTest(model=model):
                controls = self._view(_settings(model=model))._controls
                self.assertEqual(controls.noise.get_selected(), index)
                self.assertEqual(controls.noise.value_label.get_text(), name)
                self.assertEqual(controls.echo.get_selected(), 0)
                self.assertEqual(controls.intensity.value_label.get_text(), "100%")

    def test_settings_write_on_change_only(self) -> None:
        view = self._view(_settings())
        controls = view._controls
        with (
            patch.object(view._state, "set") as setting,
            patch.object(view._state, "set_volume") as volume,
        ):
            controls.sync(view._state.snapshot)
            setting.assert_not_called()
            volume.assert_not_called()
            controls.noise.set_selected(1)
            setting.assert_called_once_with("model", "dfn3")
            controls.noise.set_selected(0)
            setting.assert_called_with("denoiser", "off")
            controls.echo.set_selected(2)
            setting.assert_called_with("echo", "off")
            controls._volume_scale.set_value(80)
            self.assertTrue(controls._volume_debounce.pending)
            GLib.source_remove(controls._volume_debounce._timer_id)
            controls._volume_debounce._execute()
            volume.assert_called_once_with("mic", 80)

    def test_cleaning_rows_wait_for_noise_reduction(self) -> None:
        settings = _settings()
        settings["noise_reduction"]["enabled"] = False
        controls = self._view(settings)._controls
        self.assertEqual(controls.noise.get_selected(), 0)
        for row in (controls.intensity, controls.voice_gate):
            self.assertEqual(row.unavailable, "Noise reduction is off")
        self.assertIsNone(controls.echo.unavailable)
        writes = []
        controls.ctx.state.set = lambda key, value: writes.append((key, value))
        controls.noise.set_selected(1)
        # The saved model is DPDFNet: the light one is written, then turned on.
        self.assertEqual(writes, [("model", "dfn3"), ("denoiser", "on")])

    def test_voice_gate_choice_per_model(self) -> None:
        for model, voice_gate in ((11, True), (12, False), (13, True)):
            with self.subTest(model=model):
                controls = self._view(
                    _settings(
                        model=model,
                        voice_gate=voice_gate,
                        dpdfnet_voice_gate=voice_gate,
                    )
                )._controls
                self.assertEqual(controls.voice_gate.switch.get_active(), voice_gate)
                writes = []
                controls.ctx.state.set = lambda key, value, writes=writes: (
                    writes.append((key, value))
                )
                controls.cards._on_activated(None, controls.voice_gate)
                self.assertEqual(
                    writes, [("voice-gate", "off" if voice_gate else "on")]
                )
        # DPDFNet's gate is off until asked for, whatever DFN3's choice.
        controls = self._view(_settings(model=13, voice_gate=True))._controls
        self.assertFalse(controls.voice_gate.switch.get_active())
        self.assertFalse(controls._detection.get_sensitive())
        self.assertEqual(controls.voice_gate.value_label.get_text(), "Off")

    def test_voice_detection_follows_the_voice_gate(self) -> None:
        controls = self._view(_settings(model=11))._controls
        self.assertTrue(controls._detection.get_sensitive())
        self.assertEqual(controls.voice_gate.value_label.get_text(), "Normal")
        with patch.object(controls.ctx.state, "set_debounced") as write:
            controls._sensitivity.set_value(5)
        write.assert_called_once_with("voice-sensitivity", "5", 250)
        self.assertEqual(controls.voice_gate.value_label.get_text(), "Very strict")

        controls = self._view(
            _settings(model=12, voice_gate=False, voice_sensitivity=1)
        )._controls
        self.assertFalse(controls._detection.get_sensitive())
        self.assertEqual(controls._sensitivity.get_value(), 1)

    def test_manual_tuning_restores_changed_keys(self) -> None:
        controls = self._view(_settings(model=11))._controls
        self.assertFalse(controls._manual.get_visible())
        controls = self._view(
            _settings(model=11, min_snr_override_db=-18.0, startup_mute_ms=1000)
        )._controls
        self.assertTrue(controls._manual.get_visible())
        writes = []
        controls.ctx.state.set = lambda key, value: writes.append((key, value))
        controls._restore_tuning(None)
        self.assertEqual(writes, [("dfn-snr-gate", "auto")])
        dpdf = self._view(_settings(model=13, min_snr_override_db=-18.0))._controls
        self.assertFalse(dpdf._manual.get_visible())

    def test_volume_card_points_back_from_calibration(self) -> None:
        view = self._view()
        view.navigate("calibrate")
        self.assertEqual(view._visible_id(), "calibrate")
        view.navigate("voice_volume")
        self.assertEqual(view._visible_id(), "home")
        self.assertIs(view._controls.cards.hover_cards.pinned, view._controls.volume)

    def test_auto_volume_restores_without_writing(self) -> None:
        settings = _settings(calibrated=True)
        settings["mic_calibration"]["devices"]["mic"].update(
            min_volume=0.8, max_volume=1.2
        )
        view = self._view(settings)
        controls = view._controls
        self.assertFalse(controls._manual_volume.get_visible())
        self.assertEqual(controls.volume.value_label.get_text(), "Automatic")
        with patch.object(view._state, "set_level_range") as write:
            controls.sync(view._state.snapshot)
            write.assert_not_called()
            self.assertTrue(controls._automatic.get_active())
            self.assertEqual(controls._low.get_value(), 80)
            self.assertEqual(controls._high.get_value(), 120)
            controls._low.set_value(130)
            self.assertFalse(controls._apply.get_sensitive())
            controls._high.set_value(150)
            write.assert_not_called()
            controls._apply.emit("clicked")
            write.assert_called_once_with("mic", (130, 150))
            controls._automatic.set_active(False)
            self.assertEqual(write.call_args.args, ("mic", None))

    def test_auto_volume_without_calibration(self) -> None:
        view = self._view()
        with patch.object(view._state, "set_level_range") as write:
            view._controls._automatic.set_active(True)
        write.assert_called_once_with("mic", (105, 145))

    def test_equalizer_is_offered_only_for_custom(self) -> None:
        controls = self._view(_settings())._controls
        self.assertFalse(controls._equalizer.get_visible())
        writes = []
        controls.ctx.state.set = lambda key, value: writes.append((key, value))
        controls.voice_sound.set_selected(2)
        self.assertTrue(controls._equalizer.get_visible())
        self.assertIn(("mic-equalizer", "on"), writes)

    def test_preset_discards_unsent_equalizer_values(self) -> None:
        view = self._view()
        view._state.set_debounced("mic-eq-bands", "obsolete", 10000)
        view._state._queued_values["mic-eq-bands"] = "also obsolete"
        with patch.object(view._state, "set") as write:
            view._controls.voice_sound.set_selected(1)
        self.assertFalse(view._state.is_pending("mic-eq-bands"))
        self.assertNotIn("mic-eq-bands", view._state._queued_values)
        self.assertEqual(write.call_args_list[0].args, ("mic-eq-preset", "podcast"))

    def test_snapshot_keeps_pending_drag(self) -> None:
        settings = _settings()
        settings["equalizer"] = {"preset": "custom", "enabled": True, "bands": [0] * 10}
        view = self._view(settings)
        controls = view._controls
        controls._eq_scales[0].set_value(7)
        controls.sync(view._state.snapshot)
        self.assertEqual(controls._eq_scales[0].get_value(), 7)
        self.assertEqual(controls._custom_bands[0], 7)

    def test_listen_back_follows_the_shell_monitor(self) -> None:
        view = self._view()
        button = view._home._listen_back
        self.assertTrue(button.get_sensitive())
        with patch.object(view, "_start_monitor") as start:
            button.set_active(True)
            start.assert_called_once()
        self.assertTrue(view.monitor_active)
        view._monitor_failed(view._monitor_generation, "no loader")
        self.assertFalse(button.get_active())

    def test_only_the_panel_receives_meter_frames(self) -> None:
        view = self._view()
        with patch.object(view._home, "on_bands") as home:
            view.navigate("calibrate")
            view._on_spectrum_frame([0.2] * 30, 0.5)
            home.assert_not_called()
            view.navigate("home")
            view._on_spectrum_frame([0.2] * 30, 0.5)
            home.assert_called_once()

    def test_task_stops_spectrum_capture(self) -> None:
        view = self._view()
        with (
            patch.object(view, "get_mapped", return_value=True),
            patch.object(view._spectrum_tap, "configure") as capture,
        ):
            view._sync_capture()
            self.assertEqual(capture.call_args.args[0], "mic-biglinux")
            view.navigate("calibrate")
            capture.assert_called_with()
        with (
            patch.object(view, "get_mapped", return_value=False),
            patch.object(view._spectrum_tap, "configure") as capture,
        ):
            view.navigate("home")
            capture.assert_called_with()

    def test_device_description_is_literal_text(self) -> None:
        view = self._view(
            sources=[{"name": "mic", "description": "USB <b>Mic</b> & monitor"}]
        )
        self.assertEqual(
            view._controls.source.value_label.get_text(), "USB <b>Mic</b> & monitor"
        )
        self.assertFalse(view._controls.source.value_label.get_use_markup())

    def test_page_lifecycle_on_real_transitions(self) -> None:
        view = self._view()
        view.navigate("calibrate")
        page = view._ensure_page("calibrate")
        unrelated = view._ensure_page("diagnostics")
        with (
            patch.object(page, "on_shown") as shown,
            patch.object(page, "on_hidden") as hidden,
            patch.object(unrelated, "on_hidden") as untouched,
        ):
            view._page_changed()
            shown.assert_not_called()
            hidden.assert_not_called()
            view.navigate("home")
            hidden.assert_called_once()
            untouched.assert_not_called()

    def test_external_errors_skip_markup(self) -> None:
        from types import SimpleNamespace

        from biglinux_microphone.window import MicrophoneWindow

        view = self._view()
        error = "Device <capture>&USB could not be opened"
        snapshot = replace(view._state.snapshot, error=error)
        view._home.sync(snapshot)
        diagnostics = view._ensure_page("diagnostics")
        diagnostics.sync(snapshot)
        self.assertFalse(view._home._error.get_use_markup())
        self.assertFalse(diagnostics._status.get_use_markup())
        self.assertEqual(diagnostics._status.get_title(), error)
        toasts = []
        window = SimpleNamespace(
            _toast_overlay=SimpleNamespace(add_toast=toasts.append)
        )
        MicrophoneWindow.show_toast(window, error)
        self.assertFalse(toasts[0].get_use_markup())
        self.assertEqual(toasts[0].get_title(), error)

    def test_filter_failure_offers_repair(self):

        view = self._view()
        snapshot = replace(
            view._state.snapshot,
            runtime={
                **view._state.snapshot.runtime,
                "mic_running": False,
                "effective_model": None,
            },
        )
        with patch.object(view._state, "repair") as repair:
            view._state._apply_read(snapshot)
            self.assertNotEqual(view._controls.noise.get_selected(), 0)
            self.assertIn("not ready", view._home._status.get_text())
            self.assertTrue(view._home._runtime_notice.get_revealed())
            repair.assert_not_called()
            view._home._runtime_notice.emit("button-clicked")
            repair.assert_called_once()

    def test_recovery_clears_stale_meter_text(self):

        view = self._view()
        good = view._state.snapshot
        view._state._apply_read(replace(good, muted=True))
        self.assertEqual(view._home._meter_state.get_text(), "Microphone muted")
        self.assertFalse(view._controls._adjust.get_sensitive())
        view._state._apply_read(good)
        self.assertEqual(
            view._home._meter_state.get_text(), "Speak to check your microphone"
        )
        self.assertTrue(view._controls._adjust.get_sensitive())

    def test_calibration_waits_for_explicit_start(self) -> None:
        view = self._view()
        page = view._ensure_page("calibrate")
        view.navigate("calibrate")
        self.assertEqual(view._nav.get_visible_page().get_tag(), "calibrate")
        self.assertEqual(page._stack.get_visible_child_name(), "prepare")
        self.assertFalse(page._running)

    def test_calibration_hides_raw_gain(self) -> None:
        view = self._view()
        page = view._ensure_page("calibrate")
        page._finished({"heard_speech": True, "volume": 1.71, "device": "mic"})
        self.assertEqual(page._stack.get_visible_child_name(), "result")
        self.assertFalse(hasattr(page, "_result_percent"))

    def test_monitor_spawns_once(self) -> None:
        view = self._view()
        with patch.object(view, "_start_monitor") as start:
            view.set_monitor(True)
            view.set_monitor(True)
        start.assert_called_once_with()

    def test_monitor_exit_clears_ui_state(self) -> None:
        view = self._view()
        proc = object()
        view.monitor_active = True
        view._monitor_generation = 4
        view._monitor_proc = proc
        self.assertFalse(view._monitor_exited(4, proc, 1))
        self.assertFalse(view.monitor_active)
        self.assertIsNone(view._monitor_proc)

    def test_desktop_sidebar_stays_visible(self) -> None:
        view = self._view()
        view._set_narrow(True)
        self.assertFalse(view._split.get_show_sidebar())
        view._sidebar_button.set_active(True)
        self.assertTrue(view._split.get_show_sidebar())
        # Closing the overlay by clicking outside it must update the button.
        view._split.set_show_sidebar(False)
        self.assertFalse(view._sidebar_button.get_active())
        view._set_narrow(False)
        self.assertTrue(view._split.get_show_sidebar())
        self.assertFalse(view._sidebar_button.get_visible())

    def test_peak_meter_attack_decay_hold(self) -> None:
        view = self._view()
        meter = view._home._meter
        with patch(
            "biglinux_microphone.ui.widgets.activity_meter.time.monotonic",
            return_value=100.0,
        ):
            meter.clear()
            meter.set_value(0.9)
        self.assertAlmostEqual(meter.level_db, -6)
        self.assertAlmostEqual(meter.held_db, -6)
        for now in (100.5, 101.0, 101.5):
            with patch(
                "biglinux_microphone.ui.widgets.activity_meter.time.monotonic",
                return_value=now,
            ):
                meter.set_value(0.1)
        self.assertAlmostEqual(meter.level_db, -6 - 11.76 * 1.5)
        self.assertAlmostEqual(meter.held_db, -6)
        with patch(
            "biglinux_microphone.ui.widgets.activity_meter.time.monotonic",
            return_value=121.0,
        ):
            meter.set_value(0.1)
        self.assertAlmostEqual(meter.held_db, -54)
        view.navigate("calibrate")
        self.assertEqual(meter.level_db, -60)
        self.assertEqual(meter.held_db, -60)
        self.assertEqual(meter._tick_id, 0)

    def test_closed_view_cannot_restart_monitoring(self) -> None:
        view = self._view()
        view.shutdown()
        with patch.object(view, "_start_monitor") as start:
            view.set_monitor(True)
        start.assert_not_called()

    def test_microphone_tiles_survive_unrelated_updates(self) -> None:
        view = self._view()
        controls = view._controls
        tile = controls._source_tiles["mic"]
        self.assertTrue(tile.get_active())
        controls.sync(replace(view._state.snapshot, volume=72))
        self.assertIs(controls._source_tiles["mic"], tile)

    def test_styles_are_installed_once_per_display(self) -> None:
        from biglinux_microphone.ui import style

        display = Gdk.Display.get_default()
        style.install(display)
        provider = style._providers[display]
        style.install(display)
        self.assertIs(style._providers[display], provider)

    def test_closed_state_skips_debounce(self) -> None:
        view = self._view()
        view._state.shutdown()
        view._state.set_debounced("late", "value")
        self.assertNotIn("late", view._state._debouncers)

    def test_calibration_keeps_restore_volume(self) -> None:

        view = self._view()
        page = view._ensure_page("calibrate")
        page._running = True
        page.sync(replace(view._state.snapshot, volume=280))
        self.assertEqual(page._original_volume, 125)
        with patch.object(page, "cancel") as cancel:
            page.sync(replace(view._state.snapshot, current_source="other", volume=12))
        cancel.assert_called_once_with(navigate=False)
        self.assertEqual(page._source, "mic")
        self.assertEqual(page._original_volume, 125)
        page._running = False

    def test_stale_calibration_completion_is_ignored(self) -> None:
        from unittest.mock import Mock

        view = self._view()
        page = view._ensure_page("calibrate")
        old, current = Mock(), Mock()
        page._session = current
        page._running = True
        page._finished({"heard_speech": True}, old)
        page._failed("old error", old)
        self.assertTrue(page._running)
        self.assertIs(page._session, current)
        page._running = False
        page._session = None

    def test_start_calibration_is_disabled_without_microphone(self) -> None:
        view = self._view(sources=[], current_source="")
        page = view._ensure_page("calibrate")
        self.assertFalse(page._start_button.get_sensitive())
        with patch.object(cli, "start_calibration") as start:
            page._start()
        start.assert_not_called()

    def test_diagnostics_needs_observed_audio(self) -> None:
        view = self._view()
        page = view._ensure_page("diagnostics")
        self.assertIn("listen back", page._status.get_title())
        self.assertFalse(page._rows["microphone"].get_use_markup())

    def test_diagnostics_rejects_bad_latency(self) -> None:
        view = self._view()
        page = view._ensure_page("diagnostics")
        calls = []
        page.ctx.state.call = lambda fn, done, error: calls.append(done)
        page.on_shown()
        calls[-1]({"total_ms": 5})
        self.assertEqual(page._rows["latency"].get_subtitle(), "5.0 ms")
        page.on_shown()
        old = calls[-1]
        page.on_shown()
        old({"total_ms": 99})
        self.assertEqual(page._rows["latency"].get_subtitle(), "Unknown")
        for value in (float("nan"), float("inf"), -1, True, "5"):
            calls[-1]({"total_ms": value})
            self.assertEqual(page._rows["latency"].get_subtitle(), "Unknown")
        page.on_hidden()
        calls[-1]({"total_ms": 88})
        self.assertEqual(page._rows["latency"].get_subtitle(), "Unknown")

    def test_microphone_choice_is_serial(self) -> None:
        view = self._view(
            sources=[
                {"name": "mic", "description": "Test Mic"},
                {"name": "other", "description": "Other Mic"},
            ]
        )
        controls = view._controls
        calls = []
        controls.ctx.state.call = lambda fn, done, error: calls.append((done, error))
        controls.ctx.state.reload = lambda: None
        controls._source_tiles["other"].set_active(True)
        controls._choose(controls._source_tiles["other"], "other")
        self.assertEqual(len(calls), 1)
        self.assertFalse(controls._sources.get_sensitive())
        calls[0][0](None)
        self.assertTrue(controls._sources.get_sensitive())
        self.assertEqual(view._visible_id(), "home")

    def test_microphone_notice_opens_the_microphone_card(self) -> None:
        view = self._view()
        view.navigate("source_picker")
        self.assertEqual(view._visible_id(), "home")
        self.assertIs(view._controls.cards.hover_cards.pinned, view._controls.source)

    def test_load_failure_shows_in_diagnostics(self) -> None:

        view = self._view()
        page = view._ensure_page("diagnostics")
        page.sync(
            replace(view._state.snapshot, loaded=False, error="Backend unavailable")
        )
        self.assertEqual(page._status.get_title(), "Backend unavailable")
        calls = []
        page.ctx.state.call = lambda *args: calls.append(args)
        page.on_shown()
        self.assertEqual(calls, [])


if __name__ == "__main__":
    unittest.main()
