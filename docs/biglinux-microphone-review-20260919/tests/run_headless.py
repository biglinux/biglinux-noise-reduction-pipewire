#!/usr/bin/env python3
"""Explicit GLib IMPORT shim for pure state/scheduler logic. NOT a GTK test.
Usage: python run_headless.py /path/to/patched/source
No hardware, settings write, toolchain installation or Rust invocation.
"""
import argparse
import itertools
from pathlib import Path
import sys
import types


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    args = parser.parse_args()
    root = args.source.resolve()
    sys.path.insert(0, str(root / 'app'))
    sys.dont_write_bytecode = True
    ids = itertools.count(1)
    glib = types.ModuleType('GLib')
    glib.timeout_add = lambda *_a, **_kw: next(ids)
    glib.idle_add = lambda *_a, **_kw: next(ids)
    glib.source_remove = lambda _id: True
    gi = types.ModuleType('gi')
    gi.require_version = lambda *_a: None
    repository = types.ModuleType('gi.repository')
    repository.GLib = glib
    gi.repository = repository
    sys.modules.update({'gi': gi, 'gi.repository': repository})
    print('MODEL TESTS ONLY: GI/GLib import shim; no GTK display or real GLib loop tested.', flush=True)
    import pytest
    return pytest.main(['-q', '-p', 'no:cacheprovider',
        str(root / 'app/tests/test_state_store.py'),
        str(root / 'app/tests/test_async_contracts.py')])


if __name__ == '__main__':
    raise SystemExit(main())
