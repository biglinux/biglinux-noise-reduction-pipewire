#!/usr/bin/env bash
# Extract the Python front-end and the QML plasmoid into one UTF-8 gettext
# domain. --check compares source coverage without changing the tracked
# template or translator catalogs.
set -euo pipefail
export LC_ALL=C.UTF-8
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
for tool in xgettext msgcat msgcmp msgmerge; do
    command -v "$tool" >/dev/null || { printf 'Missing translation tool: %s\n' "$tool" >&2; exit 1; }
done
work="$(mktemp -d)"
trap 'status=$?; rm -rf -- "$work" 2>/dev/null || :; exit "$status"' EXIT

mapfile -t py_sources < <(grep -vE '^\s*(#|$)' po/POTFILES.in | grep -E '\.py$')
mapfile -t qml_sources < <(grep -vE '^\s*(#|$)' po/POTFILES.in | grep -E '\.qml$')
((${#py_sources[@]} > 0)) || { echo 'POTFILES.in has no Python sources' >&2; exit 1; }
version="$(awk -F\" '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"

xgettext --language=Python --from-code=UTF-8 --keyword=_ \
    --package-name=biglinux-noise-reduction-pipewire --package-version="$version" \
    --copyright-holder='BigLinux Team' \
    --output="$work/py.pot" "${py_sources[@]}"

if ((${#qml_sources[@]} > 0)); then
    xgettext --language=JavaScript --from-code=UTF-8 \
        --keyword=i18n --keyword=i18nd:2 --keyword=i18nc:1c,2 \
        --package-name=biglinux-noise-reduction-pipewire --package-version="$version" \
        --output="$work/qml.pot" "${qml_sources[@]}"
    msgcat --use-first --sort-output "$work/py.pot" "$work/qml.pot" -o "$work/all.pot"
else
    msgcat --sort-output "$work/py.pot" -o "$work/all.pot"
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
