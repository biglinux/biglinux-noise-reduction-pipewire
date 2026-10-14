import subprocess
import sys
import threading
import wave
from unittest.mock import Mock, patch

import pytest
from biglinux_microphone import cli
from biglinux_microphone.ui.pages.calibrate import CalibratePage


def test_microphone_sample_records_processed_target_and_private_wav(
    tmp_path, monkeypatch
):
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    proc = Mock()
    proc.communicate.return_value = ("", "")
    proc.returncode = 0
    with (
        patch.object(cli.shutil, "which", return_value="/usr/bin/pw-record"),
        patch.object(cli.subprocess, "Popen", return_value=proc) as popen,
    ):
        session = cli.start_microphone_sample("mic-biglinux", 5)
        with wave.open(str(session.path), "wb") as sample:
            sample.setparams((1, 2, cli.MIC_TEST_RATE, 0, "NONE", "not compressed"))
            sample.writeframes(b"\x00\x00" * cli.MIC_TEST_RATE * 5)
        assert session.wait() == session.path

    argv = popen.call_args.args[0]
    assert argv[0] == "/usr/bin/pw-record"
    assert "--target=mic-biglinux" in argv
    assert "--rate=48000" in argv
    assert "--channels=1" in argv
    assert "--sample-count=240000" in argv
    assert session.path.parent == tmp_path
    assert session.path.stat().st_mode & 0o777 == 0o600
    session.cleanup()
    assert not session.path.exists()


def test_microphone_sample_rejects_empty_or_failed_capture(tmp_path, monkeypatch):
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    proc = Mock()
    proc.communicate.return_value = ("", "target not found")
    proc.returncode = 1
    with (
        patch.object(cli.shutil, "which", return_value="/usr/bin/pw-record"),
        patch.object(cli.subprocess, "Popen", return_value=proc),
    ):
        session = cli.start_microphone_sample("missing", 5)
        with pytest.raises(cli.BackendError, match="target not found"):
            session.wait()
        assert not session.path.exists()


@pytest.mark.parametrize(
    "complete,diagnostic", [(True, False), (False, False), (True, True)]
)
def test_sample_limit_exit_requires_a_complete_wav_and_no_error(
    tmp_path, monkeypatch, complete, diagnostic
):
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    proc = Mock(returncode=1)
    with (
        patch.object(cli.shutil, "which", return_value="/usr/bin/pw-record"),
        patch.object(cli.subprocess, "Popen", return_value=proc),
    ):
        session = cli.start_microphone_sample("mic-biglinux", 1)
        proc.communicate.return_value = (
            "",
            "device disconnected" if diagnostic else str(session.path) + "\n",
        )
        with wave.open(str(session.path), "wb") as sample:
            sample.setparams((1, 2, cli.MIC_TEST_RATE, 0, "NONE", "not compressed"))
            sample.writeframes(b"\0\0" * (cli.MIC_TEST_RATE if complete else 1200))
        if complete and not diagnostic:
            assert session.wait() == session.path
            session.cleanup()
        else:
            with pytest.raises(cli.BackendError):
                session.wait()
        assert not session.path.exists()


def test_sample_playback_uses_pw_play(tmp_path):
    sample = tmp_path / "sample.wav"
    sample.write_bytes(b"R" * 128)
    proc = Mock()
    proc.communicate.return_value = ("", "")
    proc.returncode = 0
    with (
        patch.object(cli.shutil, "which", return_value="/usr/bin/pw-play"),
        patch.object(cli.subprocess, "Popen", return_value=proc) as popen,
    ):
        playback = cli.start_sample_playback(sample)
        assert playback.wait() is None
    assert popen.call_args.args[0] == ["/usr/bin/pw-play", str(sample)]


def test_calibration_shutdown_restores_without_blocking_gtk():
    page = object.__new__(CalibratePage)
    page._running = True
    page._cancelled = False
    page._source = "mic"
    page._original_volume = 137
    page._tick_id = 0
    session = Mock()
    page._session = session
    entered, release = threading.Event(), threading.Event()

    def cancel():
        entered.set()
        assert release.wait(timeout=2)

    session.cancel.side_effect = cancel
    with patch.object(cli, "set_source_volume") as restore:
        try:
            page.shutdown()
            assert entered.wait(timeout=1)
            assert not page._cancel_thread.daemon
            assert page._cancel_thread.is_alive()
            restore.assert_not_called()
            assert page._running is False and page._session is None
        finally:
            release.set()
            page._cancel_thread.join(timeout=3)
        restore.assert_called_once_with("mic", 137)
    assert not page._cancel_thread.is_alive()


def test_calibration_rollback_survives_normal_interpreter_exit(tmp_path):
    result = tmp_path / "restored"
    code = r"""
import time
from pathlib import Path
from biglinux_microphone import cli
from biglinux_microphone.ui.pages.calibrate import CalibratePage
class Session:
    def cancel(self): time.sleep(.1)
p = object.__new__(CalibratePage)
p._running=True; p._cancelled=False; p._source="pinned-mic"
p._original_volume=137; p._tick_id=0; p._session=Session()
cli.set_source_volume=lambda source, volume: Path(RESULT).write_text(f"{source}:{volume}")
p.shutdown()
""".replace("RESULT", repr(str(result)))
    # No -S here: on a distribution that ships PyGObject in site-packages,
    # skipping the site module makes `import gi` fail and the subprocess dies
    # before it can reach the rollback this test is about.
    done = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
    )
    assert done.returncode == 0, done.stderr
    assert result.read_text() == "pinned-mic:137"


