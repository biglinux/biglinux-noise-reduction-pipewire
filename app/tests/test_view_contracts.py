"""Headless GTK contracts for the beginner-first microphone UI.

These tests deliberately stub the Rust backend.  They verify the navigation and
progressive-disclosure contract without touching PipeWire or the user's audio.
"""

from __future__ import annotations

import unittest
from dataclasses import replace
from unittest.mock import patch

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from biglinux_microphone import cli
from biglinux_microphone.ui.view import _PAGES, _TASKS, MicrophoneView
from gi.repository import Adw, Gdk, GLib


def _settings(
    *, model: int = 13, calibrated: bool = False, **noise_reduction
) -> dict:
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

    def test_pages_match_the_new_information_architecture(self) -> None:
        view = self._view()
        self.assertEqual([page.id for page in view._pages], ["home"])
        for factory in (*_PAGES, *_TASKS):
            view._ensure_page(factory.id)
        self.assertEqual(
            [page.id for page in view._pages],
            [
                "home",
                "noise_reduction",
                "voice_volume",
                "voice_sound",
                "echo_routing",
                "mic_test",
                "diagnostics",
                "source_picker",
                "calibrate",
            ],
        )
        self.assertEqual(view._visible_id(), "home")

    def test_home_and_settings_share_the_same_persistent_sidebar(self) -> None:
        view = self._view()
        self.assertTrue(view._home.in_sidebar)
        self.assertEqual(
            list(view._sidebar_rows),
            [
                "home",
                "noise_reduction",
                "voice_volume",
                "voice_sound",
                "echo_routing",
                "mic_test",
                "diagnostics",
            ],
        )
        view.navigate("voice_sound")
        self.assertEqual(view._nav.get_visible_page().get_tag(), "pages")
        self.assertEqual(view._stack.get_visible_child_name(), "voice_sound")
        view.navigate("home")
        self.assertEqual(view._visible_id(), "home")

    def test_clicking_a_sidebar_row_changes_the_page(self) -> None:
        """Every other test navigates by calling `navigate`, which is not the
        path a person uses. A Gtk.ListBox only emits `row-activated` for rows
        that are activatable, and Adw.ActionRow is not one by default: the
        sidebar highlighted the row that was clicked and left the previous page
        on screen, which made the whole advanced half unreachable by mouse."""
        view = self._view()
        view.navigate("noise_reduction")
        # `get_activatable` is the half a click depends on and the half that was
        # false; emitting row-activated covers the handler behind it.
        for page_id in ("voice_volume", "diagnostics", "echo_routing"):
            row = view._sidebar_rows[page_id]
            self.assertTrue(row.get_activatable(), f"{page_id} row cannot be clicked")
            view._sidebar_list.emit("row-activated", row)
            self.assertEqual(view._stack.get_visible_child_name(), page_id)
        home = view._sidebar_rows["home"]
        self.assertTrue(home.get_activatable())
        view._sidebar_list.emit("row-activated", home)
        self.assertEqual(view._visible_id(), "home")

    def test_home_renders_beginner_state_without_technical_model_name(self) -> None:
        view = self._view(_settings(model=13, calibrated=False))
        home = view._home
        self.assertTrue(home._noise.get_active())
        self.assertIn("reducing background noise", home._noise.get_subtitle())
        self.assertNotIn("DPDF", home._noise.get_subtitle())
        self.assertIn("Not adjusted", home._volume.get_subtitle())

    def test_home_explains_when_no_microphone_is_available(self) -> None:
        view = self._view(sources=[], current_source="")
        home = view._home
        self.assertTrue(home._source_notice.get_revealed())
        self.assertEqual(home._source_notice.get_title(), "No microphone available")
        self.assertFalse(home._primary.get_sensitive())
        self.assertFalse(home._volume_scale.get_sensitive())

    def test_home_test_action_stays_available_after_calibration(self) -> None:
        view = self._view(_settings(calibrated=True))
        home = view._home
        self.assertIn("Adjusted", home._volume.get_subtitle())
        self.assertTrue(home._adjust_again.get_visible())
        child = home._primary.get_child()
        label = child.get_last_child()
        self.assertEqual(label.get_text(), "Test microphone")

    def test_model_choices_write_the_model(self) -> None:
        view = self._view(_settings(model=13))
        page = view._ensure_page("noise_reduction")
        writes = []
        page.ctx.state.set = lambda key, value: writes.append((key, value))
        page._model_buttons[12].set_active(True)
        self.assertEqual(writes, [("model", "dfn3-ll")])

    def test_model_specific_controls_follow_effective_model(self) -> None:
        dpdf = self._view(_settings(model=13))
        page = dpdf._ensure_page("noise_reduction")
        self.assertFalse(page._residual.get_visible())
        self.assertTrue(page._dpdf_info.get_revealed())

        dfn = self._view(_settings(model=11))
        page = dfn._ensure_page("noise_reduction")
        self.assertTrue(page._residual.get_visible())
        self.assertFalse(page._dpdf_info.get_revealed())

    def test_each_model_keeps_its_voice_gate_choice(self) -> None:
        for model, voice_gate in ((11, True), (12, False), (13, True)):
            with self.subTest(model=model):
                page = self._view(
                    _settings(
                        model=model,
                        voice_gate=voice_gate,
                        dpdfnet_voice_gate=voice_gate,
                    )
                )._ensure_page("noise_reduction")
                self.assertTrue(page._voice_gate.get_sensitive())
                self.assertEqual(page._voice_gate.get_active(), voice_gate)
                writes = []
                page.ctx.state.set = lambda key, value, writes=writes: writes.append(
                    (key, value)
                )
                page._voice_gate.set_active(not voice_gate)
                self.assertEqual(
                    writes, [("voice-gate", "off" if voice_gate else "on")]
                )
        # DPDFNet's gate is off until asked for, whatever DFN3's choice.
        page = self._view(_settings(model=13, voice_gate=True))._ensure_page(
            "noise_reduction"
        )
        self.assertTrue(page._voice_gate.get_sensitive())
        self.assertFalse(page._voice_gate.get_active())
        self.assertFalse(page._sensitivity_row.get_sensitive())

    def test_voice_detection_follows_the_voice_gate(self) -> None:
        page = self._view(_settings(model=11))._ensure_page("noise_reduction")
        self.assertTrue(page._sensitivity_row.get_sensitive())
        self.assertEqual(page._voice_sensitivity.get_selected(), 2)
        writes = []
        page.ctx.state.set = lambda key, value: writes.append((key, value))
        page._voice_sensitivity.set_selected(4)
        self.assertEqual(writes, [("voice-sensitivity", "5")])

        page = self._view(
            _settings(model=12, voice_gate=False, voice_sensitivity=1)
        )._ensure_page("noise_reduction")
        self.assertTrue(page._sensitivity_row.get_visible())
        self.assertFalse(page._sensitivity_row.get_sensitive())
        self.assertEqual(page._voice_sensitivity.get_selected(), 0)

    def test_manual_tuning_is_shown_and_restores_only_what_changed(self) -> None:
        page = self._view(_settings(model=11))._ensure_page("noise_reduction")
        self.assertFalse(page._manual.get_visible())
        page = self._view(
            _settings(model=11, min_snr_override_db=-18.0, startup_mute_ms=1000)
        )._ensure_page("noise_reduction")
        self.assertTrue(page._manual.get_visible())
        writes = []
        page.ctx.state.set = lambda key, value: writes.append((key, value))
        page._restore_tuning(None)
        self.assertEqual(writes, [("dfn-snr-gate", "auto")])
        dpdf = self._view(
            _settings(model=13, min_snr_override_db=-18.0)
        )._ensure_page("noise_reduction")
        self.assertFalse(dpdf._manual.get_visible())

    def test_home_controls_write_only_after_an_explicit_change(self) -> None:
        view = self._view(_settings())
        home = view._home
        with (
            patch.object(view._state, "set") as setting,
            patch.object(view._state, "set_volume") as volume,
        ):
            home.sync(view._state.snapshot)
            setting.assert_not_called()
            volume.assert_not_called()
            home._mode.set_selected(1)
            setting.assert_called_once_with("model", "dfn3-ll")
            home._volume_scale.set_value(80)
            self.assertTrue(home._volume_debounce.pending)
            GLib.source_remove(home._volume_debounce._timer_id)
            home._volume_debounce._execute()
            volume.assert_called_once_with("mic", 80)

    def test_home_cancels_pending_volume_on_device_change_and_navigation(self) -> None:
        view = self._view()
        home = view._home
        home.on_peak(0.9)
        home._volume_scale.set_value(70)
        home.sync(replace(view._state.snapshot, current_source="other"))
        self.assertFalse(home._volume_debounce.pending)
        self.assertEqual(home._meter.held_db, -60)
        home._volume_scale.set_value(80)
        view.navigate("voice_sound")
        self.assertFalse(home._volume_debounce.pending)

    def test_calibration_waits_for_explicit_start(self) -> None:
        view = self._view()
        page = view._ensure_page("calibrate")
        view.navigate("calibrate")
        self.assertEqual(view._nav.get_visible_page().get_tag(), "calibrate")
        self.assertEqual(page._stack.get_visible_child_name(), "prepare")
        self.assertFalse(page._running)

    def test_link_from_task_to_advanced_brings_advanced_page_to_front(self) -> None:
        view = self._view()
        view.navigate("voice_volume")
        view.navigate("calibrate")
        self.assertEqual(view._nav.get_visible_page().get_tag(), "calibrate")
        view.navigate("voice_volume")
        self.assertEqual(view._nav.get_visible_page().get_tag(), "pages")
        self.assertEqual(view._stack.get_visible_child_name(), "voice_volume")

    def test_calibration_result_never_exposes_raw_gain_percent(self) -> None:
        view = self._view()
        page = view._ensure_page("calibrate")
        page._finished({"heard_speech": True, "volume": 1.71, "device": "mic"})
        self.assertEqual(page._stack.get_visible_child_name(), "result")
        # The success flow deliberately has no percentage/numeric result label.
        self.assertFalse(hasattr(page, "_result_percent"))

    def test_automatic_volume_restores_saved_limits_without_writing(self) -> None:
        settings = _settings(calibrated=True)
        settings["mic_calibration"]["devices"]["mic"].update(
            min_volume=0.8, max_volume=1.2
        )
        view = self._view(settings)
        page = view._ensure_page("voice_volume")
        self.assertFalse(view._home._volume_row.get_visible())
        self.assertTrue(view._home._automatic.get_visible())
        with patch.object(view._state, "set_level_range") as write:
            page.sync(view._state.snapshot)
            write.assert_not_called()
            self.assertTrue(page._automatic.get_active())
            self.assertEqual(page._low.get_value(), 80)
            self.assertEqual(page._high.get_value(), 120)
            page._low.set_value(130)
            self.assertFalse(page._apply.get_sensitive())
            page._high.set_value(150)
            write.assert_not_called()
            page._apply.emit("clicked")
            write.assert_called_once_with("mic", (130, 150))
            page._automatic.set_active(False)
            self.assertEqual(write.call_args.args, ("mic", None))

    def test_automatic_volume_can_be_enabled_without_previous_calibration(self) -> None:
        view = self._view()
        page = view._ensure_page("voice_volume")
        with patch.object(view._state, "set_level_range") as write:
            page._automatic.set_active(True)
        write.assert_called_once_with("mic", (105, 145))

    def test_equalizer_is_revealed_only_for_custom(self) -> None:
        view = self._view(_settings())
        page = view._ensure_page("voice_sound")
        self.assertFalse(page._custom_reveal.get_reveal_child())
        writes = []
        page.ctx.state.set = lambda key, value: writes.append((key, value))
        page._preset_buttons["custom"].set_active(True)
        self.assertTrue(page._custom_reveal.get_reveal_child())
        self.assertIn(("mic-equalizer", "on"), writes)

    def test_repeated_monitor_enable_does_not_spawn_duplicate_processes(self) -> None:
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

    def test_without_a_comparison_the_test_records_what_applications_hear(self) -> None:
        view = self._view(_settings())
        page = view._ensure_page("mic_test")
        self.assertTrue(page.in_sidebar)
        self.assertFalse(page._comparing)
        self.assertEqual(page._target, "mic-biglinux")
        self.assertTrue(page._record_button.get_sensitive())
        self.assertEqual(page._intro_page.get_title(), "Record a short sample")

    def test_listen_back_switch_follows_the_shell_monitor(self) -> None:
        view = self._view()
        self.assertTrue(view._listen_back.get_sensitive())
        with patch.object(view, "_start_monitor") as start:
            view._listen_back.set_active(True)
            start.assert_called_once()
        self.assertTrue(view.monitor_active)
        view._monitor_failed(view._monitor_generation, "no loader")
        self.assertFalse(view._listen_back.get_active())

    def test_primary_action_matches_label_when_reduction_is_off(self) -> None:
        settings = _settings(calibrated=True)
        settings["noise_reduction"]["enabled"] = False
        view = self._view(settings)
        with patch.object(view._home.ctx, "navigate") as navigate:
            view._home._primary_clicked(view._home._primary)
        self.assertEqual(
            view._home._primary.get_child().get_last_child().get_text(),
            "Test microphone",
        )
        navigate.assert_called_once_with("mic_test")

    def test_the_test_requires_an_available_microphone(self) -> None:
        view = self._view(_settings(), sources=[], current_source="")
        with patch.object(view._home.ctx, "navigate") as navigate:
            view._home._primary_clicked(view._home._primary)
        navigate.assert_not_called()

    def test_both_pages_report_the_saved_model(self) -> None:
        for index, model in enumerate((11, 12, 13)):
            with self.subTest(model=model):
                view = self._view(_settings(model=model))
                self.assertEqual(view._home._mode.get_selected(), index)
                self.assertTrue(
                    view._ensure_page("noise_reduction")
                    ._model_buttons[model]
                    .get_active()
                )

    def test_sidebar_roundtrip_does_not_hide_the_desktop_sidebar(self) -> None:
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

    def test_only_the_visible_page_receives_meter_frames(self) -> None:
        view = self._view()
        with (
            patch.object(view._home, "on_bands") as home,
            patch.object(view._ensure_page("noise_reduction"), "on_bands") as hidden,
        ):
            view._on_spectrum_frame([0.2] * 30, 0.5)
        home.assert_called_once()
        hidden.assert_not_called()

    def test_peak_meter_has_instant_attack_timed_decay_and_hold(self) -> None:
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
        view.navigate("voice_sound")
        self.assertEqual(meter.level_db, -60)
        self.assertEqual(meter.held_db, -60)
        self.assertEqual(meter._tick_id, 0)

    def test_home_spectrum_capture_stops_when_hidden(self) -> None:
        view = self._view()
        with (
            patch.object(view, "get_mapped", return_value=True),
            patch.object(view._spectrum_tap, "configure") as capture,
        ):
            view._sync_capture()
            self.assertTrue(capture.call_args.kwargs["detailed"])
            view.navigate("voice_sound")
            capture.assert_called_with()
        with (
            patch.object(view, "get_mapped", return_value=False),
            patch.object(view._spectrum_tap, "configure") as capture,
        ):
            view.navigate("home")
            capture.assert_called_with()

    def test_closed_view_cannot_restart_monitoring(self) -> None:
        view = self._view()
        view.shutdown()
        with patch.object(view, "_start_monitor") as start:
            view.set_monitor(True)
        start.assert_not_called()

    def test_source_picker_preserves_rows_on_unrelated_settings_updates(self) -> None:
        from dataclasses import replace

        view = self._view()
        page = view._ensure_page("source_picker")
        group = page._groups.get_first_child()
        page.sync(replace(view._state.snapshot, volume=72))
        self.assertIs(page._groups.get_first_child(), group)

    def test_device_description_is_literal_text(self) -> None:
        view = self._view(
            sources=[{"name": "mic", "description": "USB <b>Mic</b> & monitor"}]
        )
        self.assertFalse(view._home._source.get_use_markup())
        self.assertEqual(view._home._source.get_subtitle(), "USB <b>Mic</b> & monitor")

    def test_styles_are_installed_once_per_display(self) -> None:
        from biglinux_microphone.ui import style

        display = Gdk.Display.get_default()
        style.install(display)
        provider = style._providers[display]
        style.install(display)
        self.assertIs(style._providers[display], provider)

    def test_advanced_editors_are_lazy_and_detached_when_hidden(self) -> None:
        from dataclasses import replace

        view = self._view()
        sound, noise = (
            view._ensure_page("voice_sound"),
            view._ensure_page("noise_reduction"),
        )
        self.assertIsNone(sound._eq_group)
        self.assertIsNone(noise._editor)
        settings = _settings()
        settings["equalizer"] = {
            "preset": "custom",
            "enabled": True,
            "bands": [2.0] * 10,
        }
        sound.sync(replace(view._state.snapshot, settings=settings))
        self.assertEqual(len(sound._scales), 10)
        self.assertIs(sound._custom_reveal.get_child(), sound._eq_group)
        sound.sync(view._state.snapshot)
        self.assertIsNone(sound._custom_reveal.get_child())
        settings = _settings(model=11)
        settings["noise_reduction"]["silence_curve_db"] = [-45.0] * 10
        noise.sync(
            replace(
                view._state.snapshot,
                settings=settings,
                runtime={**view._state.snapshot.runtime, "effective_model": 11},
            )
        )
        self.assertIsNotNone(noise._editor_reveal.get_child())
        noise.sync(view._state.snapshot)
        self.assertIsNone(noise._editor_reveal.get_child())

    def test_preset_discards_unsent_equalizer_values(self) -> None:
        view = self._view()
        page = view._ensure_page("voice_sound")
        view._state.set_debounced("mic-eq-bands", "obsolete", 10000)
        view._state._queued_values["mic-eq-bands"] = "also obsolete"
        with patch.object(view._state, "set") as write:
            page._preset_buttons["clarity"].set_active(True)
        self.assertFalse(view._state.is_pending("mic-eq-bands"))
        self.assertNotIn("mic-eq-bands", view._state._queued_values)
        self.assertEqual(write.call_args_list[0].args, ("mic-eq-preset", "presence"))

    def test_equalizer_snapshot_does_not_replace_pending_drag(self) -> None:
        view = self._view()
        page = view._ensure_page("voice_sound")
        page._show_custom(True)
        page._loading = False
        page._scales[0].set_value(7)
        page.sync(view._state.snapshot)
        self.assertEqual(page._scales[0].get_value(), 7)
        self.assertIsNotNone(page._custom_reveal.get_child())

    def test_closed_state_does_not_create_debounce_sources(self) -> None:
        view = self._view()
        view._state.shutdown()
        view._state.set_debounced("late", "value")
        self.assertNotIn("late", view._state._debouncers)

    def test_page_lifecycle_runs_only_on_real_transitions(self) -> None:
        view = self._view()
        view.navigate("mic_test")
        page = view._ensure_page("mic_test")
        unrelated = view._ensure_page("calibrate")
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

    def test_stale_recording_and_playback_callbacks_cannot_replace_new_session(
        self,
    ) -> None:
        import tempfile
        from pathlib import Path
        from unittest.mock import Mock

        view = self._view()
        page = view._ensure_page("mic_test")
        old, current = Mock(), Mock()
        page._recording = current
        page._playback = current
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "stale.wav"
            path.write_bytes(b"old sample")
            page._recorded(path, old)
            self.assertFalse(path.exists())
        page._record_failed("obsolete failure", old)
        page._playback_done(None, old)
        with patch.object(page.ctx, "toast") as toast:
            page._playback_failed("obsolete failure", old)
        toast.assert_not_called()
        self.assertIs(page._recording, current)
        self.assertIs(page._playback, current)
        # No fake subprocesses left for teardown.
        page._recording = page._playback = None

    def test_calibration_keeps_original_restore_volume_while_running(self) -> None:
        from dataclasses import replace

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

    def test_diagnostics_does_not_certify_audio_from_settings(self) -> None:
        view = self._view()
        page = view._ensure_page("diagnostics")
        self.assertIn("test your microphone", page._status.get_title())
        self.assertFalse(page._rows["microphone"].get_use_markup())

    def test_diagnostics_rejects_hidden_stale_and_nonfinite_latency(self) -> None:
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

    def test_source_selection_is_serial_and_does_not_steal_navigation(self) -> None:
        view = self._view(
            sources=[
                {"name": "mic", "description": "Test Mic"},
                {"name": "other", "description": "Other Mic"},
            ]
        )
        page = view._ensure_page("source_picker")
        calls, navigations = [], []
        page.ctx.state.call = lambda fn, done, error: calls.append((done, error))
        page.ctx.state.reload = lambda: None
        page.ctx.navigate = navigations.append
        page.on_shown()
        page._choose(None, "other")
        page._choose(None, "other")
        self.assertEqual(len(calls), 1)
        self.assertFalse(page._groups.get_sensitive())
        page.on_hidden()
        calls[0][0](None)
        self.assertEqual(navigations, [])
        self.assertTrue(page._groups.get_sensitive())
        page.on_shown()
        page._choose(None, "other")
        calls[-1][0](None)
        self.assertEqual(navigations, ["home"])

    def test_equalizer_reflow_keeps_two_balanced_five_band_groups(self) -> None:
        settings = _settings()
        settings["equalizer"] = {
            "enabled": True,
            "preset": "custom",
            "bands": [0.0] * 10,
        }
        view = self._view(settings)
        page = view._ensure_page("voice_sound")
        groups = {}
        for scale in page._scales:
            parent = scale.get_parent().get_parent()
            groups.setdefault(parent, []).append(scale)
        self.assertEqual(sorted(map(len, groups.values())), [5, 5])
        self.assertEqual(len(page._scales), 10)

    def test_first_load_failure_is_visible_in_diagnostics(self) -> None:
        from dataclasses import replace

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

    def test_volume_page_explains_missing_source(self) -> None:
        view = self._view(sources=[], current_source="")
        page = view._ensure_page("voice_volume")
        self.assertEqual(page._status.get_title(), "No microphone available")
        self.assertFalse(page._adjust.get_sensitive())

    def test_external_errors_are_literal_not_pango_markup(self) -> None:
        from dataclasses import replace
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
        microphone_test = view._ensure_page("mic_test")
        microphone_test._show_error(error)
        self.assertEqual(
            microphone_test._error_page.get_description(),
            GLib.markup_escape_text(error),
        )
        toasts = []
        window = SimpleNamespace(
            _toast_overlay=SimpleNamespace(add_toast=toasts.append)
        )
        MicrophoneWindow.show_toast(window, error)
        self.assertFalse(toasts[0].get_use_markup())
        self.assertEqual(toasts[0].get_title(), error)

    def test_filter_failure_is_visible_and_repair_is_an_explicit_action(self):
        from dataclasses import replace

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
            self.assertTrue(view._home._noise.get_active())
            self.assertIn("not ready", view._home._noise.get_subtitle())
            self.assertTrue(view._home._runtime_notice.get_revealed())
            self.assertFalse(
                view._ensure_page("mic_test")._record_button.get_sensitive()
            )
            repair.assert_not_called()
            view._home._runtime_notice.emit("button-clicked")
            repair.assert_called_once()

    def test_eq_only_test_uses_the_chain_and_recovery_clears_stale_meter_text(self):
        from dataclasses import replace

        settings = _settings()
        settings["noise_reduction"]["enabled"] = False
        settings["equalizer"]["enabled"] = True
        view = self._view(settings)
        self.assertEqual(view._ensure_page("mic_test")._target, "mic-biglinux")
        good = view._state.snapshot
        view._state._apply_read(replace(good, muted=True))
        self.assertEqual(view._home._meter_state.get_text(), "Microphone muted")
        self.assertFalse(view._ensure_page("mic_test")._record_button.get_sensitive())
        view._state._apply_read(good)
        self.assertEqual(
            view._home._meter_state.get_text(), "Speak to check your microphone"
        )
        self.assertTrue(view._ensure_page("mic_test")._record_button.get_sensitive())

    def test_comparison_uses_one_capture_and_cleans_both_versions_on_exit(self):
        from dataclasses import replace
        from pathlib import Path
        from unittest.mock import Mock

        view = self._view(_settings(model=11))
        expression = "ladspa=file=/usr/lib/ladspa/libdfn3_ladspa.so:plugin=deep_filter_net3_rs_mono:controls=c0=100.00"
        view._state._apply_read(
            replace(
                view._state.snapshot,
                runtime={
                    **view._state.snapshot.runtime,
                    "comparison_filter": expression,
                },
            )
        )
        view.navigate("mic_test")
        page = view._ensure_page("mic_test")
        self.assertTrue(page._comparing)
        session = Mock(path=Path("reduced.wav"), original_path=Path("original.wav"))
        with patch.object(
            cli, "SampleComparisonSession", return_value=session
        ) as capture:
            self.assertIs(page._new_recording(), session)
            capture.assert_called_once_with("mic", expression)
        page._recording = session
        page._recorded(session.path, session)
        self.assertEqual(page._sample_path, session.path)
        page._original.set_active(True)
        self.assertEqual(page._sample_path, session.original_path)
        page.on_hidden()
        session.cleanup.assert_called_once()
        self.assertIsNone(page._sample_path)


if __name__ == "__main__":
    unittest.main()
