#!/usr/bin/env bash
# Native graph tests must never connect to the developer's audio session.
set -euo pipefail
cd "$(dirname -- "${BASH_SOURCE[0]}")/.."
features=(--all-features)
case "${1:-}" in
    '') ;;
    --no-default-features) features=(--no-default-features) ;;
    *) printf 'Usage: %s [--no-default-features]\n' "$0" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] || { printf 'Unexpected arguments\n' >&2; exit 2; }
for tool in cargo pipewire pw-dump dbus-run-session timeout; do
    command -v "$tool" >/dev/null || { printf 'Required audio test tool: %s\n' "$tool" >&2; exit 1; }
done
# Compile before entering the private session; no daemon is started by this step.
# The same feature set is used for compilation and execution.
cargo test "${features[@]}" --locked --test pipewire_runtime --no-run
session="$(mktemp -d)"
# A daemon child still writing into the runtime directory can fail the removal.
# Under `set -e` that would replace the fixture's own result with a cleanup error.
trap 'status=$?; rm -rf -- "$session" 2>/dev/null || :; exit "$status"' EXIT
mkdir -m 700 "$session/runtime" "$session/config" "$session/cache" "$session/data"
: > "$session/runtime/biglinux-test-session"
unset DBUS_SESSION_BUS_ADDRESS PULSE_SERVER PIPEWIRE_REMOTE
unset PIPEWIRE_CONFIG_DIR PIPEWIRE_CONFIG_PREFIX PIPEWIRE_CONFIG_NAME
export XDG_RUNTIME_DIR="$session/runtime" XDG_CONFIG_HOME="$session/config"
export XDG_CACHE_HOME="$session/cache" XDG_DATA_HOME="$session/data"
export PIPEWIRE_RUNTIME_DIR="$session/runtime" PIPEWIRE_REMOTE=pipewire-0
export PULSE_RUNTIME_PATH="$session/runtime/pulse"
export DBUS_SYSTEM_BUS_ADDRESS="unix:path=$session/no-system-bus"
export BIGLINUX_AUDIO_SESSION_MODE=disposable
export LANG=C.UTF-8 LC_ALL=C.UTF-8
# Each child is reaped by the Rust fixture; this process-group deadline also
# covers an unexpected hang in a native module. It does not touch user services.
timeout --kill-after=5s 120s dbus-run-session -- \
    cargo test "${features[@]}" --locked --test pipewire_runtime \
    -- --ignored --exact generated_graphs_load_and_recover_independently \
    --nocapture --test-threads=1
