// SPDX-License-Identifier: MIT
//! Whole-process jemalloc for BigLinux GTK4 binaries.
//!
//! Depending on this crate links the system jemalloc (see `build.rs`); its
//! unprefixed `malloc`/`free` then interpose the entire process — GLib's C
//! allocations included — so freed memory returns to the OS over a long session.
//! glibc's allocator does not give it back, and GTK4 churns enough that RSS
//! creeps without this. Child processes are separate binaries and do NOT inherit
//! it: nothing is leaked to spawned shells, thumbnail processes, or daemons.
//!
//! Call [`anchor`] once at the top of `main()`. It references a jemalloc-only
//! symbol so the linker keeps libjemalloc NEEDED under the default `--as-needed`
//! (a bare dependency whose symbols are never referenced would be dropped, and
//! the interposition with it), and it returns the static jemalloc version string
//! for diagnostics.
//!
//! Linking jemalloc is not enough on its own: with the stock defaults it does NOT
//! return idle memory either. The [`malloc_conf`] global below tunes it so it
//! actually does — see that item for the why and the measured effect.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::NonNull;

/// Boot-time jemalloc options, read from the unprefixed `malloc_conf` global
/// before `main` runs (the system jemalloc declares its own copy *weak*, so this
/// strong definition wins; on Linux the dynamic linker resolves libjemalloc's
/// reference to the executable's symbol).
///
/// Linking jemalloc alone is not enough. jemalloc returns freed pages to the OS
/// only when a per-arena decay timer advances, and with the stock defaults
/// (`background_thread:false`, `narenas:4*ncpus`) that timer advances *only* on a
/// later allocation into the same arena — so an idle GTK4 shell never hands the
/// pages back and its RSS sits at the high-water mark.
///
/// `narenas_ratio:1` makes jemalloc size the arena pool at one arena per CPU
/// instead of its default `4*ncpus` (the multiplier is `opt_narenas_ratio`,
/// `FXP_INIT_INT(4)` in 5.3.1 — confirmed settable on the installed lib via an
/// `abort_conf:true,narenas_ratio:1` probe that did NOT abort, where a bogus key
/// did). This is the load-bearing knob: arenas manage memory independently, so a
/// thread that allocates pins its arena's retained-page headroom + metadata, and
/// the stock 40-arena pool (10-core host) spreads ~21 GTK worker threads across
/// ~21 distinct arenas. Collapsing the pool reclaims that.
///
/// Measured on big-shell (10-core host) after identical open/close-menu churn,
/// then settle, RssAnon. The cliff is the 40-arena default; below ~ncpus the
/// spread is within run-to-run noise (±~15 MB, dominated by the app's own icon
/// cache), so don't read the last rows as precise — read the first as the bug:
///
/// | options                          | arenas | RssAnon |
/// |----------------------------------|--------|---------|
/// | default                          |   40   | 203 MB  |
/// | `narenas:8`                      |    8   |  66 MB  |
/// | `narenas:4`                      |    4   |  57 MB  |
/// | `narenas:2`                      |    2   |  53 MB  |
/// | `narenas:1`                      |    1   |  50 MB  |
/// | `narenas_ratio:1` (= ncpus)      |   10   | 50–80   |
///
/// A *ratio* rather than a hard `narenas:1` because this crate is shared, and the
/// real deployment is `big-host`: one process that embeds many GTK app-modules
/// (shell, files, terminal, editor) each with their own worker threads — verified
/// by loading the editor + files + terminal `.so`s into a single host and
/// confirming `narenas_ratio:1` matched the default footprint there (≈42 MB), no
/// penalty. A fixed single arena would serialize every module's allocation through
/// one lock on a capable multi-core desktop; `narenas_ratio:1` instead scales the
/// pool with the host — one arena per CPU keeps allocator parallelism on big
/// machines yet still collapses to a couple of arenas on the few-core boxes
/// BigLinux targets, where memory matters most. On this host ncpus arenas costs
/// little more than `narenas:1`: idle GTK worker threads never touch most arenas,
/// so their pages never materialise — only the default `4*ncpus` pool is
/// pathological.
///
/// Enabling `background_thread` after locale initialization drives decay while idle, off
/// the main thread, so the aggressive `dirty_decay_ms:1000,muzzy_decay_ms:0`
/// return costs no main-thread CPU (without it jemalloc only purges on a later
/// alloc into the same arena — an idle shell never would). `tcache_max` and
/// `metadata_thp` were swept too: capping `tcache_max` saved <2 MB but forces
/// larger allocations onto the arena lock (worse on weak CPUs), and
/// `metadata_thp:auto` measured *larger* — both are CPU trades, not memory wins,
/// so the thread cache is left at its default.
///
/// Arena-pool sizing is boot-only — it cannot be set via `mallctl` at runtime,
/// which is why this is a compile-time global and not a call in [`anchor`].
/// Boot-time background work stays disabled until the application has initialized
/// locale. Use `set_background_threads` afterwards; merely moving a call within
/// main cannot prevent allocator workers created before main.
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static malloc_conf: Option<&'static u8> = {
    const OPTS: &[u8] =
        b"narenas_ratio:1,background_thread:false,dirty_decay_ms:1000,muzzy_decay_ms:0\0";
    // Option<&u8> is a thin, non-null pointer here (null niche), matching
    // jemalloc's `const char *`; it points at the NUL-terminated option string.
    Some(&OPTS[0])
};

