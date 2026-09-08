#!/usr/bin/env python3
"""Temporary publisher: verifies exact base, every file and the resulting tree."""
import base64
import gzip
import hashlib
import json
import re
import subprocess
from pathlib import Path, PurePosixPath

root = Path('../workbench/.github/review/followup')
request = json.loads((root / 'request.json').read_text())

def git(*arguments):
    return subprocess.check_output(['git', *arguments], text=True).strip()

def digest(value):
    return hashlib.sha256(value).hexdigest() if value is not None else None

assert git('rev-parse', 'HEAD') == request['base'], 'PR branch moved; refusing stale changes'
parts = request.get('parts', [request['patch']])
assert all(re.fullmatch(r'batch-[0-9]+(?:-part[0-9]+)?\.b64', part) for part in parts)
payload = ''.join((root / part).read_text().strip() for part in parts)
for at, old, new in sorted(request.get('transport_corrections', []), reverse=True):
    assert payload[at:at + len(old)] == old
    payload = payload[:at] + new + payload[at + len(old):]
data = gzip.decompress(base64.b64decode(payload, validate=True))
assert digest(data) == request['sha256'], 'Payload checksum mismatch'
prepublished = set(request.get('prepublished_paths', []))
assert prepublished <= {'.github/workflows/ci.yml'}
if request.get('format') == 'line-edits':
    for commit in json.loads(data):
        paths = []
        for change in commit['changes']:
            relative = PurePosixPath(change['path'])
            assert not relative.is_absolute() and '..' not in relative.parts and '.git' not in relative.parts
            path = Path(relative)
            assert not path.is_symlink()
            old = path.read_bytes() if path.exists() else None
            if str(path) in prepublished:
                # CI was changed explicitly with the authorized GitHub connector.
                # Do not rewrite workflow files with the Actions token.
                assert digest(old) == change['after'], 'Prepublished CI differs from checked source'
                prepublished.remove(str(path))
                continue
            assert not str(path).startswith('.github/workflows/'), 'Workflow changes require connector publication'
            assert digest(old) == change['before'], 'Unexpected base content: ' + str(path)
            lines = (old or b'').decode('utf-8').splitlines(keepends=True)
            for first, last, text in reversed(change['edits']):
                assert 0 <= first <= last <= len(lines)
                lines[first:last] = [text]
            new = ''.join(lines).encode('utf-8')
            assert digest(new) == change['after'], 'Unexpected result: ' + str(path)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(new)
            paths.append(str(path))
        if paths:
            subprocess.run(['git', 'add', '-f', '--', *paths], check=True)
            subprocess.run(['git', 'diff', '--cached', '--check'], check=True)
            subprocess.run(['git', 'commit', '-m', commit['message']], check=True)
else:
    subprocess.run(['git', 'am', '--keep-cr'], input=data, check=True)
assert not prepublished, 'Prepublished path not represented in verified payload'
assert git('rev-parse', 'HEAD^{tree}') == request['tree'], 'Replayed tree differs from checked offline source'
