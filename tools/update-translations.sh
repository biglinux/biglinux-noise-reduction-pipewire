#!/bin/sh
# Regenerate the gettext template from maintained sources and merge it into
# existing catalogs. Fuzzy matches are kept for translator review; obsolete
# entries are removed so retired UI copy does not accumulate indefinitely.
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
cd "$repo_root"

for tool in xgettext msgmerge msgattrib; do
    command -v "$tool" >/dev/null 2>&1 || {
        printf '%s\n' "Missing required gettext tool: $tool" >&2
        exit 127
    }
done

tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

python_sources="$tmp_dir/python-files"
qml_sources="$tmp_dir/qml-files"
grep -v '^[[:space:]]*#' po/POTFILES.in | grep -v '^[[:space:]]*$' | grep '\.py$' > "$python_sources" || :
grep -v '^[[:space:]]*#' po/POTFILES.in | grep -v '^[[:space:]]*$' | grep '\.qml$' > "$qml_sources" || :

[ -s "$python_sources" ] || {
    printf '%s\n' 'POTFILES.in contains no Python sources' >&2
    exit 1
}

# Paths in POTFILES.in are repository-controlled and intentionally contain no
# shell whitespace. Keeping the list there makes extraction inputs reviewable.
# shellcheck disable=SC2046
xgettext \
    --language=Python \
    --from-code=UTF-8 \
    --keyword=_ \
    --package-name=biglinux-noise-reduction-pipewire \
    --package-version=5.0.0 \
    --copyright-holder='BigLinux Team' \
    --output="$tmp_dir/messages.pot" \
    $(cat "$python_sources")

if [ -s "$qml_sources" ]; then
    # i18nd(domain, message): the translatable argument is number 2.
    # shellcheck disable=SC2046
    xgettext \
        --language=JavaScript \
        --from-code=UTF-8 \
        --keyword=i18nd:2 \
        --join-existing \
        --output="$tmp_dir/messages.pot" \
        $(cat "$qml_sources")
fi

mv "$tmp_dir/messages.pot" po/biglinux-microphone.pot

for po in po/*.po; do
    [ -f "$po" ] || continue
    msgmerge --quiet --update --backup=none "$po" po/biglinux-microphone.pot
    cleaned="$tmp_dir/$(basename "$po")"
    msgattrib --no-obsolete --output-file="$cleaned" "$po"
    mv "$cleaned" "$po"
done
