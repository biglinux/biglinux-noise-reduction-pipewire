#!/usr/bin/env bash
# Canonical quality gate for biglinux-microphone (Rust rewrite).
# Run from project root: ./scripts/quality-check.sh
# Options:
#   --fix   Apply autofixes where possible (fmt only)
#   --full  Include slower local-only checks
#   --ci    Fail rather than skip a required portable gate; cloud security
#           checks and the isolated GUI job run separately in GitHub Actions.
#
# shellcheck disable=SC2059
# We embed ANSI color escape variables directly in printf format
# strings. SC2059 flags this, but the variables are static color codes
# (no user data), so the warning is noise here.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

PASS=0
FAIL=0
SKIP=0
CHECKS=()
RESULTS=()

FIX=false
FULL=false
CI=false
for arg in "$@"; do
    case "$arg" in
        --fix)  FIX=true  ;;
        --full) FULL=true ;;
        --ci)   CI=true   ;;
        --help|-h)
            echo "Usage: $0 [--fix] [--full] [--ci]"
            echo "  --fix   Apply autofixes where possible"
            echo "  --full  Include slower local-only checks"
            echo "  --ci    Fail when a required portable check cannot run"
            exit 0
            ;;
        *) echo "Unknown argument: $arg" >&2; exit 1 ;;
    esac
done

has_cmd() { command -v "$1" &>/dev/null; }

run_check() {
    local name="$1"
    shift
    CHECKS+=("$name")
    printf "\n${BLUE}━━━ ${BOLD}%s${NC}\n" "$name"
    if "$@"; then
        printf "${GREEN}  ✓ passed${NC}\n"
        RESULTS+=("pass")
        PASS=$((PASS + 1))
    else
        printf "${RED}  ✗ FAILED${NC}\n"
        RESULTS+=("fail")
        FAIL=$((FAIL + 1))
    fi
}

skip_check() {
    local name="$1"
    local reason="${2:-not installed}"
    if $CI; then
        CHECKS+=("$name")
        RESULTS+=("fail")
        FAIL=$((FAIL + 1))
        printf 'Required check unavailable: %s (%s)\n' "$name" "$reason" >&2
        return
    fi
    CHECKS+=("$name")
    printf "\n${YELLOW}━━━ ${BOLD}%s${NC} — ${DIM}skipped (%s)${NC}\n" "$name" "$reason"
    RESULTS+=("skip")
    SKIP=$((SKIP + 1))
}

printf "\n${CYAN}╔══════════════════════════════════════════════════════╗${NC}\n"
printf "${CYAN}║${NC}  ${BOLD}biglinux-microphone quality check${NC}                    ${CYAN}║${NC}\n"
if $FIX;  then printf "${CYAN}║${NC}  mode: ${GREEN}--fix${NC}                                      ${CYAN}║${NC}\n"; fi
if $FULL; then printf "${CYAN}║${NC}  mode: ${GREEN}--full${NC}                                     ${CYAN}║${NC}\n"; fi
if $CI;   then printf "${CYAN}║${NC}  mode: ${RED}--ci${NC} (strict)                               ${CYAN}║${NC}\n"; fi
printf "${CYAN}╚══════════════════════════════════════════════════════╝${NC}\n"

# 1) Formatting
if $FIX; then
    run_check "rustfmt (fix)" cargo fmt --all
else
    run_check "rustfmt" cargo fmt --all --check
fi

# 2) Clippy (strict)
run_check "clippy (strict)" cargo clippy --all-targets --all-features --locked -- -D warnings

# 3) Tests
run_check "tests (unit, integration and doctests)" cargo test --all-features --locked

# 4) cargo-deny
if has_cmd cargo-deny; then
    run_check "cargo deny" cargo deny check
else
    skip_check "cargo deny" "install: cargo install cargo-deny"
fi

# 5) cargo-machete
# `--with-metadata` resolves crates renamed in their own Cargo.toml;
# without it the vendored big-relm4-components trips the gettext-rs /
# `gettextrs` false positive. Matches the CI invocation.
if has_cmd cargo-machete; then
    run_check "cargo machete" cargo machete --with-metadata
