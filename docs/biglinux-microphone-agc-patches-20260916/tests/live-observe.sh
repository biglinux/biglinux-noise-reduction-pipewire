#!/usr/bin/env bash
# Read-only control observer. Does not record PCM, change volume, or start services.
set -euo pipefail
export LC_ALL=C
NODE=${1:?Usage: live-observe.sh NODE_ID [DURATION_SECONDS=120] [USER_SERVICE_UNIT]}
SECONDS_TO_RUN=${2:-120}
UNIT=${3:-}
[[ "$NODE" =~ ^[0-9]+$ ]] || { echo 'NODE_ID must be an explicit numeric wpctl node ID.' >&2; exit 2; }
[[ "$SECONDS_TO_RUN" =~ ^[0-9]+$ && "$SECONDS_TO_RUN" -ge 1 && "$SECONDS_TO_RUN" -le 7200 ]] || {
    echo 'Duration must be 1..7200 seconds.' >&2; exit 2;
}
command -v wpctl >/dev/null
command -v timeout >/dev/null
OUT=$(mktemp -d "${TMPDIR:-/tmp}/biglinux-agc-live.XXXXXX")
echo "Results: $OUT"
echo 'This records device names, volume/mute values and optional service logs, not audio.'
echo 'Review/redact device identifiers and logs before sharing.'
{
    date --iso-8601=seconds
    uname -srmo
    for tool in pipewire wireplumber wpctl pw-cat; do
        command -v "$tool" >/dev/null && timeout 3 "$tool" --version || true
    done
    timeout 5 wpctl inspect "$NODE" || true
    timeout 5 wpctl status -n || true
} > "$OUT/environment.txt" 2>&1
printf 'epoch_seconds\trequested_node\twpctl_readback\n' > "$OUT/volume.tsv"
JOURNAL_PID=''
cleanup() {
    if [[ -n "$JOURNAL_PID" ]]; then
        kill "$JOURNAL_PID" 2>/dev/null || true
        wait "$JOURNAL_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
if [[ -n "$UNIT" ]]; then
    timeout "$((SECONDS_TO_RUN + 5))" journalctl --user --unit="$UNIT" \
        --since=now --follow --output=short-iso > "$OUT/agc.log" 2>&1 &
    JOURNAL_PID=$!
fi
END=$((SECONDS + SECONDS_TO_RUN))
while (( SECONDS < END )); do
    if value=$(timeout 3 wpctl get-volume "$NODE" 2>&1); then
        value=${value//$'\n'/ }
    else
        value="READBACK_ERROR ${value//$'\n'/ }"
    fi
    printf '%s\t%s\t%s\n' "$(date +%s.%N)" "$NODE" "$value" >> "$OUT/volume.tsv"
    sleep 0.5
done
cleanup
JOURNAL_PID=''
printf 'Finished. Read-only observations are in: %s\n' "$OUT"
