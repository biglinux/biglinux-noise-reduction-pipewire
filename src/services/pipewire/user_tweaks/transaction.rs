//! Byte-exact revisions for the two independently atomic tuning drop-ins.
//! A failed second write does not turn the first into a fictitious third-party edit.
use std::io;
use std::path::{Path, PathBuf};

use super::{
    UserTweaks, parse_pipewire, parse_wireplumber, pipewire_drop_in, render_pipewire,
    render_wireplumber, wireplumber_drop_in, write_or_remove,
};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TuningRevision {
    files: [Option<Vec<u8>>; 2],
}

#[derive(Debug)]
pub(crate) struct TuningWriteFailure {
    pub persisted: Option<TuningRevision>,
    pub error: io::Error,
}

impl TuningRevision {
    fn paths() -> [PathBuf; 2] {
        [pipewire_drop_in(), wireplumber_drop_in()]
    }

    pub fn load() -> io::Result<Self> {
        Self::read_at(&Self::paths())
    }

    fn read_at(paths: &[PathBuf; 2]) -> io::Result<Self> {
        Ok(Self {
            files: [read_optional(&paths[0])?, read_optional(&paths[1])?],
        })
    }

    pub fn from_settings(settings: &UserTweaks) -> Self {
        Self {
            files: [
                optional_bytes(&render_pipewire(settings)),
                optional_bytes(&render_wireplumber(settings)),
            ],
        }
    }

    pub fn settings(&self) -> io::Result<UserTweaks> {
        let mut settings = UserTweaks::default();
        for (index, bytes) in self.files.iter().enumerate() {
            if let Some(bytes) = bytes {
                let text = std::str::from_utf8(bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if index == 0 {
                    parse_pipewire(text, &mut settings);
                } else {
                    parse_wireplumber(text, &mut settings);
                }
            }
        }
        Ok(settings)
    }

    pub fn write_checked(
        settings: &UserTweaks,
        expected: &Self,
    ) -> Result<Self, TuningWriteFailure> {
        let desired = Self::from_settings(settings);
        desired.write_at(expected, &Self::paths(), write_or_remove)
    }

    fn write_at(
        &self,
        expected: &Self,
        paths: &[PathBuf; 2],
        mut writer: impl FnMut(&Path, &str) -> io::Result<()>,
    ) -> Result<Self, TuningWriteFailure> {
        let observed = Self::read_at(paths).map_err(|error| TuningWriteFailure {
            persisted: None,
            error,
        })?;
        if observed != *expected {
            return Err(TuningWriteFailure {
                persisted: None,
                error: io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Audio configuration changed outside this window. Close and reopen settings before applying.",
                ),
            });
        }
        let mut known = expected.clone();
        for (index, path) in paths.iter().enumerate() {
            if known.files[index] == self.files[index] {
                continue;
            }
            let body = std::str::from_utf8(self.files[index].as_deref().unwrap_or_default())
                .expect("tuning renderer produces UTF-8");
            if let Err(error) = writer(path, body) {
                // Atomic rename may have succeeded before directory fsync failed.
                // Recognize only our exact rendered bytes, never an unrelated edit.
                if read_optional(path).is_ok_and(|bytes| bytes == self.files[index]) {
                    known.files[index].clone_from(&self.files[index]);
                }
                return Err(TuningWriteFailure {
                    persisted: Some(known),
                    error,
                });
            }
            known.files[index].clone_from(&self.files[index]);
        }
        Ok(known)
    }
}

fn optional_bytes(text: &str) -> Option<Vec<u8>> {
    (!text.trim().is_empty()).then(|| text.as_bytes().to_vec())
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(dir: &tempfile::TempDir) -> [PathBuf; 2] {
        [dir.path().join("pw.conf"), dir.path().join("wp.conf")]
    }

    #[test]
    fn partial_write_can_be_retried_against_our_exact_persisted_revision() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let initial = TuningRevision::default();
        let desired = TuningRevision::from_settings(&UserTweaks {
            quantum: Some(512),
            headroom_usb: Some(1024),
            ..UserTweaks::default()
        });
        let error = desired
            .write_at(&initial, &paths, |path, body| {
                if path == paths[1] {
                    return Err(io::Error::other("injected disk error"));
                }
                write_or_remove(path, body)
            })
            .unwrap_err();
        let partial = error.persisted.unwrap();
        assert_eq!(TuningRevision::read_at(&paths).unwrap(), partial);
        assert_ne!(partial, desired);
        assert_eq!(
            desired.write_at(&partial, &paths, write_or_remove).unwrap(),
            desired
        );
    }

    #[test]
    fn external_edits_are_not_absorbed_as_our_successful_write() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let before = TuningRevision::default();
        std::fs::write(&paths[0], "# another application\n").unwrap();
        let desired = TuningRevision::from_settings(&UserTweaks {
            quantum: Some(512),
            ..UserTweaks::default()
        });
        let error = desired
            .write_at(&before, &paths, write_or_remove)
            .unwrap_err();
        assert_eq!(error.error.kind(), io::ErrorKind::WouldBlock);
        assert!(error.persisted.is_none());
        assert_eq!(
            std::fs::read_to_string(&paths[0]).unwrap(),
            "# another application\n"
        );
    }

    #[test]
    fn a_failed_restart_does_not_invalidate_the_persisted_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let desired = TuningRevision::from_settings(&UserTweaks {
            quantum: Some(512),
            ..UserTweaks::default()
        });
        let persisted = desired
            .write_at(&TuningRevision::default(), &paths, write_or_remove)
            .unwrap();
        // Retry after a service failure must be idempotent and perform no writes.
        let retried = desired
            .write_at(&persisted, &paths, |_, _| {
                panic!("unchanged file rewritten")
            })
            .unwrap();
        assert_eq!(retried, desired);
    }
}
