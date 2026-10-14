"""Pure scheduling contracts; replace GLib calls, not GTK widget behavior."""

from unittest.mock import patch

from biglinux_microphone.ui.backend import BackendRunner
from biglinux_microphone.ui.state import AudioState
from biglinux_microphone.utils import async_utils as au


def test_idle_delivery_is_one_shot_even_when_callback_returns_true():
    seen = []
    assert au._deliver_once(lambda x: seen.append(x) or True, 42) is False
    assert seen == [42]


def test_new_immediate_throttle_removes_an_obsolete_timer():
    seen = []
    throttle = au.Throttler(100)
    with (
        patch.object(au.time, "monotonic", side_effect=[1.0, 1.02, 1.2]),
        patch.object(au.GLib, "timeout_add", return_value=7) as add,
        patch.object(au.GLib, "source_remove") as remove,
    ):
        throttle.call(seen.append, "first")
        throttle.call(seen.append, "obsolete")
        throttle.call(seen.append, "latest")
    assert seen == ["first", "latest"]
    add.assert_called_once()
    remove.assert_called_once_with(7)
    assert throttle._pending_call is None
    assert throttle._timer_id is None


def test_debouncer_cancel_discards_the_pending_value():
    seen = []
    debounce = au.Debouncer(100)
    with (
        patch.object(au.GLib, "timeout_add", return_value=8),
        patch.object(au.GLib, "source_remove") as remove,
    ):
        debounce.call(seen.append, "old")
        debounce.cancel()
        debounce._execute()
    assert seen == []
    remove.assert_called_once_with(8)


class QueueRunner:
    def __init__(self):
        self.calls = []

    def call(self, action, on_done=None, on_error=None):
        self.calls.append((action, on_done, on_error))


def test_slider_burst_keeps_only_the_latest_pending_value():
    runner = QueueRunner()
    state = AudioState(runner)
    with (
        patch.object(state, "_after_write"),
        patch("biglinux_microphone.cli.set_value") as write,
    ):
        for n in range(1000):
            state.set("strength", str(n))
        assert len(runner.calls) == 1
        assert state._queued_values == {"strength": "999"}
        action, done, _ = runner.calls[0]
        action()
        done(None)
        assert len(runner.calls) == 2
        action, done, _ = runner.calls[1]
        action()
        done(None)
        assert write.call_args_list[0].args == ("strength", "0")
        assert write.call_args_list[1].args == ("strength", "999")
        assert not state._queued_values and not state._writing
    state.shutdown()


def test_shutdown_drops_pending_write_and_ignores_its_ack():
    runner = QueueRunner()
    state = AudioState(runner)
    state.set("echo", "off")
    state.set("echo", "on")
    state.shutdown()
    runner.calls[0][1](None)
    assert len(runner.calls) == 1
    assert not state._queued_values


def test_failed_write_does_not_starve_a_newer_request():
    runner = QueueRunner()
    state = AudioState(runner)
    with patch.object(state, "_write_failed") as failed:
        state.set("echo", "off")
        state.set("echo", "on")
        runner.calls[0][2]("denied")
        assert len(runner.calls) == 2
        failed.assert_called_once_with("denied")
    state.shutdown()


def test_backend_dispatch_ignores_callbacks_after_shutdown():
    runner = BackendRunner(lambda _: None)
    callbacks = []
    seen = []
    with patch(
        "biglinux_microphone.ui.backend.GLib.idle_add", side_effect=callbacks.append
    ):
        runner.dispatch(seen.append, 1)
    runner.shutdown()
    assert callbacks[0]() is False
    assert seen == []


def test_slow_task_does_not_block_a_device_refresh():
    import threading

    from biglinux_microphone.ui.state import Snapshot

    release = threading.Event()
    read = threading.Event()
    runner = BackendRunner(lambda _: None)
    state = AudioState(runner)
    try:
        state.call(lambda: release.wait(3))
        with patch.object(
            state, "_read", side_effect=lambda: (read.set(), Snapshot())[1]
        ):
            state.reload()
            assert read.wait(1), "device refresh waited for recording/calibration"
    finally:
        release.set()
        state.shutdown()
        runner.shutdown()


