// SPDX-License-Identifier: MIT

//! OpenSSH `ControlMaster` socket contracts: argv builders for check/exit/probe
//! operations and a deterministic control-socket filename scheme.

/// Filename prefix shared by every control socket this module names.
pub const CONTROL_SOCKET_PREFIX: &str = "ssh_control_";
/// `ssh` program name used in every argv this module builds.
pub const SSH_EXECUTABLE: &str = "ssh";
/// Default SSH port when a session specifies none.
pub const DEFAULT_SSH_PORT: u16 = 22;
/// Resolve the effective login user: the session user when non-empty, else the
/// current login name.
#[must_use]
pub fn effective_ssh_user(session_user: Option<&str>, current_login: &str) -> String {
    match session_user {
        Some(u) if !u.is_empty() => u.to_owned(),
        _ => current_login.to_owned(),
    }
}
/// Compose an `user@host` SSH target.
#[must_use]
pub fn ssh_target(user: &str, host: &str) -> String {
    format!("{user}@{host}")
}
/// Argv for `ssh -O check` against an existing control socket.
#[must_use]
pub fn build_control_check_argv(control_path: &str, target: &str) -> Vec<String> {
    vec![
        SSH_EXECUTABLE.to_owned(),
        "-O".to_owned(),
        "check".to_owned(),
        "-S".to_owned(),
        control_path.to_owned(),
        "--".to_owned(),
        target.to_owned(),
    ]
}
/// Argv for `ssh -O exit`, tearing down an existing control socket.
#[must_use]
pub fn build_control_exit_argv(control_path: &str, target: &str) -> Vec<String> {
    vec![
        SSH_EXECUTABLE.to_owned(),
        "-O".to_owned(),
        "exit".to_owned(),
        "-S".to_owned(),
        control_path.to_owned(),
        "--".to_owned(),
        target.to_owned(),
    ]
}
/// Argv for a non-interactive liveness probe reusing the control socket
/// (`BatchMode`, short timeouts, `ControlMaster=auto`, remote `true`).
#[must_use]
pub fn build_control_session_probe_argv(control_path: &str, target: &str) -> Vec<String> {
    vec![
        SSH_EXECUTABLE.to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=3".to_owned(),
        "-o".to_owned(),
        "ConnectionAttempts=1".to_owned(),
        "-o".to_owned(),
        "ControlMaster=auto".to_owned(),
        "-S".to_owned(),
        control_path.to_owned(),
        "--".to_owned(),
        target.to_owned(),
        "true".to_owned(),
    ]
}
/// Argv for `ssh -O check` against a placeholder `dummy` target (socket-only
/// existence probe).
#[must_use]
pub fn build_control_check_dummy_argv(control_path: &str) -> Vec<String> {
    vec![
        SSH_EXECUTABLE.to_owned(),
        "-O".to_owned(),
        "check".to_owned(),
        "-S".to_owned(),
        control_path.to_owned(),
        "--".to_owned(),
        "dummy".to_owned(),
    ]
}
/// True if `name` is one of this module's control-socket filenames.
#[must_use]
pub fn is_control_socket_filename(name: &str) -> bool {
    name.starts_with(CONTROL_SOCKET_PREFIX)
}
/// Resolve the effective SSH port: the session port when non-zero, else
/// [`DEFAULT_SSH_PORT`].
#[must_use]
pub const fn effective_ssh_port(session_port: Option<u16>) -> u16 {
    match session_port {
        Some(p) if p != 0 => p,
        _ => DEFAULT_SSH_PORT,
    }
}

/// Build a deterministic control-socket filename from host, port, and user,
/// sanitizing host/user components to filesystem-safe tokens.
#[must_use]
pub fn format_ssh_control_filename(host: &str, port: u16, user: &str) -> String {
    let host = sanitize_control_component(host, "host");
    let user = sanitize_control_component(user, "user");
    format!("{CONTROL_SOCKET_PREFIX}{host}_{port}_{user}")
}

fn sanitize_control_component(value: &str, fallback: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_session_probe_uses_existing_mux_socket() {
        let argv = build_control_session_probe_argv("/run/big_terminal.sock", "alice@example.com");
        assert_eq!(argv[0], SSH_EXECUTABLE);
        assert!(
            argv.windows(2)
                .any(|w| w == ["-S", "/run/big_terminal.sock"])
        );
        assert!(argv.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert!(argv.windows(2).any(|w| w == ["-o", "ControlMaster=auto"]));
        let sep = argv.iter().position(|arg| arg == "--").expect("separator");
        assert_eq!(argv[sep + 1], "alice@example.com");
        assert_eq!(argv[sep + 2], "true");
    }
}
