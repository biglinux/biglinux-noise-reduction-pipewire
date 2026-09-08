#!/usr/bin/env bash
# KWin/AT-SPI smoke for the BigLinux microphone control window.
set -euo pipefail

SCRIPT_NAME="${0##*/}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
EVIDENCE_DIR="${OUT_DIR:-${BIGLINUX_UI_EVIDENCE_DIR:-$ROOT_DIR/target/ui-smoke/microphone-window}}"
BUILD_PROFILE="${UI_BUILD_PROFILE:-debug}"

SKILL_SCRIPT_DIR="${ATSPI_SKILL_DIR:-$HOME/.agents/skills/linux-ui-a11y/scripts}"
HEADLESS_RUNNER="$SKILL_SCRIPT_DIR/run_headless_wayland_atspi.sh"
ATSPI_TOOL="$SKILL_SCRIPT_DIR/atspi_tool.py"

fail() {
	printf '%s: %s\n' "$SCRIPT_NAME" "$*" >&2
	exit 1
}

case "$BUILD_PROFILE" in
release)
	build_arguments=(--release)
	binary_path="$ROOT_DIR/target/release/biglinux-microphone"
	;;
debug)
	build_arguments=()
	binary_path="$ROOT_DIR/target/debug/biglinux-microphone"
	;;
*) fail "unsupported UI_BUILD_PROFILE: $BUILD_PROFILE" ;;
esac

[[ -x "$HEADLESS_RUNNER" && -f "$ATSPI_TOOL" ]] ||
	fail "KWin/AT-SPI runner missing under $SKILL_SCRIPT_DIR"

mkdir -p "$EVIDENCE_DIR"
printf 'cargo build --manifest-path %q --locked --bin biglinux-microphone %s\n' \
	"$ROOT_DIR/Cargo.toml" "${build_arguments[*]:-}" >"$EVIDENCE_DIR/command.txt"

cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --locked --bin biglinux-microphone "${build_arguments[@]}"

run_surface() {
	local surface_name="$1"
	local automation_body="$2"
	local surface_dir="$EVIDENCE_DIR/$surface_name"
	local automation_file="$surface_dir/automation.sh"

	mkdir -p "$surface_dir"
	{
		printf '#!/usr/bin/env bash\n'
		printf 'set -euo pipefail\n'
		printf 'ATSPI=%q\n' "$ATSPI_TOOL"
		printf '%s\n' "$automation_body"
		# shellcheck disable=SC2016
		printf 'python3 "$ATSPI" layout --max-depth 30 >%q || true\n' "$surface_dir/final.layout.txt"
		# shellcheck disable=SC2016
		printf 'python3 "$ATSPI" targets --max-depth 30 >%q || true\n' "$surface_dir/final.targets.txt"
		# shellcheck disable=SC2016
		printf 'python3 "$ATSPI" dump --max-depth 30 >%q || true\n' "$surface_dir/final.tree.txt"
	} >"$automation_file"
	chmod 700 "$automation_file"

	# Isolated per-surface config: the app under test reads/writes
	# settings.json via XDG_CONFIG_HOME, so without this the smoke
	# mutates the developer's real ~/.config/biglinux-microphone (state
	# leaks between surfaces/runs — e.g. a persisted Advanced flag
	# breaks the advanced-tabs surface on the next run).
	local config_home="$surface_dir/config"
	mkdir -p "$config_home"
	env \
		XDG_CONFIG_HOME="$config_home" \
		PIPEWIRE_REMOTE="biglinux-microphone-smoke-${BASHPID}" \
		BIGLINUX_UI_SESSION_MODE=headless \
		BIGLINUX_VISIBLE_HOST_DISPLAY_FORBIDDEN=1 \
		AUDIT_APP=biglinux-microphone \
		"$HEADLESS_RUNNER" \
		--compositor kwin \
		--log-dir "$surface_dir/session" \
		--screenshot "$surface_dir/$surface_name.png" \
		--wait-ready-query "=Filter noise" \
		--inspect-atspi \
		--audit \
		--automation-file "$automation_file" \
		--automation-delay 1 \
		--dump-before "$surface_dir/before.tree.txt" \
		--dump-after "$surface_dir/after.tree.txt" \
		--dump-diff "$surface_dir/tree.diff" \
		-- "$binary_path"

	file "$surface_dir/$surface_name.png" | grep -q '1280 x 720' ||
		fail "expected 1280x720 screenshot for $surface_name"
	[[ -s "$surface_dir/$surface_name.grid.png" ]] ||
		fail "missing grid screenshot for $surface_name"
}

# shellcheck disable=SC2016
run_surface main-window '
python3 "$ATSPI" wait-for --query "=Advanced" --role "switch" --timeout-ms 20000 >/dev/null
python3 "$ATSPI" wait-for --query "=Main menu" --role "button" --timeout-ms 20000 >/dev/null
'

# shellcheck disable=SC2016
run_surface main-menu '
python3 "$ATSPI" wait-act-first --query "=Main menu" --role "button" --timeout-ms 20000 >/dev/null
python3 "$ATSPI" wait-for --query "=Restore default settings" --role "button" --timeout-ms 10000 >/dev/null
python3 "$ATSPI" wait-for --query "=About Filter noise" --role "button" --timeout-ms 10000 >/dev/null
'

# shellcheck disable=SC2016
run_surface advanced-tabs '
python3 "$ATSPI" wait-act-first --query "=Advanced" --role "switch" --timeout-ms 20000 >/dev/null
python3 "$ATSPI" wait-for --query "=Microphone" --timeout-ms 10000 >/dev/null
python3 "$ATSPI" wait-for --query "=Output filter" --timeout-ms 10000 >/dev/null
python3 "$ATSPI" wait-for --query "=Tuning" --timeout-ms 10000 >/dev/null
'

# shellcheck disable=SC2016
run_surface reset-dialog '
python3 "$ATSPI" wait-act-first --query "=Main menu" --role "button" --timeout-ms 20000 >/dev/null
python3 "$ATSPI" wait-act-first --query "=Restore default settings" --role "button" --timeout-ms 10000 >/dev/null
python3 "$ATSPI" wait-for --query "=Restore default settings?" --role "dialog" --timeout-ms 10000 >/dev/null
python3 "$ATSPI" wait-for --query "=Restore defaults" --role "button" --timeout-ms 10000 >/dev/null
'

printf 'microphone-window\tOK\t%s\n' "$EVIDENCE_DIR" >"$EVIDENCE_DIR/summary.tsv"
printf '%s complete. Artifacts: %s\n' "$SCRIPT_NAME" "$EVIDENCE_DIR" >&2
