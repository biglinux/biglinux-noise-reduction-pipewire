from pathlib import Path
import json
import subprocess
import tomllib
from _common import done, commit

TITLE = 'fix(deps): update RustSec-affected crates and synchronized offline sources'
if not done(TITLE):
    corrected = {'anyhow': '1.0.103', 'event-listener': '5.4.2'}
    locks = ['Cargo.lock', 'vendor/big-rust-components/Cargo.lock']
    for lock in locks:
        file = Path(lock)
        packages = tomllib.loads(file.read_text())['package']
        for package in packages:
            wanted = corrected.get(package['name'])
            if wanted is None:
                continue
            old = tuple(map(int, package['version'].split('.')))
            new = tuple(map(int, wanted.split('.')))
            # Leave newer releases and unaffected older major versions alone.
            if old[0] == new[0] and old < new:
                subprocess.run(['cargo', '+stable', 'update', '--manifest-path', str(file.with_name('Cargo.toml')), '-p', package['name'] + '@' + package['version'], '--precise', wanted], check=True)
    # All registry archives and cargo checksums must describe the new lockfile.
    # Keep the generated Cargo source-replacement config at the end unchanged.
    source_file = Path('packaging/flatpak/cargo-sources.json')
    previous = json.loads(source_file.read_text())
    config = [item for item in previous if not item.get('dest', '').startswith('cargo/vendor/')]
    packages = tomllib.loads(Path('Cargo.lock').read_text())['package']
    generated = []
    for package in sorted(packages, key=lambda p: (p['name'], p['version'])):
        origin = package.get('source')
        if origin is None:
            continue
        if origin != 'registry+https://github.com/rust-lang/crates.io-index':
            raise RuntimeError('An explicit generator is required for a non-crates.io source')
        name, version, checksum = package['name'], package['version'], package['checksum']
        destination = f'cargo/vendor/{name}-{version}'
        generated.append({'type': 'archive', 'archive-type': 'tar-gzip', 'url': f'https://static.crates.io/crates/{name}/{name}-{version}.crate', 'sha256': checksum, 'dest': destination})
        generated.append({'type': 'inline', 'contents': json.dumps({'package': checksum, 'files': {}}), 'dest': destination, 'dest-filename': '.cargo-checksum.json'})
    source_file.write_text(json.dumps(generated + config, indent=4) + '\n')
    commit(TITLE, [*locks, str(source_file)])
