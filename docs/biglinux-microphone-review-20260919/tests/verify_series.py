#!/usr/bin/env python3
"""Verify all ordered patches and reverse them in an isolated touched-file tree.
No changes to SOURCE; no Rust, audio, services, downloads or installations.
Usage: python3 tests/verify_series.py /path/to/pristine/extracted/source
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tempfile


def sha(path: Path) -> str | None:
    if path.is_symlink():
        raise ValueError(f"Refusing symlink: {path}")
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None


def relative(text: str) -> Path:
    p = PurePosixPath(text)
    if p.is_absolute() or not p.parts or any(x in ('.', '..', '.git') for x in p.parts):
        raise ValueError(f"Unsafe path: {text}")
    return Path(*p.parts)


def run(root: Path, *args: str) -> None:
    p = subprocess.run(['git', '-C', str(root), *args], stdin=subprocess.DEVNULL,
                       capture_output=True, text=True, timeout=30)
    if p.returncode:
        raise RuntimeError(f"git {' '.join(args)}: {p.stderr.strip()}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    args = parser.parse_args()
    bundle = Path(__file__).resolve().parent.parent
    source = args.source.resolve(strict=True)
    manifest = json.loads((bundle/'MANIFEST.json').read_text())
    names = (bundle/'patches/series').read_text().splitlines()
    if names != [p['file'] for p in manifest['patches']]:
        raise ValueError('series order differs from MANIFEST.json')
    for patch in manifest['patches']:
        name = relative(patch['file'])
        if len(name.parts) != 1 or sha(bundle/'patches'/name) != patch['sha256']:
            raise ValueError(f"Patch checksum mismatch: {name}")
    with tempfile.TemporaryDirectory(prefix='microphone-patch-check-') as temp:
        scratch = Path(temp)
        for f in manifest['files']:
            rel = relative(f['path']); actual = source/rel
            for parent in list(rel.parents)[:-1]:
                if (source/parent).is_symlink():
                    raise ValueError(f"Symlink ancestor: {parent}")
            if sha(actual) != f['before_sha256']:
                raise ValueError(f"Not the uploaded base (or already patched): {rel}")
            if actual.is_file():
                (scratch/rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(actual, scratch/rel)
        run(scratch, 'init', '-q')
        for name in names:
            patch = str(bundle/'patches'/name)
            run(scratch, 'apply', '--check', patch)
            run(scratch, 'apply', patch)
        for f in manifest['files']:
            if sha(scratch/relative(f['path'])) != f['after_sha256']:
                raise ValueError(f"Post-apply checksum mismatch: {f['path']}")
        for name in reversed(names):
            patch = str(bundle/'patches'/name)
            run(scratch, 'apply', '--reverse', '--check', patch)
            run(scratch, 'apply', '--reverse', patch)
        for f in manifest['files']:
            if sha(scratch/relative(f['path'])) != f['before_sha256']:
                raise ValueError(f"Post-reverse checksum mismatch: {f['path']}")
    print(json.dumps({'result':'PASS', 'patches':len(names), 'files':len(manifest['files']),
        'checks':['baseline content', 'patch hashes/order', 'forward application',
                  'patched content hashes', 'reverse application', 'baseline restored'],
        'scope':'isolated touched-file tree; source untouched', 'rust':'NOT_RUN',
        'hardware':'NOT_RUN'}, indent=2))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        raise SystemExit(f'FAIL: {error}')
