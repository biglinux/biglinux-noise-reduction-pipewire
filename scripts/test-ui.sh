#!/usr/bin/env bash
# Exercise the Python/Adw front-end's rendered control tree in a headless
# session. The old Rust `ui::tests::gtk_*` contracts are gone with the Rust GUI;
# the screen is now app/ (Python), so app/tests is the UI gate. The backend is
# stubbed inside the tests, so nothing here touches PipeWire, systemd or audio.
set -euo pipefail
cd "$(dirname -- "${BASH_SOURCE[0]}")/.."

# CI passes a Rust feature flag for the native job; the Python UI has no such
# build split, so accept and ignore it rather than fail the shared invocation.
case "${1:-}" in
    ''|--no-default-features|--all-features) ;;
    *) printf 'Usage: %s [--no-default-features|--all-features]\n' "$0" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] || { printf 'Unexpected arguments\n' >&2; exit 2; }

for tool in python3 xvfb-run timeout; do
    command -v "$tool" >/dev/null || { printf 'Required UI test tool: %s\n' "$tool" >&2; exit 1; }
done

# A bare xvfb server has no GL or portals: force the software renderer and the
# X11 backend, and keep everything in C locale so title assertions see msgids.
# The tests stub the backend and never touch the session bus, so no dbus here.
export PYTHONPATH="app${PYTHONPATH:+:$PYTHONPATH}"
# The window remembers one preference of its own (ui.ini). Point the config
# directory at a throwaway one so a test run never reads — or rewrites — the
# settings of the person running it.
XDG_CONFIG_HOME="$(mktemp -d)"
export XDG_CONFIG_HOME
trap 'rm -rf -- "$XDG_CONFIG_HOME"' EXIT
export LANG=C.UTF-8 LC_ALL=C.UTF-8 LANGUAGE=C
export GDK_BACKEND=x11 GSK_RENDERER=cairo

timeout --kill-after=5s 120s xvfb-run -a \
    python3 -m unittest discover -s app/tests -t app -p 'test_*.py' -v
