#!/usr/bin/env bash
# Build/stage the Arch recipe once. Never install it, run package hooks or
# repeat the application's tests. No allocator rebuild or compatibility probe.
# shellcheck source-path=SCRIPTDIR
set -euo pipefail
root=$(cd "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$root"
[[ $# -le 1 ]] || { printf 'Usage: %s [report-directory]\n' "$0" >&2; exit 2; }
report=$(realpath -m -- "${1:-$root/.ci-results/release}")
mkdir -p "$report"
for tool in cargo rsync msgfmt desktop-file-validate appstreamcli python3 readelf ldd git; do
    command -v "$tool" >/dev/null || { printf 'Required package test tool: %s\n' "$tool" >&2; exit 1; }
done
stage=$(mktemp -d)
trap 'status=$?; rm -rf -- "$stage" 2>/dev/null || :; exit "$status"' EXIT
# shellcheck source=../packaging/arch/PKGBUILD
source "$root/packaging/arch/PKGBUILD"
msg2() { printf '%s\n' "$*"; }

# The source fixture contains nested makepkg/custom output directories.
# This exercises prepare() without doing a second application compilation.
(
    _local_root="$stage/local/$pkgname"
    mkdir -p "$_local_root/src" "$_local_root/target" "$_local_root/.git" "$_local_root/build-locale"
    printf 'original\n' > "$_local_root/src/example.rs"
    printf 'generated\n' > "$_local_root/target/do-not-copy"
    for location in packaging/arch/src custom-source; do
        srcdir="$_local_root/$location"
        prepare
        destination="$srcdir/$pkgname"
        cmp "$_local_root/src/example.rs" "$destination/src/example.rs"
        test ! -e "$destination/target"
        test ! -e "$destination/.git"
        test ! -e "$destination/build-locale"
        test ! -e "$destination/$location/$pkgname"
        printf 'stale\n' > "$destination/removed.rs"
        prepare
        test ! -e "$destination/removed.rs"
    done
    # This resolves to the checkout itself without changing the package name.
    srcdir="$stage/local"
    if prepare; then
        printf 'Unsafe source destination was accepted\n' >&2
        exit 1
    fi
    test -f "$_local_root/src/example.rs"
)

# Reuse the normal Cargo workspace and cache. Do not invoke makepkg's
# check(): the native CI job already owns the application's test suites.
srcdir="$stage/source"
pkgdir="$stage/package"
mkdir -p "$srcdir" "$pkgdir"
ln -s "$root" "$srcdir/$pkgname"
export CARGO_TARGET_DIR="$root/target"
build
package

python3 "$root/scripts/verify-package.py" "$root" "$pkgdir" "$report"
desktop-file-validate "$pkgdir/usr/share/applications/br.com.biglinux.microphone.desktop"
appstreamcli validate --no-net "$pkgdir/usr/share/metainfo/br.com.biglinux.microphone.metainfo.xml"
for binary in "$pkgdir"/usr/bin/*; do
    readelf -h "$binary" >/dev/null
    dependencies=$(ldd "$binary")
    printf '\n%s\n%s\n' "${binary#"$pkgdir/"}" "$dependencies" >> "$report/linkage.txt"
    if grep -q 'not found' <<< "$dependencies"; then
        printf 'Missing runtime library: %s\n' "$binary" >&2
        exit 1
    fi
done
# CLI help is independent of the GUI allocator and the user's audio session.
"$pkgdir/usr/bin/biglinux-microphone-cli" help > "$report/cli-help.txt"
{
    git -c safe.directory="$root" rev-parse HEAD
    rustc --version
    pkg-config --modversion gtk4 libadwaita-1 libpipewire-0.3
} > "$report/tested-build.txt"
printf 'Recipe build, staged payload, metadata, catalogs and linkage passed.\n' | tee "$report/result.txt"
