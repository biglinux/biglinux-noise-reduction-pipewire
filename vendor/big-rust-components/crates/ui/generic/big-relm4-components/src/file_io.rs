// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Off-main-loop file read/write for GTK apps.
//!
//! Read or write a file on a worker thread and deliver the result back on the
//! GTK main loop, so a slow or hung filesystem (a network mount, an unresponsive
//! disk) never freezes the UI. Built on [`crate::task::spawn_blocking_result`]
//! and `big_os_kit::storage::atomic_write`. The canonical home for "our program
//! needs to save/load a file without blocking" — apps should reach for these
//! instead of a bare `std::fs::read`/`write` on the main thread.
//!
//! The `on_done` callbacks run on the main thread, so they can touch `!Send` GTK
//! widgets and `Rc` UI state. Hold a generation/stale guard in `on_done` when a
//! rapid re-save/re-load of the same target can supersede an in-flight op.

use std::io;
use std::path::{Path, PathBuf};

use crate::task::spawn_blocking_result;

/// Atomically write `bytes` to `path` off the main loop (temp + `fsync` + rename
/// via `big_os_kit::storage::atomic_write`); `on_done` receives the result on the
/// main thread.
///
/// See `atomic_write`'s note on permissions/inode: the rename installs a new
/// inode with default permissions, so for editing a user document whose mode
/// must be preserved use [`write_with_async`] with an in-place writer instead.
/// Right for config/state and freshly-created files.
pub fn write_bytes_async<D>(path: PathBuf, bytes: Vec<u8>, on_done: D)
where
    D: FnOnce(io::Result<()>) + 'static,
{
    spawn_blocking_result(
        move || big_os_kit::storage::atomic_write(&path, &bytes),
        on_done,
    );
}

/// Write `bytes` to `path` off the main loop using a caller-supplied `writer`
/// (e.g. a plain in-place write, or one with a privileged-helper fallback),
/// reporting `Result<(), E>` on the main thread. The writer runs on the worker,
/// so it and `E` must be `Send`.
pub fn write_with_async<E, W, D>(path: PathBuf, bytes: Vec<u8>, writer: W, on_done: D)
where
    E: Send + 'static,
    W: FnOnce(&Path, &[u8]) -> Result<(), E> + Send + 'static,
    D: FnOnce(Result<(), E>) + 'static,
{
    spawn_blocking_result(move || writer(&path, &bytes), on_done);
}

/// Read all of `path` off the main loop; `on_done` receives the bytes on the
/// main thread.
pub fn read_bytes_async<D>(path: PathBuf, on_done: D)
where
    D: FnOnce(io::Result<Vec<u8>>) + 'static,
{
    spawn_blocking_result(move || std::fs::read(&path), on_done);
}

/// Read `path` as a UTF-8 string off the main loop; `on_done` receives it on the
/// main thread. Errors on invalid UTF-8 — use [`read_bytes_async`] for binary.
pub fn read_to_string_async<D>(path: PathBuf, on_done: D)
where
    D: FnOnce(io::Result<String>) + 'static,
{
    spawn_blocking_result(move || std::fs::read_to_string(&path), on_done);
}
