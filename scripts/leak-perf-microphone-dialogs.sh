#!/usr/bin/env bash
# Headless KWin/AT-SPI leak/perf gate for the reset dialog.
set -euo pipefail

SCRIPT_NAME="${0##*/}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
EVIDENCE_DIR="${OUT_DIR:-${BIGLINUX_UI_EVIDENCE_DIR:-$ROOT_DIR/target/leak-perf/microphone-dialogs}}"
BUILD_PROFILE="${LEAK_BUILD_PROFILE:-debug}"

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
*) fail "unsupported LEAK_BUILD_PROFILE: $BUILD_PROFILE" ;;
esac

[[ -x "$HEADLESS_RUNNER" && -f "$ATSPI_TOOL" ]] ||
	fail "KWin/AT-SPI runner missing under $SKILL_SCRIPT_DIR"

ITERATIONS="${LEAK_ITERS:-12}"
THRESHOLD_KIB="${LEAK_THRESHOLD_KB:-512}"
CLOSE_SLEEP="${LEAK_CLOSE_SLEEP:-0.35}"

SESSION_DIR="$EVIDENCE_DIR/reset-dialog-cycles"
AUTOMATION_SCRIPT="$SESSION_DIR/automation.sh"
SAMPLES_CSV="$SESSION_DIR/samples.csv"
PROBE_RESULT="$SESSION_DIR/probe-result.txt"
PROBE_RESULTS_JSON="$SESSION_DIR/probe-results.json"
SUMMARY_TSV="$EVIDENCE_DIR/summary.tsv"

mkdir -p "$SESSION_DIR"
printf 'cargo build --manifest-path %q --locked --bin biglinux-microphone %s\n' \
	"$ROOT_DIR/Cargo.toml" "${build_arguments[*]:-}" >"$EVIDENCE_DIR/command.txt"

cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --locked --bin biglinux-microphone "${build_arguments[@]}"

cat >"$AUTOMATION_SCRIPT" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

ATSPI="${ATSPI_TOOL:?}"
BINARY_PATH="${BINARY_PATH:?}"
ITERATIONS="${ITERATIONS:?}"
SAMPLES_CSV="${SAMPLES_CSV:?}"
CLOSE_SLEEP="${CLOSE_SLEEP:?}"
FINAL_LAYOUT="${FINAL_LAYOUT:?}"
FINAL_TARGETS="${FINAL_TARGETS:?}"
FINAL_TREE="${FINAL_TREE:?}"

application_pid="$(pgrep -n -f -- "$BINARY_PATH" || true)"
[[ -n "$application_pid" ]] || {
	echo "no Filter noise pid for $BINARY_PATH" >&2
	exit 1
}

anonymous_rss_kib() {
	awk '$1=="Anonymous:"{print $2; found=1} END{if(!found) print 0}' "/proc/$application_pid/smaps_rollup" 2>/dev/null
}

sample_rss() {
	printf '%s,%s,%s\n' "$1" "$(date +%s%3N)" "$(anonymous_rss_kib)" >>"$SAMPLES_CSV"
}

open_reset_dialog() {
	python3 "$ATSPI" wait-act-first --query "=Main menu" --role button --timeout-ms 10000 >/dev/null
	python3 "$ATSPI" wait-act-first --query "=Restore default settings" --role "menu item" --timeout-ms 10000 >/dev/null
	python3 "$ATSPI" wait-for --query "=Restore default settings?" --timeout-ms 10000 >/dev/null
}

close_reset_dialog() {
	python3 "$ATSPI" wait-act-first --query "=Cancel" --role button --timeout-ms 10000 >/dev/null
	python3 "$ATSPI" wait-gone --query "=Restore default settings?" --timeout-ms 10000 >/dev/null
}

: >"$SAMPLES_CSV"
python3 "$ATSPI" wait-for --query "=Filter noise" --role window --timeout-ms 20000 >/dev/null
python3 "$ATSPI" wait-for --query "=Main menu" --role button --timeout-ms 20000 >/dev/null
sample_rss start

for ((iteration = 1; iteration <= ITERATIONS; iteration++)); do
	open_reset_dialog
	sample_rss open
	close_reset_dialog
	sleep "$CLOSE_SLEEP"
	sample_rss closed
done

