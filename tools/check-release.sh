#!/usr/bin/env bash
# Sequential offline checks. Outputs are evidence, not release certification.
set -uo pipefail
ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P) || exit 1
cd "$ROOT" || exit 1
OUT_BASE=${BIGMIC_CHECK_OUTPUT:-$ROOT/.release-check}
mkdir -p "$OUT_BASE" || exit 1
OUT=$(mktemp -d "$OUT_BASE/run-$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX") || exit 1
printf 'Evidence: %s\n' "$OUT"
exec 9>"$ROOT/.release-gate.lock"
flock -n 9 || { printf 'ENV: another release gate is active.\n' >&2; exit 125; }
MODE=full
if [[ ${1:-} == --ui-only && $# == 1 ]]; then
    MODE=ui-only
elif (( $# != 0 )); then
    printf 'Usage: tools/check-release.sh [--ui-only]\n' >&2
    exit 2
fi
PY=("${BIGMIC_PYTHON:-python3}")
[[ ${BIGMIC_PYTHON_NO_SITE:-0} != 1 ]] || PY+=(-S)
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}
export CARGO_INCREMENTAL=0 CARGO_NET_OFFLINE=true
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PYTHONDONTWRITEBYTECODE=1 PYTEST_DISABLE_PLUGIN_AUTOLOAD=1
export PYTHONPATH="$ROOT/app${PYTHONPATH:+:$PYTHONPATH}"
export GSETTINGS_BACKEND=memory GSK_RENDERER=${GSK_RENDERER:-cairo}
printf 'stage\tresult\trc\n' > "$OUT/status.tsv"
FAILED=0
run_stage() {
    local name=$1 seconds=$2 rc
    shift 2
    "${PY[@]}" tools/run-bounded.py --log "$OUT/$name.log" --seconds "$seconds" -- "$@"
    rc=$?
    if (( rc == 0 )); then
        printf '%s\tPASS\t0\n' "$name" >> "$OUT/status.tsv"
    else
        printf '%s\tFAIL\t%s\n' "$name" "$rc" >> "$OUT/status.tsv"
        FAILED=1
    fi
    return "$rc"
}
{
    printf 'mode=%s\n' "$MODE"
    "${PY[@]}" --version
    git rev-parse HEAD 2>/dev/null || true
    git status --short 2>/dev/null || true
    for field in memory.max memory.current memory.events; do
        printf '\n%s\n' "$field"; cat "/sys/fs/cgroup/$field" 2>/dev/null || true
    done
    df -h "$OUT"
} > "$OUT/environment.txt"
run_stage python-syntax 30 "${PY[@]}" -c 'import ast,pathlib; paths=[p for d in ("app","tools") for p in pathlib.Path(d).rglob("*.py") if "__pycache__" not in p.parts]; [ast.parse(p.read_text(encoding="utf-8"),filename=str(p)) for p in paths]; print(len(paths), "Python sources parsed")'
run_stage python-tests 90 dbus-run-session -- xvfb-run -a "${PY[@]}" -m pytest -q app/tests
run_stage translations 60 "${PY[@]}" tools/validate-translations.py --report "$OUT/translations.json"
run_stage desktop 20 desktop-file-validate usr/share/applications/br.com.biglinux.microphone.desktop
run_stage appstream 20 appstreamcli validate --no-net usr/share/metainfo/br.com.biglinux.microphone.metainfo.xml
run_stage shell-syntax 20 bash -c 'for f in pkgbuild/PKGBUILD tools/*.sh; do bash -n "$f" || exit; done'
run_stage shellcheck 20 shellcheck -S warning -e SC2034 pkgbuild/PKGBUILD tools/check-release.sh tools/update-translations.sh
if [[ $MODE == full ]]; then
    run_stage rust-format 30 cargo fmt --all -- --check
    if run_stage cargo-resolution 60 cargo metadata --format-version 1 --locked --offline; then
        if run_stage rust-check 900 cargo check --all-targets --locked --offline; then
            run_stage rust-clippy 900 cargo clippy --all-targets --locked --offline -- -D warnings
            run_stage rust-lib 900 cargo test --lib --locked --offline
            for bin in biglinux-microphone-cli biglinux-microphone-pwloader biglinux-microphone-probe; do
                run_stage "rust-$bin" 900 cargo test --bin "$bin" --locked --offline
            done
            for test in tests/*.rs; do
                target=${test##*/}; target=${target%.rs}
                run_stage "rust-$target" 900 cargo test --test "$target" --locked --offline
            done
            run_stage rust-doc 300 cargo test --doc --locked --offline
        else
            printf 'rust-dependent-tests\tNOT_RUN\trust-check failed\n' >> "$OUT/status.tsv"
        fi
    else
        printf 'rust-build-clippy-tests\tNOT_RUN\tdependency resolution failed\n' >> "$OUT/status.tsv"
    fi
fi
printf 'mode=%s failed=%s; hardware and visual acceptance remain separate\n' "$MODE" "$FAILED" | tee "$OUT/result.txt"
exit "$FAILED"
