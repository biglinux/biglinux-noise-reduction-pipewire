"""Exercise the actual standalone monitor outside the package import path."""
import json
import os
from pathlib import Path
import subprocess

from biglinux_microphone import cli


def test_private_monitor_does_not_need_site_packages_or_pythonpath(tmp_path):
    loader = tmp_path / "biglinux-microphone-pwloader"
    loader.write_text(
        "#!/usr/bin/python3 -I\n"
        "import json, sys\n"
        "from pathlib import Path\n"
        "print(json.dumps({'module':sys.argv[1], 'config':Path(sys.argv[2]).read_text()}))\n"
        "import time\n"
        f"while not Path({str(tmp_path / 'reader.json')!r}).exists(): time.sleep(0.01)\n"
    )
    loader.chmod(0o700)
    # The reader that keeps the microphone running: records its arguments, then
    # waits to be stopped together with the loader.
    reader = tmp_path / "pw-cat"
    reader.write_text(
        "#!/usr/bin/python3 -I\n"
        "import json, sys, time\n"
        f"open({str(tmp_path / 'reader.json')!r}, 'w').write(json.dumps(sys.argv[1:]))\n"
        "time.sleep(30)\n"
    )
    reader.chmod(0o700)
    # A normal '-m' invocation from the installed private tree cannot find the
    # package. It must also never import modules supplied by the user's cwd.
    (tmp_path / "signal.py").write_text("raise RuntimeError('untrusted shadow module')\n")
    env = dict(os.environ, PATH=str(tmp_path), PYTHONPATH=str(tmp_path))
    argv = cli.monitor_argv()
    assert argv[1] == "-I"
    assert Path(argv[2]).is_absolute()
    done = subprocess.run(argv, cwd=tmp_path, env=env, text=True, capture_output=True, timeout=5)
    assert done.returncode == 0, done.stderr
    result = json.loads(done.stdout)
    assert result["module"] == "libpipewire-module-filter-chain"
    assert '"Delay (s)" = 2.0' in result["config"]
    # Routed like the delay's capture (no fixed target), and gone with the monitor.
    reader_args = json.loads((tmp_path / "reader.json").read_text())
    assert "--record" in reader_args and "--target" not in reader_args