python3 "$ATSPI" layout --max-depth 30 >"$FINAL_LAYOUT" || true
python3 "$ATSPI" targets --max-depth 30 >"$FINAL_TARGETS" || true
python3 "$ATSPI" dump --max-depth 30 >"$FINAL_TREE" || true
EOF
chmod 700 "$AUTOMATION_SCRIPT"

env \
	ATSPI_TOOL="$ATSPI_TOOL" \
	BINARY_PATH="$binary_path" \
	ITERATIONS="$ITERATIONS" \
	SAMPLES_CSV="$SAMPLES_CSV" \
	CLOSE_SLEEP="$CLOSE_SLEEP" \
	FINAL_LAYOUT="$SESSION_DIR/final.layout.txt" \
	FINAL_TARGETS="$SESSION_DIR/final.targets.txt" \
	FINAL_TREE="$SESSION_DIR/final.tree.txt" \
	PIPEWIRE_REMOTE="biglinux-microphone-smoke-${BASHPID}" \
	BIGLINUX_UI_SESSION_MODE=headless \
	BIGLINUX_VISIBLE_HOST_DISPLAY_FORBIDDEN=1 \
	AUDIT_APP=biglinux-microphone \
	"$HEADLESS_RUNNER" \
	--compositor kwin \
	--log-dir "$SESSION_DIR/session" \
	--wait-ready-query "=Filter noise" \
	--inspect-atspi \
	--audit \
	--diagnostic-no-screenshot \
	--automation-file "$AUTOMATION_SCRIPT" \
	--automation-delay 0 \
	--dump-before "$SESSION_DIR/before.tree.txt" \
	--dump-after "$SESSION_DIR/after.tree.txt" \
	--dump-diff "$SESSION_DIR/tree.diff" \
	-- "$binary_path"

awk -F, -v threshold="$THRESHOLD_KIB" -v json_path="$PROBE_RESULTS_JSON" '
	$1=="closed" {
		n++;
		x[n]=n;
		y[n]=$3;
		if (n == 1) first=$3;
		last=$3;
	}
	END {
		if (n < 3) {
			printf "INCONCLUSIVE: need >=3 close samples (got %d)\n", n;
			printf "{\"status\":\"inconclusive\",\"closed_samples\":%d}\n", n > json_path;
			exit 2;
		}
		sx=sy=sxx=sxy=0;
		for (i=1; i<=n; i++) {
			sx+=x[i];
			sy+=y[i];
			sxx+=x[i]*x[i];
			sxy+=x[i]*y[i];
		}
		slope=(n*sxy - sx*sy)/(n*sxx - sx*sx);
		half=int(n/2);
		first_average=0;
		last_average=0;
		for (i=2; i<=half; i++) first_average+=y[i]-y[i-1];
		if (half > 1) first_average/=(half-1);
		for (i=half+1; i<=n; i++) last_average+=y[i]-y[i-1];
		if (n-half > 0) last_average/=(n-half);
		printf "anon RSS after close: first=%dKiB last=%dKiB over %d iters\n", first, last, n;
		printf "per-iteration slope = %.0f KiB/open (early avg %.0f, late avg %.0f KiB/open)\n", slope, first_average, last_average;
		if (slope <= threshold) {
			printf "VERDICT: clean (slope %.0f <= %d KiB/open)\n", slope, threshold;
			status="clean";
			exit_code=0;
		} else if (last_average < first_average*0.4) {
			printf "VERDICT: GROWS but DECELERATING (%.0f->%.0f) - likely cache warming\n", first_average, last_average;
			status="cache_warming";
			exit_code=0;
		} else {
			printf "VERDICT: GROWTH %.0f KiB/open (steady)\n", slope;
			status="growth";
			exit_code=3;
		}
		printf "{\"status\":\"%s\",\"closed_samples\":%d,\"first_kib\":%d,\"last_kib\":%d,\"slope_kib_per_open\":%.0f,\"threshold_kib_per_open\":%d}\n", status, n, first, last, slope, threshold > json_path;
		exit exit_code;
	}
' "$SAMPLES_CSV" >"$PROBE_RESULT"

printf 'reset-dialog-cycles\t%s\t%s\n' "$ITERATIONS" "$PROBE_RESULT" >"$SUMMARY_TSV"
printf '%s complete. Artifacts: %s\n' "$SCRIPT_NAME" "$EVIDENCE_DIR" >&2
