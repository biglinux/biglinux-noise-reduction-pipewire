#!/usr/bin/env bash
# Extract Rust and QML into one UTF-8 gettext domain. --check compares source
# coverage without changing the tracked template or translator catalogs.
set -euo pipefail
export LC_ALL=C.UTF-8
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
for tool in xtr xgettext msguniq msgcat msgcmp msgmerge python3; do
    command -v "$tool" >/dev/null || { printf 'Missing translation tool: %s\n' "$tool" >&2; exit 1; }
done
work="$(mktemp -d)"
trap 'status=$?; rm -rf -- "$work" 2>/dev/null || :; exit "$status"' EXIT
mapfile -t rust_sources < <(grep -vE '^\s*(#|$)' po/POTFILES.in | grep -E '\.rs$')
mapfile -t qml_sources < <(grep -vE '^\s*(#|$)' po/POTFILES.in | grep -E '\.qml$')
((${#rust_sources[@]} > 0)) || { echo 'POTFILES.in has no Rust sources' >&2; exit 1; }
version="$(awk -F\" '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"
xtr --keywords=i18n --keywords=mark --package-name=biglinux-noise-reduction-pipewire \
    --package-version="$version" --copyright-holder='BigLinux Team' \
    --output "$work/rust.pot" "${rust_sources[@]}"
# gettext reserves the context-free empty msgid for its header. xtr can
# also extract an empty UI literal; gettext then rejects the duplicate
# before msguniq can merge it. Remove only empty msgids, not multiline
# nonempty messages, and write exactly one explicit UTF-8 header.
python3 - "$work/rust.pot" "$work/rust-clean.pot" "$version" <<'PY'
import ast
import re
import sys
from pathlib import Path
source, destination, version = sys.argv[1:]
kept = []
for block in re.split(r'\n[ \t]*\n', Path(source).read_text(encoding='utf-8')):
    lines = block.splitlines()
    if 'msgid ""' in lines and not any(line.startswith('msgctxt ') for line in lines):
        start = lines.index('msgid ""') + 1
        fragments = []
        for line in lines[start:]:
            if not line.startswith('"'):
                break
            fragments.append(ast.literal_eval(line))
        if not ''.join(fragments):
            continue
    if block.strip():
        kept.append(block)
header = '\n'.join([
    'msgid ""', 'msgstr ""',
    '"Project-Id-Version: biglinux-noise-reduction-pipewire ' + version + '\\n"',
    '"MIME-Version: 1.0\\n"',
    '"Content-Type: text/plain; charset=UTF-8\\n"',
    '"Content-Transfer-Encoding: 8bit\\n"',
])
Path(destination).write_text(header + '\n\n' + '\n\n'.join(kept) + '\n', encoding='utf-8')
PY
msguniq --use-first "$work/rust-clean.pot" -o "$work/rust-unique.pot"
if ((${#qml_sources[@]} > 0)); then
    xgettext --language=JavaScript --from-code=UTF-8 --keyword=i18nd:2 \
        --package-name=biglinux-noise-reduction-pipewire --package-version="$version" \
        --output="$work/qml.pot" "${qml_sources[@]}"
    msgcat --use-first --sort-output "$work/rust-unique.pot" "$work/qml.pot" -o "$work/all.pot"
else
    msgcat --sort-output "$work/rust-unique.pot" -o "$work/all.pot"
fi
pot=po/biglinux-noise-reduction-pipewire.pot
if [[ "${1:-}" == --check ]]; then
    msgcmp --use-untranslated --use-fuzzy "$work/all.pot" "$pot"
    msgcmp --use-untranslated --use-fuzzy "$pot" "$work/all.pot"
    exit 0
fi
[[ $# == 0 ]] || { echo 'Usage: scripts/refresh-pot.sh [--check]' >&2; exit 2; }
cat "$work/all.pot" > "$pot"
for catalog in po/*.po; do
    msgmerge --quiet --update --backup=none "$catalog" "$pot"
done
