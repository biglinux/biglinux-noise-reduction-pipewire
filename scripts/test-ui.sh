#!/usr/bin/env bash
# A disposable display, bus and XDG tree prevent tests from modifying a
# developer's desktop/audio session. Failure to create them is a test failure.
set -euo pipefail
cd "$(dirname -- "${BASH_SOURCE[0]}")/.."
for tool in cargo xvfb-run dbus-run-session; do
    command -v "$tool" >/dev/null || { printf 'Required UI test tool: %s\n' "$tool" >&2; exit 1; }
done
session="$(mktemp -d)"
trap 'rm -rf -- "$session"' EXIT
mkdir -m 700 "$session/runtime" "$session/config" "$session/cache" "$session/data"
unset DBUS_SESSION_BUS_ADDRESS PULSE_SERVER PIPEWIRE_REMOTE
export XDG_RUNTIME_DIR="$session/runtime" XDG_CONFIG_HOME="$session/config"
export XDG_CACHE_HOME="$session/cache" XDG_DATA_HOME="$session/data"
export DBUS_SYSTEM_BUS_ADDRESS="unix:path=$session/no-system-bus"
export BIGLINUX_UI_SESSION_MODE=disposable
export G_DEBUG=fatal-warnings
export LANG=C.UTF-8 LC_ALL=C.UTF-8 LANGUAGE=C GDK_BACKEND=x11 GSK_RENDERER=cairo
xvfb-run -a dbus-run-session -- cargo test --all-features --locked --lib \
    ui::tests::gtk_display_contracts_cover_dialogs_and_model_picker \
    -- --ignored --exact --nocapture --test-threads=1
