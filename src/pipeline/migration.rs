//! Conservative migration of application-owned configuration files.
//!
//! Never migrate a generic PipeWire configuration by filename. A hard link
//! creates a backup without replacing an existing backup; only then may the
//! old directory entry be removed. Unknown files and symlinks are left alone.

use std::io;
use std::path::Path;

pub(super) fn archive(path: &Path) -> io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() {
        return Err(io::Error::other("legacy path is not a regular file; left unchanged"));
    }
    let Some(name) = path.file_name() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "missing filename"));
    };
    let mut backup_name = name.to_os_string();
    backup_name.push(".biglinux-backup");
    let backup = path.with_file_name(backup_name);
    // Both entries are in the same directory/filesystem. hard_link is an
    // atomic create-if-absent, unlike rename, which could overwrite a backup.
    std::fs::hard_link(path, &backup)?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    std::fs::remove_file(path)?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    log::info!("pipeline: preserved legacy configuration at {}", backup.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_keeps_a_recoverable_copy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("biglinux.conf");
        std::fs::write(&path, "user edits\n").unwrap();
        archive(&path).unwrap();
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(dir.path().join("biglinux.conf.biglinux-backup")).unwrap(), "user edits\n");
        archive(&path).unwrap();
    }

    #[test]
    fn migration_never_overwrites_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("biglinux.conf");
        let backup = dir.path().join("biglinux.conf.biglinux-backup");
        std::fs::write(&path, "new user edits").unwrap();
        std::fs::write(&backup, "original backup").unwrap();
        assert!(archive(&path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "new user edits");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "original backup");
    }

    #[test]
    fn migration_does_not_follow_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("personal.conf");
        let link = dir.path().join("biglinux.conf");
        std::fs::write(&target, "personal configuration").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(archive(&link).is_err());
        assert!(link.is_symlink());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "personal configuration");
    }
}
