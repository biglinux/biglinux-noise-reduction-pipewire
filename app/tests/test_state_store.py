"""The store's contract, without a display.

These are the three ways a shared state store goes wrong, and the window is
about to depend on all three being right on six pages at once:

* a write that failed is shown as if it had worked,
* two changes made quickly land in the wrong order, so the file keeps the older
  one,
* updating a widget to match the truth is mistaken for the person changing it,
  which writes it back, which updates the widget, forever.

No GTK widgets here: the store is plain Python precisely so this can run
anywhere. The runner is replaced by one that works synchronously, which is also
what makes the ordering assertion meaningful rather than a race.
"""

from __future__ import annotations

import unittest
from unittest.mock import patch

from biglinux_microphone import cli
from biglinux_microphone.ui.state import AudioState, Snapshot, calibration_window


class DirectRunner:
    """A BackendRunner that runs everything now, on this thread, in order."""

    def __init__(self) -> None:
        self.closed = False
        self.errors: list[str] = []

    def dispatch(self, callback, *args) -> None:
        callback(*args)

    def call(self, action, on_done=None, on_error=None) -> None:
        try:
            result = action()
        except Exception as exc:  # noqa: BLE001 - mirrors the runner's contract
            self.errors.append(str(exc))
            if on_error is not None:
                on_error(str(exc))
            return
        if on_done is not None:
            on_done(result)


def _settings(mode: str = "auto", enabled: bool = True) -> dict:
    return {
        "noise_reduction": {"enabled": enabled, "model": 11, "strength": 1.0},
        "equalizer": {"enabled": False, "bands": [], "preset": ""},
        "echo_cancel": {"mode": mode},
        "mic_calibration": {"devices": {"mic": {"volume": 1.4}}},
    }


