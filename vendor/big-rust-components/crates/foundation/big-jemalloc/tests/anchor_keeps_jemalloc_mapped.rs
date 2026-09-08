// SPDX-License-Identifier: MIT
//! Link regression guard for `big_jemalloc::anchor`.
//!
//! This binary intentionally does not declare or call `mallctl` itself. The only
//! jemalloc-specific reference must come from `big_jemalloc::anchor()`, so an
//! empty anchor body lets the linker's default `--as-needed` drop libjemalloc and
//! makes this test fail.

// Miri cannot execute the real jemalloc interposition path; ordinary
// `cargo test` still covers this link/runtime contract.
#[test]
#[cfg_attr(miri, ignore)]
fn anchor_keeps_jemalloc_mapped_into_current_address_space() {
    let version = big_jemalloc::anchor().expect("jemalloc version should be readable");
    assert!(
        !version.to_bytes().is_empty(),
        "jemalloc version string should not be empty"
    );
    let maps = std::fs::read_to_string("/proc/self/maps").expect("read /proc/self/maps");
    assert!(
        maps.contains("libjemalloc"),
        "libjemalloc not mapped; the `--as-needed` link dropped it after anchor lost its jemalloc reference"
    );
}
