#!/usr/bin/env bash
# Regenerate po/biglinux-noise-reduction-pipewire.pot from sources listed
# in po/POTFILES.in, then msgmerge every locale catalog. Idempotent —
# safe to run as part of release prep.
#
# Requires: xtr (cargo install xtr), gettext (msgmerge / msgcmp).

set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
PO_DIR="$ROOT/po"
POT="$PO_DIR/biglinux-noise-reduction-pipewire.pot"
POTFILES="$PO_DIR/POTFILES.in"

cd "$ROOT"

if [[ ! -f "$POTFILES" ]]; then
    printf 'refresh-pot: missing %s\n' "$POTFILES" >&2
    exit 1
fi

if ! command -v xtr >/dev/null 2>&1; then
    # shellcheck disable=SC2016  # backticks are literal text in the message
    printf 'refresh-pot: xtr not found — install with `cargo install xtr`\n' >&2
    exit 1
fi

# Collect source paths from POTFILES.in, skipping comments / blanks.
mapfile -t SOURCES < <(grep -vE '^\s*(#|$)' "$POTFILES")
if [[ ${#SOURCES[@]} -eq 0 ]]; then
    printf 'refresh-pot: POTFILES.in has no sources\n' >&2
    exit 1
fi

xtr \
    --keywords=i18n \
    --package-name=biglinux-noise-reduction-pipewire \
    --package-version="$(awk -F\" '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)" \
    --copyright-holder='BigLinux Team' \
    --omit-header \
    --output "$POT" \
    "${SOURCES[@]}"

# xtr's --omit-header drops the canonical preamble; restore a minimal
# one so msgmerge stays happy and existing `.po` files keep applying.
TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT
{
    printf '# SOME DESCRIPTIVE TITLE.\n'
    printf '# Copyright (C) YEAR BigLinux Team\n'
    printf '# This file is distributed under the same license as the biglinux-noise-reduction-pipewire package.\n'
    printf '# FIRST AUTHOR <EMAIL@ADDRESS>, YEAR.\n'
    printf '#\n'
    printf '#, fuzzy\n'
    printf 'msgid ""\n'
    printf 'msgstr ""\n'
    printf '"Project-Id-Version: biglinux-noise-reduction-pipewire %s\\n"\n' \
        "$(awk -F\" '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"
    printf '"Report-Msgid-Bugs-To: \\n"\n'
    printf '"POT-Creation-Date: %s\\n"\n' "$(date -u +'%Y-%m-%d %H:%M+0000')"
    printf '"PO-Revision-Date: YEAR-MO-DA HO:MI+ZONE\\n"\n'
    printf '"Last-Translator: FULL NAME <EMAIL@ADDRESS>\\n"\n'
    printf '"Language-Team: LANGUAGE <LL@li.org>\\n"\n'
    printf '"Language: \\n"\n'
    printf '"MIME-Version: 1.0\\n"\n'
    printf '"Content-Type: text/plain; charset=UTF-8\\n"\n'
    printf '"Content-Transfer-Encoding: 8bit\\n"\n'
    printf '\n'
    cat "$POT"
} > "$TMP"
mv "$TMP" "$POT"
trap - EXIT

# Merge into every locale catalog, preserving translations.
shopt -s nullglob
for po in "$PO_DIR"/*.po; do
    msgmerge --quiet --update --backup=none "$po" "$POT"
done
shopt -u nullglob

printf 'refresh-pot: %s regenerated, %d catalogs merged\n' \
    "$POT" "$(find "$PO_DIR" -maxdepth 1 -name '*.po' | wc -l)"
