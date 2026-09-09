#!/usr/bin/env bash
# Only deterministic data-model tests. dlopen, GTK, file locks and native
# subprocess tests have a different runner; unsupported FFI is not an UB result.
set -euo pipefail
cd "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo +nightly miri test --locked --lib -- \
    config::audio:: config::echo_cancel:: config::equalizer:: \
    config::output_filter:: config::processing:: config::ui::
