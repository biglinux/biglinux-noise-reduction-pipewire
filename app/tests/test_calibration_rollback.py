import subprocess
import sys
import threading
from unittest.mock import Mock, patch

from biglinux_microphone import cli
from biglinux_microphone.ui.pages.calibrate import CalibratePage


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
