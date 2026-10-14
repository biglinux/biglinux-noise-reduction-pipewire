#!/usr/bin/env bash
# Maintainer-side only. Uses the already installed compiler; never installs Rust.
set -euo pipefail
ROOT=$(realpath "${1:?Usage: run-core-rust.sh /path/to/project-or-reviewed-code}")
command -v rustc >/dev/null || { echo 'An existing Rust compiler is required; nothing will be installed.' >&2; exit 2; }
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
python3 - "$ROOT" "$TMP/harness.rs" <<'PY'
import json
import pathlib
import sys
root = pathlib.Path(sys.argv[1])
quote = lambda s: json.dumps(str(s), ensure_ascii=False)
paths = [root/'src/gain_policy.rs', root/'src/agc/features.rs', root/'src/agc/control.rs']
for path in paths:
    if not path.is_file():
        raise SystemExit(f'Missing patched source: {path}')
text = f'''// Pure std-only policy/adapter tests; no PipeWire, GTK, model or device needed.
#![allow(dead_code)]
#[path = {quote(paths[0])}]
mod gain_policy;
mod agc {{
    #[path = {quote(paths[1])}]
    mod features;
    #[path = {quote(paths[2])}]
    mod control;
}}
'''
pathlib.Path(sys.argv[2]).write_text(text)
PY
rustc --version
rustc --edition=2024 --test "$TMP/harness.rs" -o "$TMP/agc-tests"
"$TMP/agc-tests" --nocapture
