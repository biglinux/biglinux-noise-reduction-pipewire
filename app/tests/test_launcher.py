from __future__ import annotations

import json
import os
import shutil
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
LAUNCHER = ROOT / "packaging" / "launcher.py"


def _make_install(tmp_path: Path, main_source: str) -> Path:
    app_root = tmp_path / "usr" / "lib" / "biglinux-microphone"
    package = app_root / "biglinux_microphone"
    package.mkdir(parents=True)
    shutil.copy2(LAUNCHER, app_root / "launcher.py")
    (app_root / "launcher.py").chmod(0o755)
    (package / "__init__.py").write_text("__version__ = 'test'\n")
    (package / "main.py").write_text(main_source)
    bin_dir = tmp_path / "usr" / "bin"
    bin_dir.mkdir(parents=True)
    entry = bin_dir / "biglinux-microphone"
    entry.symlink_to("../lib/biglinux-microphone/launcher.py")
    return entry


def test_launcher_resolves_relative_usr_bin_symlink_and_preserves_arguments(tmp_path: Path) -> None:
    entry = _make_install(
        tmp_path,
        "import json, sys\ndef main():\n    print(json.dumps(sys.argv[1:]))\n    raise SystemExit(0)\n",
    )
    done = subprocess.run([entry, "--debug", "value"], text=True, capture_output=True, check=False)
    assert done.returncode == 0, done.stderr
    assert json.loads(done.stdout) == ["--debug", "value"]


def test_launcher_isolated_mode_ignores_pythonpath_and_user_shadowing(tmp_path: Path) -> None:
    entry = _make_install(
        tmp_path,
        "def main():\n    print('private-app')\n    raise SystemExit(0)\n",
    )
    rogue = tmp_path / "rogue"
    (rogue / "biglinux_microphone").mkdir(parents=True)
    (rogue / "biglinux_microphone" / "__init__.py").write_text("")
    (rogue / "biglinux_microphone" / "main.py").write_text(
        "def main():\n    print('rogue-app')\n    raise SystemExit(99)\n"
    )
    env = os.environ.copy()
    env["PYTHONPATH"] = str(rogue)
    env["PYTHONUSERBASE"] = str(rogue)
    done = subprocess.run([entry], env=env, text=True, capture_output=True, check=False)
    assert done.returncode == 0, done.stderr
    assert done.stdout.strip() == "private-app"


def test_launcher_does_not_scan_old_python_site_packages(tmp_path: Path) -> None:
    entry = _make_install(
        tmp_path,
        "def main():\n    print('private-app')\n    raise SystemExit(0)\n",
    )
    old_package = tmp_path / "usr" / "lib" / "python3.14" / "site-packages" / "biglinux_microphone"
    old_package.mkdir(parents=True)
    (old_package / "__init__.py").write_text("")
    (old_package / "main.py").write_text("raise RuntimeError('old package must not load')\n")
    done = subprocess.run([entry], text=True, capture_output=True, check=False)
    assert done.returncode == 0, done.stderr
    assert done.stdout.strip() == "private-app"


def test_launcher_reports_incomplete_installation(tmp_path: Path) -> None:
    app_root = tmp_path / "usr" / "lib" / "biglinux-microphone"
    app_root.mkdir(parents=True)
    shutil.copy2(LAUNCHER, app_root / "launcher.py")
    (app_root / "launcher.py").chmod(0o755)
    done = subprocess.run([app_root / "launcher.py"], text=True, capture_output=True, check=False)
    assert done.returncode == 127
    assert "installation is incomplete" in done.stderr


def test_launcher_explains_pygobject_missing_for_active_python(tmp_path: Path) -> None:
    entry = _make_install(
        tmp_path,
        "raise ModuleNotFoundError(\"No module named 'gi'\", name='gi')\n",
    )
    done = subprocess.run([entry], text=True, capture_output=True, check=False)
    assert done.returncode == 127
    assert "PyGObject is unavailable" in done.stderr


def test_launcher_has_isolated_python_shebang() -> None:
    assert LAUNCHER.read_text().splitlines()[0] == "#!/usr/bin/python3 -I"
    assert "python3." not in LAUNCHER.read_text()
    assert "site-packages" in LAUNCHER.read_text()  # documentation only, never a search path
