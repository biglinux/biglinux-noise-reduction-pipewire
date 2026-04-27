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
    # Many distros mount /tmp `noexec`, which breaks pip when it tries
    # to dlopen build-env .so files (numpy/scipy header probes fail
    # with "failed to map segment from shared object"). Point pip at an
    # exec-friendly tempdir under the venv so wheels from source can
    # actually finish building.
    export TMPDIR="$VENV/tmp"
    mkdir -p "$TMPDIR"

    # shellcheck disable=SC1091
    source "$VENV/bin/activate"
    python -m pip install --upgrade pip wheel
    python -m pip install -r "$SCRIPT_DIR/requirements.txt"
    if [[ -f "$SCRIPT_DIR/requirements-extra.txt" ]]; then
        # Optional native deps (pesq, faster-whisper). Best-effort: a
        # failure here does not block the rest of the bootstrap because
        # PESQ and WER are nice-to-have on top of DNSMOS + STOI.
        python -m pip install -r "$SCRIPT_DIR/requirements-extra.txt" || \
            printf '  WARNING: optional metrics (pesq/whisper) failed to install — continuing.\n'
    fi
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

# ── VoiceBank+DEMAND test set (~250 MB) ─────────────────────────────
# The Microsoft DNS-Challenge blind manifests broke (404 against
# `master`). VoiceBank+DEMAND is the long-standing Edinburgh DataShare
# benchmark for speech enhancement and gives us paired clean/noisy
# WAVs at 48 kHz — exactly what the chain emulator needs to score
# PESQ/STOI/SI-SDR alongside the no-reference DNSMOS metric.
if [[ $SKIP_DATASET -eq 0 ]]; then
    DS_DIR="$CACHE_ROOT/datasets/voicebank_demand"
    mkdir -p "$DS_DIR"
    printf '\n[2/2] VoiceBank+DEMAND test set into %s\n' "$DS_DIR"

    # Edinburgh DataShare DOI 10.7488/ds/2117. The handle URLs are
    # stable since 2017 and serve direct ZIPs.
    BASE="https://datashare.ed.ac.uk/bitstream/handle/10283/2791"
    for archive in clean_testset_wav noisy_testset_wav; do
        zip="$DS_DIR/${archive}.zip"
        ok="$DS_DIR/${archive}/.extracted"
        if [[ -f "$ok" && $FORCE -eq 0 ]]; then
            printf '  skip (extracted): %s\n' "$archive"
            continue
        fi
        fetch "$BASE/${archive}.zip" "$zip" || {
            printf '  WARNING: %s download failed — skipping.\n' "$archive"
            continue
        }
        rm -rf "$DS_DIR/${archive}"
        if unzip -q -o "$zip" -d "$DS_DIR"; then
            : > "$DS_DIR/${archive}/.extracted"
            rm -f "$zip"
        else
            printf '  WARNING: failed to unzip %s — keeping the .zip.\n' "$archive"
        fi
    done
fi

printf '\nDone. Cache root: %s\n' "$CACHE_ROOT"
printf 'Activate venv:    source %s/bin/activate\n' "$VENV"
printf 'Smoke test:       python %s/score_pair.py --help\n' "$SCRIPT_DIR"
