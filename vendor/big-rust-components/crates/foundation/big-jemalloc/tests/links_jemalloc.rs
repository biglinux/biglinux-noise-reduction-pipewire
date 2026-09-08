// SPDX-License-Identifier: MIT
//! Link/interposition regression guard.
//!
//! This test binary depends on `big-jemalloc` exactly like a real app, so it
//! exercises the whole contract: `build.rs` adds `-ljemalloc`, and `anchor()`'s
//! reference to `mallctl` must keep libjemalloc NEEDED under the linker's default
//! `--as-needed`. If that ever breaks (anchor removed, symbol DCE'd, link order
//! regressed), jemalloc stops servicing allocations and this fails loudly.
//! This binary also declares `mallctl` to assert jemalloc stats; the sibling
//! `anchor_keeps_jemalloc_mapped` test binary intentionally does not, so it
//! proves `anchor()` supplies its own jemalloc reference.
//!
//! Requires a jemalloc with `--enable-stats` (default) on the link path.

use std::ffi::{c_char, c_int, c_void};

unsafe extern "C" {
    fn mallctl(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> c_int;
}

/// Refresh jemalloc's stats epoch, then read `stats.allocated`. `u64::MAX` on
/// failure (no jemalloc / stats disabled).
fn jemalloc_allocated() -> u64 {
    unsafe {
        let mut epoch: u64 = 1;
        let mut sz = std::mem::size_of::<u64>();
        let _ = mallctl(
            c"epoch".as_ptr(),
            (&raw mut epoch).cast::<c_void>(),
            &raw mut sz,
            (&raw mut epoch).cast::<c_void>(),
            std::mem::size_of::<u64>(),
        );
        let mut allocated: usize = 0;
        let mut len = std::mem::size_of::<usize>();
        let rc = mallctl(
            c"stats.allocated".as_ptr(),
            (&raw mut allocated).cast::<c_void>(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        );
        if rc == 0 { allocated as u64 } else { u64::MAX }
    }
}

// Miri cannot execute the real jemalloc `mallctl` interposition path; ordinary
// `cargo test` still covers this link/runtime contract.
#[test]
#[cfg_attr(miri, ignore)]
fn jemalloc_library_is_mapped_into_current_address_space() {
    big_jemalloc::anchor();
    let version = big_jemalloc::anchor().expect("jemalloc version should be readable");
    assert!(
        !version.to_bytes().is_empty(),
        "jemalloc version string should not be empty"
    );
    let maps = std::fs::read_to_string("/proc/self/maps").expect("read /proc/self/maps");
    assert!(
        maps.contains("libjemalloc"),
        "libjemalloc not mapped — the `--as-needed` link dropped it (anchor reference lost?)"
    );
}

// Miri cannot execute the real jemalloc `mallctl` interposition path; ordinary
// `cargo test` still covers this link/runtime contract.
#[test]
#[cfg_attr(miri, ignore)]
fn allocations_route_through_jemalloc() {
    big_jemalloc::anchor();
    let version = big_jemalloc::anchor().expect("jemalloc version should be readable");
    assert!(
        !version.to_bytes().is_empty(),
        "jemalloc version string should not be empty"
    );
    let before = jemalloc_allocated();
    assert_ne!(
        before,
        u64::MAX,
        "mallctl(stats.allocated) failed — jemalloc not interposing the process"
    );
    let blob = vec![0u8; 16 * 1024 * 1024];
    std::hint::black_box(blob.as_ptr());
    let after = jemalloc_allocated();
    std::hint::black_box(blob);
    assert!(
        after > before,
        "jemalloc stats.allocated did not rise across a 16 MiB alloc \
         (before={before}, after={after}) — malloc is not bound to jemalloc"
    );
}