@pytest.mark.parametrize("kind", ["garbage", "truncated", "wrong_rate", "empty"])
def test_microphone_sample_validates_wav_not_just_size(tmp_path, monkeypatch, kind):
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    tmp_path.chmod(0o700)
    proc = Mock(returncode=0)
    proc.communicate.return_value = ("", "")
    with (
        patch.object(cli.shutil, "which", return_value="/usr/bin/pw-record"),
        patch.object(cli.subprocess, "Popen", return_value=proc),
    ):
        session = cli.start_microphone_sample("mic", 5)
        with wave.open(str(session.path), "wb") as sample:
            sample.setparams(
                (
                    1,
                    2,
                    8000 if kind == "wrong_rate" else 48000,
                    0,
                    "NONE",
                    "not compressed",
                )
            )
            sample.writeframes(b"\x00\x00" * (0 if kind == "empty" else 480))
        if kind == "garbage":
            session.path.write_bytes(b"R" * 128)
        elif kind == "truncated":
            session.path.write_bytes(session.path.read_bytes()[:-20])
        with pytest.raises(cli.BackendError, match="empty or incomplete"):
            session.wait()
        assert not session.path.exists()


@pytest.mark.parametrize("relative", [True, False])
def test_private_sample_ignores_relative_or_world_readable_runtime(
    tmp_path, monkeypatch, relative
):
    directory = tmp_path / "runtime"
    directory.mkdir(mode=0o755)
    directory.chmod(0o755)
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("XDG_RUNTIME_DIR", "runtime" if relative else str(directory))
    with patch.object(cli.tempfile, "mkstemp", wraps=cli.tempfile.mkstemp) as create:
        sample = cli._new_private_wav("biglinux-test-")
    try:
        assert create.call_args.kwargs["dir"] is None
        assert sample.stat().st_mode & 0o777 == 0o600
    finally:
        sample.unlink()


@pytest.mark.parametrize("cancel", [False, True])
def test_comparison_balances_private_samples_and_cancel_reaps_renderer(
    tmp_path, monkeypatch, cancel
):
    import math
    import time
    from array import array

    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    renderer = tmp_path / "ffmpeg"
    renderer.write_text(
        f"#!{sys.executable}\n"
        "import sys,wave,time\n"
        "from array import array\n"
        + ("time.sleep(10)\n" if cancel else "")
        + "source=sys.argv[sys.argv.index('-i')+1]\n"
        "with wave.open(source,'rb') as f:\n"
        " params=f.getparams(); pcm=array('h',f.readframes(f.getnframes()))\n"
        "with wave.open(sys.argv[-1],'wb') as f:\n"
        " f.setparams(params); f.writeframes(array('h',(round(x/4) for x in pcm)).tobytes())\n"
    )
    renderer.chmod(0o700)
    monkeypatch.setattr(cli.shutil, "which", lambda _name: str(renderer))

    class Capture:
        def __init__(self, target):
            assert target == "physical-mic"
            self.path = cli._new_private_wav("original-")
            pcm = array("h", (round(4000 * math.sin(n * 0.07)) for n in range(48000)))
            with wave.open(str(self.path), "wb") as sample:
                sample.setparams((1, 2, 48000, 0, "NONE", "not compressed"))
                sample.writeframes(pcm.tobytes())

        def wait(self):
            return self.path

        def cancel(self):
            pass

        def cleanup(self):
            self.path.unlink(missing_ok=True)

    monkeypatch.setattr(cli, "MicrophoneSampleSession", Capture)
    session = cli.SampleComparisonSession(
        "physical-mic",
        "ladspa=file=/usr/lib/ladspa/libdfn3_ladspa.so:plugin=deep_filter_net3_rs_mono:controls=c0=100.00",
    )
    paths = (session.original_path, session.path)
    assert all(path.stat().st_mode & 0o777 == 0o600 for path in paths)
    errors = []

    def render():
        try:
            session.wait()
        except cli.BackendError as error:
            errors.append(str(error))

    thread = threading.Thread(target=render)
    thread.start()
    try:
        if cancel:
            deadline = time.monotonic() + 2
            while session._process is None and time.monotonic() < deadline:
                time.sleep(0.01)
            assert session._process is not None
            session.cancel()
        thread.join(timeout=3)
        assert not thread.is_alive()
        assert session._process.poll() is not None
        if cancel:
            assert errors
            assert all(not path.exists() for path in paths)
        else:
            assert not errors
            levels = []
            for path in paths:
                assert path.stat().st_mode & 0o777 == 0o600
                with wave.open(str(path), "rb") as sample:
                    pcm = array("h", sample.readframes(sample.getnframes()))
                levels.append(math.sqrt(sum(x * x for x in pcm) / len(pcm)))
                assert max(abs(x) for x in pcm) <= 30000
            assert abs(levels[0] - levels[1]) < 2
    finally:
        session.cancel()
        thread.join(timeout=3)
        session.cleanup()
    assert all(not path.exists() for path in paths)


@pytest.mark.parametrize(
    "expression",
    [None, "", "volume=1", "ladspa=file=/tmp/plugin.so:controls=c0=100.00"],
)
def test_comparison_refuses_arbitrary_filter_expressions(expression):
    with patch.object(cli, "MicrophoneSampleSession") as capture:
        with pytest.raises(cli.BackendError):
            cli.SampleComparisonSession("physical-mic", expression)
        capture.assert_not_called()
