#!/usr/bin/env bash
# Extract Rust + QML into the shared gettext domain, then preserve every
# existing translation with msgmerge. --check validates source coverage
# without modifying the tracked POT or any translator's catalog.
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
for tool in xtr xgettext msgcat msgcmp msgmerge; do
    command -v "$tool" >/dev/null || { printf 'Missing translation tool: %s\n' "$tool" >&2; exit 1; }
done
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
mapfile -t rust_sources < <(grep -vE '^\s*(#|$)' po/POTFILES.in | grep -E '\.rs$')
mapfile -t qml_sources < <(grep -vE '^\s*(#|$)' po/POTFILES.in | grep -E '\.qml$')
((${#rust_sources[@]} > 0)) || { echo 'POTFILES.in has no Rust sources' >&2; exit 1; }
version="$(awk -F\" '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"
xtr --keywords=i18n --keywords=mark --package-name=biglinux-noise-reduction-pipewire \
    --package-version="$version" --copyright-holder='BigLinux Team' --omit-header \
    --output "$work/rust.pot" "${rust_sources[@]}"
{
    printf 'msgid ""\nmsgstr ""\n'
    printf '"Project-Id-Version: biglinux-noise-reduction-pipewire %s\\n"\n' "$version"
    printf '"MIME-Version: 1.0\\n"\n"Content-Type: text/plain; charset=UTF-8\\n"\n"Content-Transfer-Encoding: 8bit\\n"\n\n'
    cat "$work/rust.pot"
} > "$work/combined.pot"
if ((${#qml_sources[@]} > 0)); then
    xgettext --language=JavaScript --from-code=UTF-8 --keyword=i18nd:2 \
        --package-name=biglinux-noise-reduction-pipewire --package-version="$version" \
        --omit-header --output="$work/qml.pot" "${qml_sources[@]}"
    msgcat --use-first --sort-output "$work/combined.pot" "$work/qml.pot" -o "$work/all.pot"
else
    msgcat --sort-output "$work/combined.pot" -o "$work/all.pot"
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
