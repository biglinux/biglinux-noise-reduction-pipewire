#!/usr/bin/env bash
# Apply only to the exact five modified input files from the supplied snapshot.
# No dependency installation, compilation, service changes, staging or commit.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(realpath "${1:?Usage: apply.sh /path/to/project-or-code [--check]}")
MODE=${2:-apply}
case "$MODE" in apply|--check) ;; *) echo 'Second argument must be --check, or omitted.' >&2; exit 2 ;; esac
command -v git >/dev/null
command -v sha256sum >/dev/null
(cd "$ROOT" && sha256sum --check "$HERE/validation/base-files.sha256")
while IFS= read -r name; do
    [[ ! -e "$ROOT/$name" && ! -L "$ROOT/$name" ]] || {
        echo "Refusing to overwrite an existing added file: $name" >&2; exit 2;
    }
done < "$HERE/validation/new-files.txt"
PATCHES=()
while IFS= read -r name; do
    [[ -n "$name" ]] && PATCHES+=("$HERE/patches/$name")
done < "$HERE/patches/series"
git -C "$ROOT" apply --check --whitespace=error-all "${PATCHES[@]}"
if [[ "$MODE" == --check ]]; then
    echo 'Exact input hashes and full patch applicability: OK. No files changed.'
    exit 0
fi
# git apply rejects an unsuccessful patch set as a whole without --reject.
git -C "$ROOT" apply --whitespace=error-all "${PATCHES[@]}"
(cd "$ROOT" && sha256sum --check "$HERE/validation/patched-files.sha256")
echo 'Patches applied. Review/build/test before enabling the controller.'
