// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]

//! GTK-free OS-level contracts for BigLinux Rust apps.
//!
//! Split out of `big-app-kit` so CLI tools and non-GUI library crates
//! (camera/audio pipelines, PTY, launchers, integrations) can reuse the
//! same typed primitives without pulling in gtk4/libadwaita/relm4.
//!
//! - [`subprocess`]: argv-form subprocess specs with timeout, capture,
//!   redaction, allow-list, and a spawn-handle for long-running children.
//! - [`storage`]: atomic JSON persistence (temp + rename) and a versioned
//!   forward-migration envelope.
//! - [`keyring`]: typed secret-store trait, an in-memory test backend, and
//!   (feature `libsecret-system-backend`) a system libsecret backend.
//! - [`microdaemon`]: GTK-free versioned frame protocol shared by the small
//!   per-user daemons that own durable work.
//!
//! `big-app-kit` re-exports these modules unchanged, so existing
//! `big_app_kit::{subprocess, storage, keyring}` paths keep working.

/// Blocking HTTP client with timeouts, retry policy, and size limits
/// (feature `http-client`). Single home for outbound HTTP across the
/// BigLinux stack — re-exported by `big-app-kit`, consumed by data services.
/// Filesystem-safe filename sanitization.
pub mod filenames;
#[cfg(feature = "http-client")]
pub mod http_client;
/// Human-readable byte/speed/duration formatting.
pub mod human_format;
/// Secret-store contracts and backends.
pub mod keyring;
/// Process allocator maintenance (`trim_arenas`).
pub mod mem;
/// Shared microdaemon protocol primitives.
pub mod microdaemon;
/// PulseAudio/PipeWire device discovery and control through `pactl`.
pub mod pulseaudio;
/// POSIX shell single-quoting for shell-command interpolation.
pub mod shell_quote;
/// `ssh`/`sftp` argv builders and OpenSSH `ControlMaster` socket helpers.
pub mod ssh;
/// Atomic JSON persistence helpers.
pub mod storage;
/// Safe subprocess argv specs and spawn handles.
pub mod subprocess;
/// Text-vs-binary detection and editor-mode classification for files.
pub mod text_file_kind;
/// RFC 3986 percent-encoding + URL filename helpers (GTK-free, no deps).
pub mod url;
/// Local webcam/video-node inventory helpers.
pub mod webcam;
/// XDG directory resolution for BigLinux desktop apps.
pub mod xdg_dirs;
