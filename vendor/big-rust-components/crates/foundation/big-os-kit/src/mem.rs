// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Process allocator maintenance.

/// Return free `malloc` arenas to the OS so resident memory drops after a burst
/// of allocations is freed (e.g. closing a widget-heavy dialog or a tab).
///
/// glibc-only: a no-op on other libc/platforms. Single canonical implementation
/// shared by GUI (`big-relm4-components` dialog teardown) and non-GUI (PTY)
/// consumers so the `malloc_trim` wrapper is not duplicated per crate.
pub fn trim_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: `malloc_trim` is thread-safe and only operates on already-free
        // chunks owned by the same allocator as every other allocation in the
        // process. No Rust invariants are crossed.
        unsafe {
            libc::malloc_trim(0);
        }
    }
}
