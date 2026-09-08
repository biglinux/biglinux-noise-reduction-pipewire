#!/usr/bin/env bash
# CI-only reproduction of the jemalloc-gtk-fixed runtime requirement.
# Upstream 5.3.1 calls sdallocx on NULL from both C23 entry points. GLib may
# legitimately call free_sized(NULL, size); preserve the normal non-NULL path.
set -euo pipefail
cd "$(dirname -- "${BASH_SOURCE[0]}")/.."
[[ "${GITHUB_ACTIONS:-}" == true && -n "${GITHUB_ENV:-}" && -n "${RUNNER_TEMP:-}" ]] || {
    printf 'This script is restricted to disposable GitHub Actions runners.\n' >&2
    exit 2
}
for tool in cc git autoconf make python3 timeout; do
    command -v "$tool" >/dev/null || { printf 'Required allocator test tool: %s\n' "$tool" >&2; exit 1; }
done
ulimit -c 0
source_dir="$(mktemp -d "$RUNNER_TEMP/jemalloc-source.XXXXXX")"
trap 'rm -rf -- "$source_dir"' EXIT
probe="$source_dir/compat"
cc -std=c11 -O2 -Wall -Wextra -Werror -fno-builtin \
    tests/jemalloc_compat.c -Wl,--no-as-needed -ljemalloc -o "$probe"
if timeout --kill-after=2s 10s "$probe"; then
    printf 'Installed jemalloc satisfies the native C23 contract.\n'
    exit 0
fi
printf 'Installed jemalloc failed the contract; building the pinned CI compatibility runtime.\n'
revision=81034ce1f1373e37dc865038e1bc8eeecf559ce8
prefix="$RUNNER_TEMP/biglinux-jemalloc-5.3.1-c23"
git -C "$source_dir" init -q
git -C "$source_dir" remote add origin https://github.com/jemalloc/jemalloc.git
git -C "$source_dir" fetch -q --depth=1 origin "$revision"
git -C "$source_dir" checkout -q --detach FETCH_HEAD
[[ "$(git -C "$source_dir" rev-parse HEAD)" == "$revision" ]]
python3 - "$source_dir/src/jemalloc.c" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
for signature in (
    "je_free_sized(void *ptr, size_t size) {\n",
    "je_free_aligned_sized(void *ptr, size_t alignment, size_t size) {\n",
):
    if text.count(signature) != 1:
        raise SystemExit("Pinned allocator source does not match the reviewed C23 entry point")
    text = text.replace(signature, signature + "\tif (ptr == NULL) {\n\t\treturn;\n\t}\n", 1)
path.write_text(text, encoding="utf-8")
PY
git -C "$source_dir" diff --check
git -C "$source_dir" diff --stat
(
    cd "$source_dir"
    ./autogen.sh --prefix="$prefix" --with-version="5.3.1-0-g$revision" > configure-ci.log 2>&1 || {
        cat configure-ci.log >&2; exit 1;
    }
    make -j2 > build-ci.log 2>&1 || { tail -100 build-ci.log >&2; exit 1; }
    make install > install-ci.log 2>&1 || { cat install-ci.log >&2; exit 1; }
)
cc -std=c11 -O2 -Wall -Wextra -Werror -fno-builtin \
    tests/jemalloc_compat.c -L"$prefix/lib" -Wl,-rpath,"$prefix/lib" \
    -Wl,--no-as-needed -ljemalloc -o "$probe"
timeout --kill-after=2s 10s "$probe"
# These paths last for this job only; never change /usr or a user's ld.so cache.
{
    printf 'LIBRARY_PATH=%s/lib%s\n' "$prefix" "${LIBRARY_PATH:+:$LIBRARY_PATH}"
    printf 'LD_LIBRARY_PATH=%s/lib%s\n' "$prefix" "${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    printf 'PKG_CONFIG_PATH=%s/lib/pkgconfig%s\n' "$prefix" "${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
} >> "$GITHUB_ENV"
printf 'Validated CI allocator source: %s plus the two displayed NULL guards.\n' "$revision"
