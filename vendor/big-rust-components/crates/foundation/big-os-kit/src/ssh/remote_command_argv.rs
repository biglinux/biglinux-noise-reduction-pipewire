// SPDX-License-Identifier: MIT

//! Compose an argv list for an `ssh` or `sftp` connection from typed inputs.

/// Which remote client an argv line invokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCommandKind {
    /// Interactive/remote-exec `ssh`.
    Ssh,
    /// File-transfer `sftp`.
    Sftp,
}

impl RemoteCommandKind {
    /// Program name (`ssh` or `sftp`).
    #[must_use]
    pub const fn executable(self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
            Self::Sftp => "sftp",
        }
    }
    /// Port flag for this client (`ssh` uses `-p`, `sftp` uses `-P`).
    #[must_use]
    pub const fn port_flag(self) -> &'static str {
        match self {
            Self::Ssh => "-p",
            Self::Sftp => "-P",
        }
    }
}

/// Borrowed connection parameters used to build a remote command argv.
pub struct RemoteCommandInputs<'a> {
    /// Client to invoke (`ssh` or `sftp`).
    pub kind: RemoteCommandKind,
    /// Target hostname.
    pub hostname: &'a str,
    /// Optional login user (`user@host` when set).
    pub username: Option<&'a str>,
    /// Optional identity file passed via `-i`.
    pub key_file: Option<&'a str>,
    /// Optional non-default port.
    pub port: Option<u16>,
    /// Extra `-o key=value` options, in order.
    pub options: &'a [(String, String)],
    /// Optional remote path (appended as `host:path` for `sftp`).
    pub remote_path: Option<&'a str>,
}

fn compose_target(
    kind: RemoteCommandKind,
    username: Option<&str>,
    hostname: &str,
    remote_path: Option<&str>,
) -> String {
    let mut target = match username {
        Some(user) => format!("{user}@{hostname}"),
        None => hostname.to_owned(),
    };
    if kind == RemoteCommandKind::Sftp
        && let Some(path) = remote_path
    {
        target.push(':');
        target.push_str(path.trim());
    }
    target
}
/// Build the full argv (program + flags + `--` + target) for a remote
/// connection. The target is emitted after `--` so a hostile hostname cannot
/// be reinterpreted as an option.
#[must_use]
pub fn build_remote_command(inputs: &RemoteCommandInputs<'_>) -> Vec<String> {
    let mut argv: Vec<String> = Vec::new();
    argv.push(inputs.kind.executable().to_owned());

    for (key, value) in inputs.options {
        argv.push("-o".to_owned());
        argv.push(format!("{key}={value}"));
    }

    if let Some(key_file) = inputs.key_file {
        argv.push("-i".to_owned());
        argv.push(key_file.to_owned());
    }

    if let Some(port) = inputs.port {
        argv.push(inputs.kind.port_flag().to_owned());
        argv.push(port.to_string());
    }

    argv.push("--".to_owned());
    argv.push(compose_target(
        inputs.kind,
        inputs.username,
        inputs.hostname,
        inputs.remote_path,
    ));

    argv
}