else
    skip_check "cargo machete" "install: cargo install cargo-machete"
fi

# 6) Cyclomatic complexity (lizard)
CCN_THRESHOLD=${CCN_THRESHOLD:-25}
if has_cmd lizard; then
    run_check "cyclomatic complexity (CCN ≤ $CCN_THRESHOLD)" \
        lizard src/ -l rust -C "$CCN_THRESHOLD" -w
else
    skip_check "lizard (CC)" "install: pipx install lizard"
fi

# 7) Duplicate-code detection (jscpd)
if has_cmd jscpd; then
    run_check "duplicate code (jscpd)" \
        jscpd --min-lines 50 --min-tokens 100 --threshold 5 \
              --reporters console --silent src/
else
    skip_check "jscpd (dup)" "install: npm install -g jscpd"
fi

# Translation validation includes source coverage, not just PO syntax.
if has_cmd xtr && has_cmd xgettext && has_cmd msgcat && has_cmd msgcmp && has_cmd msgmerge && has_cmd msgfmt; then
    run_check "translation extraction coverage" bash scripts/refresh-pot.sh --check
    for catalog in po/*.po; do
        run_check "catalog: $catalog" msgfmt --check --check-header -o /dev/null "$catalog"
    done
else
    skip_check "gettext source coverage" "install gettext and xtr"
fi

# ── --full extras ──────────────────────────────────────────────────
if $FULL; then
    if has_cmd typos; then
        if $FIX; then
            run_check "typos (fix)" typos --write-changes src/
        else
            run_check "typos" typos src/
        fi
    else
        skip_check "typos" "install: cargo install typos-cli"
    fi

    if has_cmd cargo-audit; then
        run_check "cargo audit (RustSec)" cargo audit
    else
        skip_check "cargo audit" "install: cargo install cargo-audit --locked"
    fi

    if has_cmd gitleaks; then
        run_check "gitleaks (secret scan)" gitleaks detect --no-banner --redact
    else
        skip_check "gitleaks" "install: see gitleaks.io"
    fi

    # Miri — only pure-logic config submodules. config::paths::tests
    # calls `dirs::config_dir` → `getpwuid_r` FFI which miri cannot
    # simulate, so it is excluded by listing the safe modules explicitly.
    if rustup toolchain list | grep -q nightly \
       && rustup component list --toolchain nightly 2>/dev/null | grep -q 'miri.*installed'; then
        run_check "miri (pure data-model contracts)" bash scripts/test-miri.sh
    else
        skip_check "miri" "install: rustup +nightly component add miri"
    fi
fi

# ── Summary ────────────────────────────────────────────────────────
TOTAL=$((PASS + FAIL + SKIP))
printf "\n${CYAN}══════════════════════════════════════════════════════════${NC}\n"
printf "${BOLD}  Quality Check Summary${NC}\n"
printf "${CYAN}──────────────────────────────────────────────────────────${NC}\n"
for i in "${!CHECKS[@]}"; do
    case "${RESULTS[$i]}" in
        pass) icon="${GREEN}✓${NC}" ;;
        fail) icon="${RED}✗${NC}" ;;
        skip) icon="${YELLOW}○${NC}" ;;
    esac
    printf "  %b  %s\n" "$icon" "${CHECKS[$i]}"
done
printf "${CYAN}──────────────────────────────────────────────────────────${NC}\n"
printf "  ${GREEN}%d passed${NC}  ${RED}%d failed${NC}  ${YELLOW}%d skipped${NC}  (${DIM}%d total${NC})\n\n" \
    "$PASS" "$FAIL" "$SKIP" "$TOTAL"

if [[ $FAIL -gt 0 ]]; then
    printf "${RED}${BOLD}  ✗ Quality check FAILED${NC}\n\n"
    exit 1
elif [[ $SKIP -gt 0 ]]; then
    printf "Checks completed with %d checks unavailable; not a full approval.\n" "$SKIP"
    exit 0
else
    printf "${GREEN}${BOLD}  ✓ Quality check PASSED${NC}\n\n"
    exit 0
fi
