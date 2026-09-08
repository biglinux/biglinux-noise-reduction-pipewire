from pathlib import Path
from _common import done, replace, write, commit

TITLE = 'fix(i18n): extract indirect messages and Plasma strings into one catalog'
if not done(TITLE):
    path = 'src/ui/i18n.rs'
    with Path(path).open('a') as file:
        file.write('''
/// Extraction marker for a literal that is translated only when displayed.
/// This must not translate at startup, before the locale is initialized.
pub const fn mark(message: &'static str) -> &'static str { message }
''')
    path = 'src/ui/window.rs'
    replace(path, 'use super::i18n::i18n;', 'use super::i18n::{i18n, mark};')
    replace(path, 'const PRIMARY_MENU_LABEL: &str = "Main menu";', 'const PRIMARY_MENU_LABEL: &str = mark("Main menu");')
    replace(path, '("Restore default settings", "win.reset-defaults")', '(mark("Restore default settings"), "win.reset-defaults")')
    replace(path, '("About Filter noise", "win.about")', '(mark("About Filter noise"), "win.about")')
    path = 'src/ui/widgets/spectrum.rs'
    replace(path, 'use crate::ui::i18n::i18n;', 'use crate::ui::i18n::{i18n, mark};')
    replace(path, 'const PEAK_METER_CAPTION_MSGID: &str = "LEVEL / PEAK";', 'const PEAK_METER_CAPTION_MSGID: &str = mark("LEVEL / PEAK");')
    write('scripts/refresh-pot.sh', r'''#!/usr/bin/env bash
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
''')
    # Keep new source files in extraction, without scanning vendored examples.
    path = Path('po/POTFILES.in')
    old = path.read_text().splitlines()
    present = set(old)
    for source in sorted(Path('src/ui').rglob('*.rs')):
        if any(word in source.read_text() for word in ('i18n(', 'mark(')) and not source.stem.endswith('_tests'):
            if str(source) not in present: old.append(str(source))
    path.write_text('\n'.join(old)+'\n')
    commit(TITLE, ['src/ui/i18n.rs','src/ui/window.rs','src/ui/widgets/spectrum.rs','scripts/refresh-pot.sh','po/POTFILES.in'])
