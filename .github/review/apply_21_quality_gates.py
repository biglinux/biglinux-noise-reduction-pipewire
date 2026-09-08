from pathlib import Path
from _common import done, replace, write, commit

TITLE = 'test: distinguish isolated GUI coverage from skipped checks'
if not done(TITLE):
    path = 'src/ui.rs'
    replace(path, '    #[test]\n    fn gtk_display_contracts_cover_dialogs_and_model_picker()', '    #[test]\n    #[ignore = "requires isolated display; run scripts/test-ui.sh"]\n    fn gtk_display_contracts_cover_dialogs_and_model_picker()')
    replace(path, '''        if !matches!(session_mode.as_str(), "headless" | "vm" | "disposable") {
            eprintln!("skip: GTK display contracts require KWin headless, VM, or disposable UI");
            return;
        }''', '''        assert!(
            matches!(session_mode.as_str(), "headless" | "vm" | "disposable"),
            "GTK contracts require an isolated session; run scripts/test-ui.sh"
        );''')
    write('scripts/test-ui.sh', r'''#!/usr/bin/env bash
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
export LANG=C.UTF-8 LC_ALL=C.UTF-8 LANGUAGE=C GDK_BACKEND=x11 GSK_RENDERER=cairo
xvfb-run -a dbus-run-session -- cargo test --all-features --locked --lib \
    ui::tests::gtk_display_contracts_cover_dialogs_and_model_picker \
    -- --ignored --exact --test-threads=1
''')
    write('scripts/test-miri.sh', r'''#!/usr/bin/env bash
# Only deterministic data-model tests. dlopen, GTK, file locks and native
# subprocess tests have a different runner; unsupported FFI is not an UB result.
set -euo pipefail
cd "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo +nightly miri test --locked --lib -- \
    config::audio:: config::echo_cancel:: config::equalizer:: \
    config::output_filter:: config::processing:: config::ui::
''')
    for file in ['scripts/test-ui.sh', 'scripts/test-miri.sh']:
        Path(file).chmod(0o755)
    commit(TITLE, ['src/ui.rs', 'scripts/test-ui.sh', 'scripts/test-miri.sh'])

TITLE = 'fix(ci): fail strict quality checks when required tools are missing'
if not done(TITLE):
    path = 'scripts/quality-check.sh'
    replace(path, '#   --ci    Use the exact gate enforced in CI', '#   --ci    Fail rather than skip a required portable gate; cloud security\n#           checks and the isolated GUI job run separately in GitHub Actions.')
    replace(path, 'echo "  --ci    Use the exact CI gate"', 'echo "  --ci    Fail when a required portable check cannot run"')
    replace(path, '''    CHECKS+=("$name")
    printf "\\n${YELLOW}''', '''    if $CI; then
        CHECKS+=("$name")
        RESULTS+=("fail")
        FAIL=$((FAIL + 1))
        printf 'Required check unavailable: %s (%s)\\n' "$name" "$reason" >&2
        return
    fi
    CHECKS+=("$name")
    printf "\\n${YELLOW}''')
    replace(path, 'cargo fmt\n', 'cargo fmt --all\n')
    replace(path, 'cargo fmt --check', 'cargo fmt --all --check')
    replace(path, 'cargo clippy --all-targets --all-features -- -D warnings', 'cargo clippy --all-targets --all-features --locked -- -D warnings')
    replace(path, 'run_check "tests" cargo test', 'run_check "tests (unit, integration and doctests)" cargo test --all-features --locked')
    marker = '# ── --full extras'
    text = Path(path).read_text()
    assert text.count(marker) == 1
    text = text.replace(marker, '''# Translation validation includes source coverage, not just PO syntax.
if has_cmd xtr && has_cmd xgettext && has_cmd msgcat && has_cmd msgcmp && has_cmd msgmerge && has_cmd msgfmt; then
    run_check "translation extraction coverage" bash scripts/refresh-pot.sh --check
    for catalog in po/*.po; do
        run_check "catalog: $catalog" msgfmt --check --check-header -o /dev/null "$catalog"
    done
else
    skip_check "gettext source coverage" "install gettext and xtr"
fi

''' + marker)
    start = text.index('        run_check "miri (safe-code UB)"')
    end = text.index('\n    else', start)
    text = text[:start] + '        run_check "miri (pure data-model contracts)" bash scripts/test-miri.sh' + text[end:]
    # Never summarize a partial local run as full approval.
    text = text.replace('else\n    printf "${GREEN}${BOLD}  ✓ Quality check PASSED', 'elif [[ $SKIP -gt 0 ]]; then\n    printf "Checks completed with %d checks unavailable; not a full approval.\\n" "$SKIP"\n    exit 0\nelse\n    printf "${GREEN}${BOLD}  ✓ Quality check PASSED')
    Path(path).write_text(text)
    commit(TITLE, [path])

TITLE = 'fix(lint): preserve protocol names and encoded test fixtures'
if not done(TITLE):
    path = '_typos.toml'
    replace(path, '    "i18nd?",', '    "i18nd?",\n    # MPRIS specifies the signal name Seeked; do not rename the protocol.\n    "[Ss]eeked",\n    "seeked_position",\n    "emit_seeked",')
    replace(path, 'HDA = "HDA"', 'HDA = "HDA"\n# GTK terminology and percent-encoded café fixture.\nunparented = "unparented"\ncaf = "caf"')
    path2 = 'vendor/big-rust-components/crates/ui/generic/big-relm4-components/examples/zone_layout_editor_demo.rs'
    replace(path2, 'driveable headless', 'drivable headless')
    commit(TITLE, [path, path2])
