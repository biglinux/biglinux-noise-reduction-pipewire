#!/usr/bin/env python3
"""Temporary publication helper; verifies exact base, files and final tree."""
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
# Every correction specifies its original bytes. The complete corrected data
# must still pass the independent SHA-256, per-file checksums and tree check.
for at, old, new in sorted(request.get('transport_corrections', []), reverse=True):
    assert payload[at:at + len(old)] == old
    payload = payload[:at] + new + payload[at + len(old):]
data = gzip.decompress(base64.b64decode(payload, validate=True))
assert digest(data) == request['sha256'], 'Payload checksum mismatch'
if request.get('format') == 'line-edits':
    for commit in json.loads(data):
        paths = []
        for change in commit['changes']:
            relative = PurePosixPath(change['path'])
            assert not relative.is_absolute() and '..' not in relative.parts and '.git' not in relative.parts
            path = Path(relative)
            assert not path.is_symlink()
            old = path.read_bytes() if path.exists() else None
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
        subprocess.run(['git', 'add', '-f', '--', *paths], check=True)
        subprocess.run(['git', 'diff', '--cached', '--check'], check=True)
        subprocess.run(['git', 'commit', '-m', commit['message']], check=True)
else:
    subprocess.run(['git', 'am', '--keep-cr'], input=data, check=True)
assert git('rev-parse', 'HEAD^{tree}') == request['tree'], 'Replayed tree differs from checked offline source'
