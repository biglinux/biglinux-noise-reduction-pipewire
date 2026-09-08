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
assert re.fullmatch(r'batch-[0-9]+\.b64', request['patch'])
payload = (root / request['patch']).read_text()
for at, old, new in request.get('transport_corrections', []):
    assert len(old) == len(new) == 1 and payload[at] == old
    payload = payload[:at] + new + payload[at + 1:]
data = gzip.decompress(base64.b64decode(payload))
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
