"""Exercise package() with explicit fake Rust files; not an Arch build test."""
import os
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
NAME = 'biglinux-noise-reduction-pipewire'
BINS = ('biglinux-microphone-cli', 'biglinux-microphone-probe', 'biglinux-microphone-pwloader')


def fixture_tree(tmp_path):
    source = tmp_path/'src'/NAME
    source.mkdir(parents=True)
    # Only package-owned files are needed to exercise installation layout.
    for package_dir in ('biglinux_microphone', 'big_gtk_kit'):
        shutil.copytree(ROOT/'app'/package_dir, source/'app'/package_dir,
                        ignore=shutil.ignore_patterns('__pycache__', '*.pyc'))
    shutil.copytree(ROOT/'docs', source/'docs')
    for name in ('usr', 'packaging'):
        (source/name).symlink_to(ROOT/name, target_is_directory=True)
    for name in ('README.md', 'CONTRIBUTING.md', 'SUPPORT.md',
                 'SECURITY.md', 'CODE_OF_CONDUCT.md', 'LICENSE', 'LICENSE-MIT'):
        shutil.copy2(ROOT/name, source/name)
    cache = source/'app/biglinux_microphone/__pycache__'
    cache.mkdir()
    (cache/'must-not-ship.pyc').write_bytes(b'fixture')
    catalog = source/'build-locale/pt_BR/LC_MESSAGES/biglinux-microphone.mo'
    catalog.parent.mkdir(parents=True)
    # Layout fixture only; GNU gettext validates actual catalogs in its own gate.
    catalog.write_bytes(b'catalog-layout-fixture')
    return source


def package(tmp_path, source, target):
    env = os.environ.copy()
    env.update(srcdir=str(source.parent), pkgdir=str(tmp_path/'payload'),
               CARGO_TARGET_DIR=str(target), BIGMIC_SOURCE_COMMIT='')
    return subprocess.run(['bash', '-c', 'set -e; source "$1"; package', 'package-fixture',
                           str(ROOT/'pkgbuild/PKGBUILD')],
                          env=env, text=True, capture_output=True, timeout=15)


@pytest.mark.parametrize('relative', [False, True])
def test_package_uses_custom_target_and_preserves_private_layout(tmp_path, relative):
    source = fixture_tree(tmp_path)
    target = source/'custom-target' if relative else tmp_path/'external-target'
    (target/'release').mkdir(parents=True)
    for name in BINS:
        (target/'release'/name).write_bytes(b'FAKE-RUST-BINARY-LAYOUT-TEST\n')
    done = package(tmp_path, source, 'custom-target' if relative else target)
    assert done.returncode == 0, done.stderr
    payload = tmp_path/'payload'
    entry = payload/'usr/bin/biglinux-microphone'
    assert entry.is_symlink()
    assert os.readlink(entry) == '../lib/biglinux-microphone/launcher.py'
    assert entry.resolve().is_file()
    assert entry.read_text().startswith('#!/usr/bin/python3 -I')
    for name in BINS:
        assert (payload/'usr/bin'/name).read_bytes().startswith(b'FAKE-RUST-BINARY')
    assert not list(payload.rglob('*.pyc'))
    assert not list(payload.rglob('__pycache__'))
    # A rolling base moves the interpreter on its own schedule; a front-end left
    # in /usr/lib/python3.N/site-packages leaves 3.N+1's search path with it.
    assert not list((payload/'usr/lib').glob('python*'))
    assert len(list((payload/'usr/share/locale').glob('*/LC_MESSAGES/*.mo'))) == 1
    docs = payload/'usr/share/doc'/NAME
    assert (docs/'docs/screenshots/home.png').is_file()
    assert (docs/'docs/guia-rapido.pt_BR.md').is_file()
    assert (docs/'SUPPORT.md').is_file()
    for link in payload.rglob('*'):
        if link.is_symlink():
            assert link.resolve().is_relative_to(payload)


def test_package_refuses_missing_backend_before_creating_gui_payload(tmp_path):
    source = fixture_tree(tmp_path)
    done = package(tmp_path, source, tmp_path/'missing-target')
    assert done.returncode != 0
    assert not (tmp_path/'payload/usr/lib/biglinux-microphone').exists()
