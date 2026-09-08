// SPDX-License-Identifier: MIT

//! XDG directory resolution for Linux desktop apps.
//!
//! This module intentionally avoids the cross-platform `dirs` crate. BigLinux
//! packages target Linux/XDG, so resolving the relevant environment variables
//! directly keeps the dependency graph smaller and avoids unrelated platform
//! transitive crates.

use std::path::{Path, PathBuf};

const USER_DIRS_RELATIVE: &str = "user-dirs.dirs";

/// Return `$HOME` when it is set to an absolute path.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    absolute_env_path("HOME")
}

/// Return `$XDG_CONFIG_HOME` or `$HOME/.config`.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    xdg_base_dir("XDG_CONFIG_HOME", ".config")
}

/// Return `$XDG_CACHE_HOME` or `$HOME/.cache`.
#[must_use]
pub fn cache_dir() -> Option<PathBuf> {
    xdg_base_dir("XDG_CACHE_HOME", ".cache")
}

/// Return `$XDG_STATE_HOME` or `$HOME/.local/state`.
#[must_use]
pub fn state_dir() -> Option<PathBuf> {
    xdg_base_dir("XDG_STATE_HOME", ".local/state")
}

/// Return `XDG_MUSIC_DIR` from `user-dirs.dirs` or `$HOME/Music`.
#[must_use]
pub fn audio_dir() -> Option<PathBuf> {
    user_dir("XDG_MUSIC_DIR", "Music")
}

/// Return `XDG_VIDEOS_DIR` from `user-dirs.dirs` or `$HOME/Videos`.
#[must_use]
pub fn video_dir() -> Option<PathBuf> {
    user_dir("XDG_VIDEOS_DIR", "Videos")
}

fn xdg_base_dir(env_key: &str, home_relative: &str) -> Option<PathBuf> {
    absolute_env_path(env_key).or_else(|| home_dir().map(|home| home.join(home_relative)))
}

fn user_dir(user_dirs_key: &str, fallback_name: &str) -> Option<PathBuf> {
    let home = home_dir()?;
    read_user_dirs_value(user_dirs_key, &home).or_else(|| Some(home.join(fallback_name)))
}

fn absolute_env_path(key: &str) -> Option<PathBuf> {
    let value = std::env::var_os(key)?;
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

fn read_user_dirs_value(key: &str, home: &Path) -> Option<PathBuf> {
    let path = config_dir()?.join(USER_DIRS_RELATIVE);
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .filter_map(|line| parse_user_dirs_line(line, home))
        .find_map(|(found_key, path)| (found_key == key).then_some(path))
}

fn parse_user_dirs_line(line: &str, home: &Path) -> Option<(String, PathBuf)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (key, raw_value) = trimmed.split_once('=')?;
    let value = raw_value.trim().strip_prefix('"')?.strip_suffix('"')?;
    Some((key.trim().to_owned(), expand_user_dirs_value(value, home)?))
}

fn expand_user_dirs_value(value: &str, home: &Path) -> Option<PathBuf> {
    if value == "$HOME" {
        return Some(home.to_path_buf());
    }
    if let Some(relative) = value.strip_prefix("$HOME/") {
        return Some(home.join(relative));
    }
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_dirs_line_expands_home_relative_path() {
        let home = Path::new("/home/tester");

        let parsed = parse_user_dirs_line(r#"XDG_MUSIC_DIR="$HOME/Audio Files""#, home)
            .expect("parse user dir");

        assert_eq!(parsed.0, "XDG_MUSIC_DIR");
        assert_eq!(parsed.1, PathBuf::from("/home/tester/Audio Files"));
    }

    #[test]
    fn parse_user_dirs_line_ignores_comments_and_relative_paths() {
        let home = Path::new("/home/tester");

        assert!(parse_user_dirs_line("# comment", home).is_none());
        assert!(parse_user_dirs_line(r#"XDG_MUSIC_DIR="Music""#, home).is_none());
    }
}
