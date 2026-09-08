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

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly script_dir
repo_root="$(cd "${script_dir}/../.." && pwd)"
readonly repo_root

for command in curl python3 sha256sum; do
	if ! command -v "$command" >/dev/null 2>&1; then
		printf 'Missing required command: %s\n' "$command" >&2
		exit 127
	fi
done

readonly generator_commit='737c0085912f9f7dabf9341d4608e2a77a51a73a'
readonly generator_sha256='b373c8ab1a05378ec5d8ed0645c7b127bcec7d2f7a1798694fbc627d570d856c'
readonly generator_url="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/${generator_commit}/cargo/flatpak-cargo-generator.py"
generator_path="$(mktemp "${TMPDIR:-/tmp}/flatpak-cargo-generator.XXXXXX.py")"
readonly generator_path
trap 'rm -f "$generator_path"' EXIT

echo "==> Fetching pinned flatpak-cargo-generator.py (${generator_commit})" >&2
curl --fail --location --proto '=https' --tlsv1.2 \
	--output "$generator_path" "$generator_url"
printf '%s  %s\n' "$generator_sha256" "$generator_path" | sha256sum --check --status

readonly output="${script_dir}/cargo-sources.json"
echo "==> Generating $output from Cargo.lock" >&2
python3 "$generator_path" "${repo_root}/Cargo.lock" -o "$output"
echo "==> Done. Reference $output in the Flatpak manifest sources list." >&2
