#!/usr/bin/env bash
# Build once, then isolate each GTK contract in its own process and session.
# Even --test-threads=1 does not reuse the initializing thread between tests.
set -euo pipefail
cd "$(dirname -- "${BASH_SOURCE[0]}")/.."
features=(--all-features)
case "${1:-}" in
    '') ;;
    --no-default-features) features=(--no-default-features) ;;
    *) printf 'Usage: %s [--no-default-features]\n' "$0" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] || { printf 'Unexpected arguments\n' >&2; exit 2; }
for tool in cargo python3 xvfb-run dbus-run-session timeout; do
    command -v "$tool" >/dev/null || { printf 'Required UI test tool: %s\n' "$tool" >&2; exit 1; }
done
suite="$(mktemp -d)"
# A DBus-activated portal child can repopulate the directory while it is being
# removed. Under `set -e` a failing cleanup would replace the suite's own result.
trap 'status=$?; rm -rf -- "$suite" 2>/dev/null || :; exit "$status"' EXIT
cargo test "${features[@]}" --locked --lib --no-run --message-format=json > "$suite/build.jsonl"
python3 - "$suite/build.jsonl" > "$suite/executable" <<'PY'
import json
import sys
from pathlib import Path

executables = set()
for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines():
    item = json.loads(line)
    if (item.get("reason") == "compiler-artifact"
            and item.get("target", {}).get("name") == "biglinux_microphone"
            and item.get("profile", {}).get("test") and item.get("executable")):
        executables.add(item["executable"])
if len(executables) != 1:
    raise SystemExit(f"Expected one library test executable, got {executables!r}")
print(executables.pop())
PY
IFS= read -r executable < "$suite/executable"
"$executable" --ignored --list > "$suite/tests"
count=0
failed=0
while IFS= read -r line; do
    [[ "$line" == ui::tests::gtk_*': test' ]] || continue
    test_name="${line%: test}"
    count=$((count + 1))
    session="$suite/case-$count"
    mkdir -m 700 "$session"
    mkdir -m 700 "$session/runtime" "$session/config" "$session/cache" "$session/data"
    printf '\n=== GTK contract %s ===\n' "$test_name"
    if (
        unset DBUS_SESSION_BUS_ADDRESS PULSE_SERVER G_DEBUG
        unset PIPEWIRE_CONFIG_DIR PIPEWIRE_CONFIG_PREFIX PIPEWIRE_CONFIG_NAME
        export XDG_RUNTIME_DIR="$session/runtime" XDG_CONFIG_HOME="$session/config"
        export XDG_CACHE_HOME="$session/cache" XDG_DATA_HOME="$session/data"
        export PIPEWIRE_RUNTIME_DIR="$session/runtime" PIPEWIRE_REMOTE=pipewire-0
        export PULSE_RUNTIME_PATH="$session/runtime/pulse"
        export DBUS_SYSTEM_BUS_ADDRESS="unix:path=$session/no-system-bus"
        export BIGLINUX_UI_SESSION_MODE=disposable
        export LANG=C.UTF-8 LC_ALL=C.UTF-8 LANGUAGE=C GDK_BACKEND=x11 GSK_RENDERER=cairo
        # The application's warnings remain fatal, not the unrelated portal
        # services activated by DBus in a container without a document mount.
        timeout --kill-after=5s 60s xvfb-run -a dbus-run-session -- \
            env G_DEBUG=fatal-warnings "$executable" "$test_name" \
            --ignored --exact --nocapture --test-threads=1
    ); then
        printf 'PASS: %s\n' "$test_name"
    else
        status=$?
        printf 'FAIL: %s (exit %s)\n' "$test_name" "$status" >&2
        failed=$((failed + 1))
    fi
done < "$suite/tests"
[[ $count -gt 0 ]] || { printf 'No GTK contracts were discovered\n' >&2; exit 1; }
printf '\nGTK contracts: %s passed, %s failed, %s total\n' "$((count - failed))" "$failed" "$count"
[[ $failed -eq 0 ]]
