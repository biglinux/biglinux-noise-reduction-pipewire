"""Cheap guard/parser regressions; never start Cargo or allocate pressure loads."""
import fcntl
import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def load_tool(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'tools' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture
def guard():
    return load_tool('run-bounded')


def execute(guard, tmp_path, command, seconds=2):
    result = guard.execute(command, tmp_path/'run.log', tmp_path/'run.lock', seconds, min_free_mib=0)
    assert json.loads((tmp_path/'run.log.json').read_text()) == result
    return result


def test_guard_normal_failure_and_missing_executable(guard, tmp_path):
    assert execute(guard, tmp_path, ['/bin/echo', 'guard-ok'])['rc'] == 0
    assert 'guard-ok' in (tmp_path/'run.log').read_text()
    result = execute(guard, tmp_path, ['/bin/false'])
    assert result['classification'] == 'UNKNOWN'
    assert result['rc'] == 1
    result = execute(guard, tmp_path, [str(tmp_path/'absent')])
    assert result['classification'] == 'DEPENDENCY'
    assert result['rc'] == 127


def test_guard_refuses_overlap_before_spawning(guard, tmp_path):
    with (tmp_path/'run.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        result = execute(guard, tmp_path, ['/bin/echo', 'not-run'])
    assert result['rc'] == 125
    assert result['reason'] == 'another validation is active'
    assert not (tmp_path/'run.log').exists()


def test_guard_headroom_is_not_a_project_error(guard, tmp_path, monkeypatch):
    monkeypatch.setattr(guard, 'memory_limit', lambda: 100)
    monkeypatch.setattr(guard, 'memory_current', lambda: 81)
    result = execute(guard, tmp_path, ['/bin/echo', 'not-run'])
    assert result['classification'] == 'ENV'
    assert result['reason'] == 'insufficient cgroup headroom'
    assert not (tmp_path/'run.log').exists()


def test_guard_timeout_reaps_command(guard, tmp_path):
    pidfile = tmp_path/'pid'
    code = 'import os,time;open(%r,"w").write(str(os.getpid()));time.sleep(20)' % str(pidfile)
    result = execute(guard, tmp_path, [sys.executable, '-S', '-c', code], .3)
    assert result['rc'] == 124
    assert result['classification'] == 'UNKNOWN'
    with pytest.raises(ProcessLookupError):
        os.kill(int(pidfile.read_text()), 0)


def test_guard_records_pressure_increase_even_when_command_passes(guard, tmp_path, monkeypatch):
    samples = iter([{'max': 0}, {'max': 1}])
    monkeypatch.setattr(guard, 'memory_events', lambda: next(samples))
    result = execute(guard, tmp_path, ['/bin/true'])
    assert result['classification'] == 'ENV'
    assert result['reason'] == 'cgroup pressure increased'


def test_po_parser_context_plural_and_obsolete(tmp_path):
    tool = load_tool('validate-translations')
    path = tmp_path/'test.po'
    path.write_text('msgctxt "action"\nmsgid "Open"\nmsgstr "Abrir"\n\n'
                    'msgctxt "state"\nmsgid "Open"\nmsgstr "Aberto"\n\n'
                    'msgid "one"\nmsgid_plural "many"\nmsgstr[0] "um"\nmsgstr[1] "muitos"\n\n'
                    '#~ msgid "gone"\n#~ msgstr "antigo"\n')
    entries = tool.read_po(path)
    assert len(entries) == 3
    assert entries['action','Open']['msgstr'] == 'Abrir'
    assert entries['state','Open']['msgstr'] == 'Aberto'
    assert entries['','one']['msgstr[1]'] == 'muitos'
    path.write_text('msgid "same"\nmsgstr "x"\n\nmsgid "same"\nmsgstr "y"\n')
    with pytest.raises(ValueError, match='duplicate message'):
        tool.read_po(path)


@pytest.mark.parametrize('linguas', ['', 'pt_BR pt_BR', '../pt_BR', 'pt_BR;false'])
def test_allowlist_rejects_empty_duplicate_and_paths(tmp_path, linguas):
    tool = load_tool('validate-translations')
    (tmp_path/'po').mkdir()
    (tmp_path/'po/LINGUAS').write_text(linguas)
    with pytest.raises(ValueError):
        tool.release_languages(tmp_path)


def test_allowlist_comments_do_not_become_catalog_names(tmp_path):
    tool = load_tool('validate-translations')
    (tmp_path/'po').mkdir()
    (tmp_path/'po/LINGUAS').write_text('# Approved\npt_BR # reviewed\n')
    assert tool.release_languages(tmp_path) == ['pt_BR']


def test_maintained_documentation_links_resolve():
    import re
    from urllib.parse import unquote, urlsplit
    missing = []
    paths = list(ROOT.glob('*.md')) + list((ROOT/'docs').glob('*.md'))
    for path in paths:
        text = re.sub(r'```.*?```', '', path.read_text(), flags=re.S)
        for target in re.findall(r'!?\[[^\]]*\]\(([^)]+)\)', text):
            parsed = urlsplit(target)
            if parsed.scheme or not parsed.path:
                continue
            resolved = path.parent/unquote(parsed.path)
            if not resolved.exists():
                missing.append((str(path.relative_to(ROOT)), target))
    assert not missing


def test_installed_localedir_is_not_glibcs_own_directory(tmp_path):
    """Installed, this module lives four directories below /usr/lib, so the
    checkout's `locale/` heuristic resolves to /usr/lib/locale — a directory
    glibc always provides and that never holds our catalog. Accepting it turned
    the whole interface back to English with the translation sitting unread in
    <prefix>/share/locale. Checked in a subprocess: importing a second copy of the
    package into this one would poison every test that runs after it."""
    root = tmp_path/'usr/lib/biglinux-microphone'
    (root/'biglinux_microphone/utils').mkdir(parents=True)
    (tmp_path/'usr/lib/locale').mkdir()          # glibc's own, with no catalog of ours
    for package in ('biglinux_microphone', 'biglinux_microphone/utils'):
        (root/package/'__init__.py').write_text('')
    (root/'biglinux_microphone/utils/i18n.py').write_text(
        (ROOT/'app/biglinux_microphone/utils/i18n.py').read_text())
    done = subprocess.run(
        [sys.executable, '-c',
         'from biglinux_microphone.utils.i18n import LOCALEDIR; print(LOCALEDIR)'],
        cwd=root, capture_output=True, text=True, timeout=15)
    assert done.returncode == 0, done.stderr
    assert done.stdout.strip() == str(tmp_path/'usr/share/locale'), done.stdout
