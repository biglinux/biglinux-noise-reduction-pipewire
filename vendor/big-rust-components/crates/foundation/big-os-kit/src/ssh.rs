// SPDX-License-Identifier: MIT

//! GTK-free helpers for building `ssh`/`sftp` command lines and managing
//! OpenSSH connection-multiplexing (`ControlMaster`) sockets.
//!
//! - `remote_command_argv`: compose an argv list for an `ssh` or `sftp`
//!   invocation from typed connection inputs (host, user, key, port, options).
//! - `ssh_control_master`: derive the argv and socket-filename contracts for
//!   OpenSSH control-socket check/exit/probe operations.
//!
//! Every helper returns argv lists (never a shell string), so callers spawn
//! through an argv-form subprocess without shell interpolation.

/// Argv builder for one-shot `ssh`/`sftp` connections.
pub mod remote_command_argv;
/// OpenSSH `ControlMaster` socket argv + filename helpers.
pub mod ssh_control_master;
