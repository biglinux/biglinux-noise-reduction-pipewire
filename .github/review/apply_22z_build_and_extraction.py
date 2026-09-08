from _common import done, replace, write, commit

TITLE = 'fix(ui): import libadwaita traits for advanced runtime controls'
if not done(TITLE):
    path = 'src/ui/views/mic.rs'
    replace(path, 'use gtk::prelude::*;', 'use adw::prelude::*;')
    commit(TITLE, [path])

TITLE = 'fix(i18n): preserve UTF-8 headers and deduplicate extracted messages'
if not done(TITLE):
    write('scripts/refresh-pot.sh', r'''#!/usr/bin/env bash
# Extract Rust and QML into one UTF-8 gettext domain. --check compares source
# coverage without changing the tracked template or translator catalogs.
set -euo pipefail
export LC_ALL=C.UTF-8
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
for tool in xtr xgettext msguniq msgcat msgcmp msgmerge; do
    command -v "$tool" >/dev/null || { printf 'Missing translation tool: %s\n' "$tool" >&2; exit 1; }
done
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
mapfile -t rust_sources < <(grep -vE '^\s*(#|$)' po/POTFILES.in | grep -E '\.rs$')
mapfile -t qml_sources < <(grep -vE '^\s*(#|$)' po/POTFILES.in | grep -E '\.qml$')
((${#rust_sources[@]} > 0)) || { echo 'POTFILES.in has no Rust sources' >&2; exit 1; }
version="$(awk -F\" '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"
xtr --keywords=i18n --keywords=mark --package-name=biglinux-noise-reduction-pipewire \
    --package-version="$version" --copyright-holder='BigLinux Team' \
    --output "$work/rust.pot" "${rust_sources[@]}"
# A source listed both directly and as a Rust child module may be visited
# twice. Normalize extractor duplicates before combining separate languages.
msguniq --use-first "$work/rust.pot" -o "$work/rust-unique.pot"
if ((${#qml_sources[@]} > 0)); then
    # Retain the UTF-8 header: a headerless QML catalog is interpreted as
    # ASCII by msgcat, corrupting ellipses and translated punctuation.
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
''')
    commit(TITLE, ['scripts/refresh-pot.sh'])