def test_meter_drops_queued_frames_after_source_change_or_pause():
    from biglinux_microphone.ui.audio_capture import SpectrumTap

    frames = []
    callbacks = []
    tap = SpectrumTap(lambda *frame: frames.append(frame))
    with patch(
        "biglinux_microphone.ui.audio_capture.GLib.idle_add",
        side_effect=callbacks.append,
    ):
        tap.configure("mic", ("first",))
        tap._publish(tap._generation, [], 0.7)
        tap.configure("mic", ("replacement",))
        callbacks.pop()()
        assert not frames
        tap._publish(tap._generation, [], 0.3)
        tap.configure()
        callbacks.pop()()
        assert not frames
        tap.configure("mic", ("replacement",))
        for level in (0.2, 0.4, 0.6):
            tap._publish(tap._generation, [], level)
        assert len(callbacks) == 1
        callbacks.pop()()
        assert frames == [([], 0.6)]
        tap._publish(tap._generation, [], 0.95)
        tap._publish(tap._generation, [], 0.1)
        callbacks.pop()()
        assert frames[-1] == ([], 0.95), "a queued transient peak was lost"
    tap.stop()


def test_spectrum_uses_waveform_peak_and_preserves_frequency_scale(tmp_path):
    import subprocess

    import numpy as np
    from biglinux_microphone.ui.audio_capture import SpectrumTap

    real_popen = subprocess.Popen
    tone = np.sin(2 * np.pi * 750 * np.arange(4096) / 48000)
    expected = [-40, -20, -9, -0.1]
    pcm = np.concatenate(
        [tone * 10 ** (db / 20) for db in expected]
        + [np.full(4096, 0.2), np.full(4096, 0.2), np.zeros(4096), np.zeros(4096)]
    )
    path = tmp_path / "signal.f32"
    pcm.astype("float32").tofile(path)
    frames = []
    tap = SpectrumTap(lambda *_: None)
    tap.configure("fixture", ("fixture",), detailed=True)
    with (
        patch(
            "biglinux_microphone.ui.audio_capture.subprocess.Popen",
            side_effect=lambda _argv, **kw: real_popen(["cat", str(path)], **kw),
        ),
        patch.object(
            tap,
            "_publish",
            side_effect=lambda _g, bands, peak: frames.append((bands, peak)),
        ),
    ):
        tap._capture("pw-cat", np, "fixture", True, tap._generation)
    assert len(frames) == 8
    for (bands, peak), db in zip(frames, expected):
        assert abs((-60 + peak * 60) - db) < 0.001
        assert np.argmax(bands) == 13, "750 Hz moved to a different frequency band"
    assert abs((-60 + frames[5][1] * 60) - 20 * np.log10(0.2)) < 0.001
    assert max(frames[4][0]) == 0, "DC appeared as audible frequency energy"
    assert frames[7] == ([0.0] * 30, 0.0)


def test_meter_pause_reaps_capture_and_level_only_mode_never_runs_an_fft():
    import subprocess
    import sys
    import threading

    import numpy as np
    from biglinux_microphone.ui.audio_capture import SpectrumTap

    started = threading.Event()
    frame = threading.Event()
    processes = []
    real_popen = subprocess.Popen
    writer = "import os,struct,time\nb=struct.pack('<4096f', *([0.1,-0.1]*2048))\nwhile True:\n os.write(1,b);time.sleep(.085)"

    def start(_argv, **kwargs):
        proc = real_popen([sys.executable, "-c", writer], **kwargs)
        processes.append(proc)
        started.set()
        return proc

    tap = SpectrumTap(lambda *_: None)
    with (
        patch(
            "biglinux_microphone.ui.audio_capture.shutil.which", return_value="pw-cat"
        ),
        patch(
            "biglinux_microphone.ui.audio_capture.subprocess.Popen", side_effect=start
        ),
        patch(
            "biglinux_microphone.ui.audio_capture.GLib.idle_add",
            side_effect=lambda _: frame.set(),
        ),
        patch.object(
            np.fft,
            "rfft",
            side_effect=AssertionError("level-only capture performed FFT"),
        ),
    ):
        try:
            tap.start()
            assert not started.wait(0.1), "an invisible meter opened the microphone"
            tap.configure("physical-mic", ("1",))
            assert started.wait(2)
            assert frame.wait(2)
            tap.configure()
            processes[0].wait(timeout=2)
            assert len(processes) == 1
        finally:
            tap.stop()
            tap._thread.join(timeout=3)
            assert not tap._thread.is_alive()
            for proc in processes:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait()
