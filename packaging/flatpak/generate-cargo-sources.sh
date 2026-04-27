#!/usr/bin/env bash
# Generate `cargo-sources.json` for offline Flathub builds.
#
# Flathub builders have no network access; every cargo registry
# dependency has to be declared as a `file` source in the manifest.
# `flatpak-cargo-generator` (from flatpak-builder-tools) walks
# `Cargo.lock` and emits a sources list the flatpak-builder JSON loader
# can splice in.
#
# Usage:
#   ./packaging/flatpak/generate-cargo-sources.sh
#
# Output: packaging/flatpak/cargo-sources.json
#
# After generating, reference it from the manifest:
#
#   sources:
#     - type: file
#       path: cargo-sources.json
#       dest-filename: cargo-sources.json
#     - cargo-sources.json   # expanded inline by flatpak-builder
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly repo_root
cd "$repo_root"

readonly generator_url='https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py'
readonly generator_path="${repo_root}/packaging/flatpak/flatpak-cargo-generator.py"

if [ ! -f "$generator_path" ]; then
    echo "==> Fetching flatpak-cargo-generator.py" >&2
    curl --fail --location --output "$generator_path" "$generator_url"
    chmod +x "$generator_path"
fi

if ! command -v python3 >/dev/null 2>&1; then
    printf 'Missing required command: python3\n' >&2
    exit 127
fi

readonly output='packaging/flatpak/cargo-sources.json'
echo "==> Generating $output from Cargo.lock" >&2
python3 "$generator_path" Cargo.lock -o "$output"
echo "==> Done. Reference $output in the Flatpak manifest sources list." >&2