class StoreContract(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = DirectRunner()
        self.state = AudioState(self.runner)
        self.stubs = patch.multiple(
            cli,
            load_status=lambda: {
                "settings": _settings(),
                "audio_available": True,
                "mic_enabled": True,
                "mic_running": True,
            },
            list_sources=lambda: [{"name": "mic", "description": "Test Mic"}],
            get_default_source=lambda: "mic",
            get_source_volume=lambda _name: 100,
            eq_presets=list,
        )
        self.stubs.start()
        self.addCleanup(self.stubs.stop)
        self.addCleanup(self.state.shutdown)

    def test_a_reading_reaches_every_subscriber(self) -> None:
        seen: list[Snapshot] = []
        self.state.subscribe(seen.append)
        self.state.subscribe(seen.append)
        self.state.reload()
        self.assertEqual(len(seen), 2)
        self.assertTrue(seen[0].loaded)
        self.assertEqual(seen[0].current_source, "mic")
        self.assertEqual(seen[0].echo_mode, "auto")
        self.assertTrue(seen[0].filter_on)
        self.assertEqual(seen[0].calibration()["volume"], 1.4)

    def test_volume_write_is_bound_to_the_selected_microphone(self) -> None:
        self.state.reload()
        with patch.object(cli, "set_source_volume") as write:
            self.state.set_volume("old-microphone", 80)
            write.assert_not_called()
            self.state.set_volume("mic", 80)
            write.assert_called_once_with("mic", 80)
            self.state.shutdown()
            self.state.set_volume("mic", 90)
            self.assertEqual(write.call_count, 1)

    def test_a_write_that_failed_does_not_change_what_is_shown(self) -> None:
        self.state.reload()
        before = self.state.snapshot
        self.assertEqual(before.echo_mode, "auto")

        def refuse(_key, _value):
            raise RuntimeError("the backend said no")

        with patch.object(cli, "set_value", side_effect=refuse):
            self.state.set("echo", "off")

        # The truth on screen is still the truth in the file, plus a reason.
        self.assertEqual(self.state.snapshot.echo_mode, "auto")
        self.assertIn("the backend said no", self.state.snapshot.error)

    def test_two_changes_apply_in_the_order_they_were_made(self) -> None:
        self.state.reload()
        calls: list[tuple[str, str]] = []
        with patch.object(
            cli, "set_value", side_effect=lambda k, v: calls.append((k, v))
        ):
            self.state.set("echo", "off")
            self.state.set("echo", "on")
        self.assertEqual(calls, [("echo", "off"), ("echo", "on")])

    def test_rendering_the_truth_is_not_mistaken_for_a_change(self) -> None:
        """A page that writes from inside its own sync would loop forever."""
        writes: list[tuple[str, str]] = []

        def page(snapshot: Snapshot) -> None:
            # Exactly what a widget handler does: stand down while syncing.
            if self.state.syncing:
                return
            writes.append(("echo", snapshot.echo_mode))

        self.state.subscribe(page)
        with patch.object(cli, "set_value", side_effect=lambda k, v: None):
            self.state.reload()
        self.assertEqual(writes, [])

    def test_a_failed_reading_keeps_the_last_good_one(self) -> None:
        self.state.reload()
        good = self.state.snapshot

        with patch.object(cli, "load_status", side_effect=RuntimeError("no backend")):
            self.state.reload()

        self.assertEqual(self.state.snapshot.current_source, good.current_source)
        self.assertTrue(self.state.snapshot.loaded)
        self.assertIn("no backend", self.state.snapshot.error)

    def test_a_subscriber_that_raises_does_not_silence_the_others(self) -> None:
        seen: list[Snapshot] = []

        def broken(_snapshot: Snapshot) -> None:
            raise ValueError("this page has a bug")

        self.state.subscribe(broken)
        self.state.subscribe(seen.append)
        self.state.reload()
        self.assertEqual(len(seen), 1)


class FilterState(unittest.TestCase):
    """Which flag says the microphone is being filtered."""

    def test_a_bypassed_chain_is_off_however_the_denoiser_is_set(self) -> None:
        # What the master switch writes is `mic_bypass`; `noise_reduction.enabled`
        # is the denoiser's own preference under it, and it survives the master
        # being turned off. Reading only the second one made the home page show
        # the filter lit over a microphone passing straight through.
        bypassed = Snapshot(
            settings={"mic_bypass": True, "noise_reduction": {"enabled": True}},
            loaded=True,
        )
        self.assertFalse(bypassed.filter_on)
        running = Snapshot(
            settings={"mic_bypass": False, "noise_reduction": {"enabled": True}},
            loaded=True,
        )
        self.assertTrue(running.filter_on)
        no_denoiser = Snapshot(
            settings={"mic_bypass": False, "noise_reduction": {"enabled": False}},
            loaded=True,
        )
        self.assertFalse(no_denoiser.filter_on)


class CalibrationWindow(unittest.TestCase):
    """What counts as "the level follows you between two places"."""

    def test_a_window_narrower_than_its_own_label_is_not_a_window(self) -> None:
        # Real numbers from a machine that measured the same spot twice: the
        # screen printed "between 184% and 184%" over a window 0.35% wide.
        self.assertIsNone(
            calibration_window({"min_volume": 1.8364770412445068, "max_volume": 1.84})
        )
        self.assertIsNone(calibration_window({"min_volume": 3.0, "max_volume": 3.0}))
        self.assertEqual(
            calibration_window({"min_volume": 0.8, "max_volume": 2.4}), (0.8, 2.4)
        )

    def test_half_a_window_is_no_window(self) -> None:
        self.assertIsNone(calibration_window({"volume": 1.2}))
        self.assertIsNone(calibration_window({"min_volume": 0.8, "max_volume": None}))
        self.assertIsNone(calibration_window({"min_volume": 2.4, "max_volume": 0.8}))


if __name__ == "__main__":
    unittest.main()


class ReadCoalescing(unittest.TestCase):
    def test_a_reload_burst_has_only_one_pending_successor(self):
        class QueueRunner:
            def __init__(self):
                self.calls = []

            def call(self, *args):
                self.calls.append(args)

        runner = QueueRunner()
        state = AudioState(runner)
        for _ in range(100):
            state.reload()
        self.assertEqual(len(runner.calls), 1)
        runner.calls[0][1](Snapshot(loaded=True))
        self.assertEqual(len(runner.calls), 2)
        runner.calls[1][1](Snapshot(loaded=True))
        self.assertEqual(len(runner.calls), 2)
        state.shutdown()
        state.reload()
        self.assertEqual(len(runner.calls), 2)

    def test_missing_default_does_not_fall_back_to_another_device(self):
        with patch.multiple(
            cli,
            load_status=lambda: {"settings": {}},
            list_sources=lambda: [{"name": "other"}],
            get_default_source=lambda: "removed",
            get_source_volume=lambda _: self.fail("wrong mic"),
            eq_presets=list,
        ):
            snapshot = AudioState._read()
        self.assertEqual(snapshot.current_source, "")
        self.assertIsNone(snapshot.volume)

    def test_nonfinite_out_of_range_and_boolean_calibrations_are_rejected(self):
        for low, high in [
            (float("nan"), 2),
            (0, float("inf")),
            (0, 4),
            (-1, 1),
            (True, 2),
        ]:
            self.assertIsNone(
                calibration_window({"min_volume": low, "max_volume": high})
            )


def test_capture_follows_the_effective_chain_and_never_falls_back():
    from dataclasses import replace

    snapshot = Snapshot(
        settings={
            "noise_reduction": {"enabled": False},
            "equalizer": {"enabled": True},
        },
        current_source="hardware",
        sources=[{"name": "hardware", "serial": "18"}],
        runtime={
            "audio_available": True,
            "mic_enabled": True,
            "mic_running": True,
            "denoising": False,
            "mic_serial": "42",
            "effective_model": 11,
        },
        loaded=True,
    )
    assert snapshot.capture_target == "mic-biglinux"
    assert snapshot.capture_identity == ("hardware", "18", "42")
    assert snapshot.model == 11
    changed = replace(
        snapshot, current_source="other", sources=[{"name": "other", "serial": "19"}]
    )
    assert changed.capture_target == snapshot.capture_target
    assert changed.capture_identity != snapshot.capture_identity
    for runtime in (
        {"audio_available": False},
        {"audio_available": True, "mic_enabled": True, "mic_running": False},
        {**snapshot.runtime, "denoising": True},
    ):
        failed = replace(snapshot, runtime=runtime)
        assert (
            not failed.ready
            and not failed.capture_target
            and not failed.capture_identity
        )
    bypassed = replace(
        snapshot, runtime={"audio_available": True, "mic_enabled": False}
    )
    assert bypassed.capture_target == "hardware" and bypassed.capture_identity == (
        "hardware",
        "18",
        "",
    )
    removed = replace(snapshot, current_source="")
    assert not removed.ready


def test_source_list_keeps_device_state_and_separates_virtual_inputs(monkeypatch):
    import json
    import subprocess

    rows = [
        {
            "name": "usb",
            "description": "(null)",
            "properties": {
                "device.api": "alsa",
                "node.description": "USB — external input",
                "object.serial": "12",
            },
            "mute": True,
            "volume": {"mono": {"value_percent": "75%"}},
            "active_port": "mic",
            "ports": [{"name": "mic", "availability": "not available"}],
        },
        {
            "name": "effects",
            "description": "Effects",
            "properties": {"node.virtual": "true"},
        },
        {"name": "unusual-loopback", "properties": {"device.class": "monitor"}},
        {"name": "mic-biglinux", "properties": {}},
    ]
    monkeypatch.setattr(cli, "_pactl", lambda: "pactl")
    monkeypatch.setattr(
        cli,
        "_command",
        lambda *_a, **_kw: subprocess.CompletedProcess([], 0, json.dumps(rows), ""),
    )
    sources = cli.list_sources()
    assert [s["name"] for s in sources] == ["usb", "effects"]
    assert sources[0] == {
        "name": "usb",
        "description": "USB — external input",
        "serial": "12",
        "muted": True,
        "volume": 75,
        "available": False,
        "virtual": False,
    }
    assert sources[1]["virtual"] is True