unsafe extern "C" {
    /// jemalloc's introspection entry point. glibc has no `mallctl`, so naming it
    /// forces the linker to bind libjemalloc — that is the whole point (see
    /// [`anchor`]). Read-only when called with a NULL `newp`.
    fn mallctl(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> c_int;
}

/// Tie this binary to jemalloc. Call ONCE, early in `main()`.
///
/// Performs a read-only `mallctl("version", ...)` query. The call's primary
/// purpose is the reference to `mallctl`, which keeps libjemalloc a NEEDED
/// dynamic dependency so its allocator interposes the whole process. The
/// returned string is a cheap diagnostic that also keeps this contract
/// test-observable. Safe to call before the GTK main loop.
#[inline(never)]
pub fn anchor() -> Option<&'static CStr> {
    let mut version: *const c_char = std::ptr::null();
    let mut len = std::mem::size_of::<*const c_char>();
    // SAFETY: read-only mallctl("version", ...): jemalloc copies its static
    // version-string pointer into `version` (NULL `newp` => no write). No
    // ownership crosses the boundary.
    unsafe {
        let status = mallctl(
            c"version".as_ptr(),
            (&raw mut version).cast::<c_void>(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        );
        if status != 0 {
            return None;
        }

        let version = NonNull::new(std::hint::black_box(version).cast_mut())?;
        // SAFETY: jemalloc returns a pointer to its static NUL-terminated
        // version string for mallctl("version").
        Some(CStr::from_ptr(version.as_ptr()))
    }
}

/// Change jemalloc's background workers through its documented dynamic API.
/// Disabling terminates these workers synchronously, including a MALLOC_CONF
/// override that enabled them before main. This does not stop unrelated threads.
pub fn set_background_threads(enabled: bool) -> Result<(), i32> {
    let mut value = enabled;
    // SAFETY: background_thread accepts a bool of exactly this size. No
    // borrowed pointer outlives mallctl, which synchronously consumes it.
    let status = unsafe {
        mallctl(
            c"background_thread".as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            (&raw mut value).cast::<c_void>(),
            std::mem::size_of::<bool>(),
        )
    };
    if status == 0 { Ok(()) } else { Err(status) }
}

/// Application default after locale/GTK initialization. Honor an explicit
/// debugging override that opts out of background purging.
pub fn background_threads_requested() -> bool {
    std::env::var("MALLOC_CONF")
        .ok()
        .and_then(|options| {
            options
                .split(',')
                .filter_map(|option| option.split_once(':'))
                .filter(|(key, _)| *key == "background_thread")
                .map(|(_, value)| value == "true")
                .next_back()
        })
        .unwrap_or(true)
}
