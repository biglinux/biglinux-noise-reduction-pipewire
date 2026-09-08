//! Links the system jemalloc (`-ljemalloc`) into every binary that depends on
//! this crate. jemalloc exports unprefixed `malloc`/`free`/`realloc`/... so once
//! it is a NEEDED library it interposes the WHOLE process — Rust allocations AND
//! GLib's C-side ones. That is what lets a long-lived GTK4 app return freed
//! memory to the OS: glibc's allocator keeps freed blocks in per-thread arenas,
//! jemalloc gives them back. Child processes are separate binaries, so they do
//! NOT inherit jemalloc — no launcher, no LD_PRELOAD, nothing leaked to spawned
//! commands.
//!
//! `rustc-link-lib` propagates transitively onto the dependent binary's link
//! line. Under the linker's default `--as-needed` the NEEDED entry survives only
//! if a jemalloc symbol is actually referenced — `lib.rs`'s `anchor()` references
//! the jemalloc-only `mallctl` for exactly that reason; call it once in `main()`.
//!
//! Runtime needs the C23-`free_sized`-correct jemalloc (jemalloc-gtk-fixed): a
//! jemalloc that mishandles `free_sized(NULL)` corrupts under glibc 2.41+, which
//! is why the consuming packages depend on the fixed build, not stock jemalloc.

fn main() {
    println!("cargo:rustc-link-lib=dylib=jemalloc");
    println!("cargo:rerun-if-changed=build.rs");
}
