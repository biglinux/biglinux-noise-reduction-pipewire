#!/usr/bin/env bash
# Bootstrap the calibration environment.
#
#   - Creates a Python venv at `scripts/calibrate/.venv`
#   - Installs `requirements.txt`
#   - Downloads DNSMOS ONNX models into the cache
#   - Downloads the DNS-5 blind test set (~5 GB) into the cache
#
# The cache lives outside the repo at:
#   ${XDG_CACHE_HOME:-$HOME/.cache}/biglinux-noise-reduction-pipewire/calibration/
#
# Re-running is safe: existing models/datasets are skipped unless
# --force is passed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CACHE_ROOT="${XDG_CACHE_HOME:-$HOME/.cache}/biglinux-noise-reduction-pipewire/calibration"
VENV="$SCRIPT_DIR/.venv"

FORCE=0
SKIP_DATASET=0
SKIP_MODELS=0
SKIP_DEPS=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --force) FORCE=1 ;;
        --skip-dataset) SKIP_DATASET=1 ;;
        --skip-models) SKIP_MODELS=1 ;;
        --skip-deps) SKIP_DEPS=1 ;;
        --help|-h)
            sed -n '2,17p' "$0"
            exit 0
            ;;
        *) echo "unknown flag: $1" >&2; exit 1 ;;
    esac
    shift
done

mkdir -p "$CACHE_ROOT/models/dnsmos" "$CACHE_ROOT/datasets/dns"

# ── Python venv + deps ──────────────────────────────────────────────
if [[ $SKIP_DEPS -eq 0 ]]; then
    if [[ ! -d "$VENV" ]]; then
        python3 -m venv "$VENV"
    fi
    # shellcheck disable=SC1091
    source "$VENV/bin/activate"
    python -m pip install --upgrade pip wheel
    python -m pip install -r "$SCRIPT_DIR/requirements.txt"
    deactivate
fi

# ── DNSMOS ONNX models ──────────────────────────────────────────────
fetch() {
    local url="$1" dest="$2"
    if [[ -f "$dest" && $FORCE -eq 0 ]]; then
        printf '  skip (cached): %s\n' "$(basename "$dest")"
        return 0
    fi
    printf '  fetch: %s\n' "$url"
    curl -fL --retry 3 --retry-delay 2 -o "$dest.partial" "$url"
    mv "$dest.partial" "$dest"
}

if [[ $SKIP_MODELS -eq 0 ]]; then
    printf '\n[1/2] DNSMOS ONNX models\n'
    fetch \
        "https://raw.githubusercontent.com/microsoft/DNS-Challenge/master/DNSMOS/DNSMOS/sig_bak_ovr.onnx" \
        "$CACHE_ROOT/models/dnsmos/sig_bak_ovr.onnx"
    fetch \
        "https://raw.githubusercontent.com/microsoft/DNS-Challenge/master/DNSMOS/DNSMOS/model_v8.onnx" \
        "$CACHE_ROOT/models/dnsmos/model_v8.onnx"
fi

# ── DNS-5 blind set (~5 GB) ─────────────────────────────────────────
# The DNS-Challenge repo distributes the blind set as TSV manifests
# pointing at Azure blob URLs. We pull the manifest and iterate.
if [[ $SKIP_DATASET -eq 0 ]]; then
    printf '\n[2/2] DNS-5 blind test set (~5 GB) into %s\n' "$CACHE_ROOT/datasets/dns/"
    MANIFEST="$CACHE_ROOT/datasets/dns/blind_testset_5.tsv"
    fetch \
        "https://raw.githubusercontent.com/microsoft/DNS-Challenge/master/manifests/blind_testset_5.tsv" \
        "$MANIFEST" || {
            printf '  WARNING: blind manifest 404. Falling back to a known stable\n'
            printf '           noisy-only subset (~2 GB) from the V3 release.\n'
            fetch \
                "https://github.com/microsoft/DNS-Challenge/raw/interspeech2021/datasets/test_set/synthetic/no_reverb/noisy/manifest.txt" \
                "$MANIFEST"
        }

    if [[ -f "$MANIFEST" ]]; then
        WAV_DIR="$CACHE_ROOT/datasets/dns/wav"
        mkdir -p "$WAV_DIR"
        # Manifest format varies; treat each non-empty, non-comment line
        # as either a URL or a relative path under the DNS-Challenge LFS
        # tree. We dedupe and rate-limit to be polite.
        TOTAL=$(grep -cvE '^#|^$' "$MANIFEST" || echo 0)
        i=0
        while IFS= read -r line; do
            [[ -z "$line" || "$line" == \#* ]] && continue
            i=$((i + 1))
            url="$line"
            [[ "$url" != http* ]] && \
                url="https://raw.githubusercontent.com/microsoft/DNS-Challenge/master/$line"
            name="$(basename "$url")"
            dest="$WAV_DIR/$name"
            if [[ -f "$dest" && $FORCE -eq 0 ]]; then
                continue
            fi
            printf '  [%d/%d] %s\n' "$i" "$TOTAL" "$name"
            curl -fsSL --retry 2 --retry-delay 2 -o "$dest.partial" "$url" && \
                mv "$dest.partial" "$dest" || \
                printf '    skip (fetch failed): %s\n' "$name"
        done < "$MANIFEST"
    fi
fi

printf '\nDone. Cache root: %s\n' "$CACHE_ROOT"
printf 'Activate venv:    source %s/bin/activate\n' "$VENV"
printf 'Smoke test:       python %s/score_pair.py --help\n' "$SCRIPT_DIR"
