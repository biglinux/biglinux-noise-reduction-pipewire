// SPDX-License-Identifier: MIT

//! Shared protocol primitives for BigLinux per-user microdaemons.
//!
//! The framework uses small GTK-free daemons for durable work that must survive
//! a host or module crash. This module defines the stable command/event frame
//! those daemons share before each daemon adds its own job-specific arguments.
//!
//! The wire representation is a four-byte big-endian length prefix followed by
//! a JSON frame. It is intentionally boring: easy to inspect in tests, stable
//! across languages, and independent of GTK/Relm4.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Current microdaemon protocol version.
pub const MICRODAEMON_PROTOCOL_VERSION: u16 = 1;

/// Maximum accepted JSON frame bytes after the four-byte length prefix.
pub const MAX_MICRODAEMON_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Official daemon identifiers covered by the shared protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MicrodaemonId {
    /// Durable terminal PTY session daemon.
    BigTerminalPtyd,
    /// Durable file operation daemon.
    BigFileopsd,
    /// Audio/video conversion daemon.
    BigConvertd,
    /// Screen/camera/microphone recording daemon.
    BigRecordd,
    /// Camera/device capture daemon.
    BigCamerad,
    /// BigShell durable session-state daemon.
    BigShellSessiond,
    /// Optional file/media indexing daemon.
    BigIndexd,
}

impl MicrodaemonId {
    /// Stable manifest and socket identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BigTerminalPtyd => "big-terminal-ptyd",
            Self::BigFileopsd => "big-fileopsd",
            Self::BigConvertd => "big-convertd",
            Self::BigRecordd => "big-recordd",
            Self::BigCamerad => "big-camerad",
            Self::BigShellSessiond => "big-shell-sessiond",
            Self::BigIndexd => "big-indexd",
        }
    }

    /// Socket file name under the daemon's private runtime directory.
    #[must_use]
    pub const fn socket_file_name(self) -> &'static str {
        match self {
            Self::BigTerminalPtyd => "ptyd.sock",
            Self::BigFileopsd => "fileopsd.sock",
            Self::BigConvertd => "convertd.sock",
            Self::BigRecordd => "recordd.sock",
            Self::BigCamerad => "camerad.sock",
            Self::BigShellSessiond => "shell-sessiond.sock",
            Self::BigIndexd => "indexd.sock",
        }
    }
}

/// A durable job submitted to one of the microdaemons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DurableJobSpec {
    /// Caller-provided idempotency key. Daemons may reuse it as the public job id.
    pub job_key: String,
    /// Daemon-local class such as `copy`, `video-conversion`, or `screen-recording`.
    pub job_class: String,
    /// Whether the job should continue without an attached UI client.
    pub is_durable: bool,
    /// Job-specific JSON arguments owned by the target daemon.
    pub arguments: serde_json::Value,
}

/// Current known state of a daemon job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DurableJobState {
    /// Accepted but not started.
    Queued,
    /// Actively running.
    Running,
    /// Cancel request has been accepted and cleanup is in progress.
    Cancelling,
    /// Completed successfully.
    Completed,
    /// Stopped before completion by an explicit cancel request.
    Cancelled,
    /// Interrupted by daemon restart or external failure.
    Interrupted,
    /// Failed and requires caller-visible recovery or retry.
    Failed,
}

/// Snapshot returned by list/attach operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DurableJobSnapshot {
    /// Stable daemon job id.
    pub job_id: String,
    /// Original daemon-local job class.
    pub job_class: String,
    /// Current state.
    pub state: DurableJobState,
    /// Last known progress, when the daemon can estimate it.
    pub progress: Option<JobProgress>,
    /// Optional short user-facing status owned by the daemon.
    pub status_text: Option<String>,
    /// Optional daemon-owned structured details for richer clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Journal row pairing a public snapshot with the original restartable spec.
#[derive(Debug, Clone, PartialEq)]
pub struct DurableJobJournalRecord {
    /// Last persisted job snapshot.
    pub snapshot: DurableJobSnapshot,
    /// Original validated job spec, when the daemon supports restart.
    pub restart_spec: Option<DurableJobSpec>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DurableJobJournal {
    version: u32,
    jobs: Vec<DurableJobSnapshot>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    restart_specs: HashMap<String, DurableJobSpec>,
}

/// Load a durable-job journal shared by the BigLinux microdaemons.
///
/// Corrupt or unsupported journals are quarantined by renaming the original file
/// to a `.corrupt.<pid>-<nanos>` sibling and returning an empty set. State
/// recovery policy remains daemon-owned: callers decide which non-terminal
/// states become interrupted and how domain-specific details are reconciled.
///
/// # Errors
///
/// Returns filesystem errors other than missing journal files.
pub fn load_durable_job_journal(
    journal_path: &Path,
    accepted_versions: &[u32],
) -> io::Result<Vec<DurableJobJournalRecord>> {
    let content = match fs::read_to_string(journal_path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let journal = match serde_json::from_str::<DurableJobJournal>(&content) {
        Ok(journal) if accepted_versions.contains(&journal.version) => journal,
        _ => {
            quarantine_corrupt_journal(journal_path);
            return Ok(Vec::new());
        }
    };
    Ok(journal
        .jobs
        .into_iter()
        .map(|snapshot| DurableJobJournalRecord {
            restart_spec: journal.restart_specs.get(&snapshot.job_id).cloned(),
            snapshot,
        })
        .collect())
}

/// Persist a durable-job journal atomically.
///
/// The write uses create-new temp files, `fsync` on the file, atomic rename, and
/// `fsync` on the parent directory. On write failure the temp file is removed.
///
/// # Errors
///
/// Returns serialization or filesystem errors.
pub fn persist_durable_job_journal(
    journal_path: &Path,
    version: u32,
    records: &[DurableJobJournalRecord],
) -> io::Result<()> {
    let Some(parent) = journal_path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "journal path has no parent",
        ));
    };
    fs::create_dir_all(parent)?;
    let temp_path = journal_temp_path(journal_path);
    let journal = DurableJobJournal {
        version,
        jobs: records
            .iter()
            .map(|record| record.snapshot.clone())
            .collect(),
        restart_specs: records
            .iter()
            .filter_map(|record| {
                record
                    .restart_spec
                    .clone()
                    .map(|spec| (record.snapshot.job_id.clone(), spec))
            })
            .collect(),
    };
    let encoded = serde_json::to_vec_pretty(&journal).map_err(io::Error::other)?;
    let write_result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, journal_path)?;
        File::open(parent)?.sync_all()
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn journal_temp_path(journal_path: &Path) -> PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    journal_path.with_extension(format!("json.tmp.{}-{suffix}", std::process::id()))
}

fn quarantine_corrupt_journal(journal_path: &Path) {
    if !journal_path.exists() {
        return;
    }
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let quarantine_path =
        journal_path.with_extension(format!("json.corrupt.{}-{suffix}", std::process::id()));
    let _ = fs::rename(journal_path, quarantine_path);
}

/// Bounded progress report.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JobProgress {
    /// Completed work units.
    pub completed_units: u64,
    /// Total work units when known.
    pub total_units: Option<u64>,
}

impl JobProgress {
    /// Build a bounded progress report.
    ///
    /// When `total_units` is known, `completed_units` is clamped to it so UI
    /// consumers never render progress above 100%.
    #[must_use]
    pub fn new(completed_units: u64, total_units: Option<u64>) -> Self {
        let completed_units =
            total_units.map_or(completed_units, |total| completed_units.min(total));
        Self {
            completed_units,
            total_units,
        }
    }
}

/// Commands sent from clients to daemons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum MicrodaemonCommand {
    /// Protocol handshake; daemon replies with [`MicrodaemonEvent::Hello`].
    Ping,
    /// Submit a new job.
    Submit {
        /// Job to enqueue.
        job: DurableJobSpec,
    },
    /// List currently known jobs.
    List,
    /// Attach to a job and replay its current snapshot/progress.
    Attach {
        /// Stable daemon job id.
        job_id: String,
    },
    /// Cancel a job.
    Cancel {
        /// Stable daemon job id.
        job_id: String,
    },
    /// Restart a failed, interrupted, or completed job using daemon-owned state.
    Restart {
        /// Stable daemon job id.
        job_id: String,
    },
    /// Ask the daemon to recover/reconcile an interrupted job from its journal.
    Recover {
        /// Stable daemon job id.
        job_id: String,
    },
    /// Acknowledge cleanup of a completed/cancelled/failed job.
    AckCleanup {
        /// Stable daemon job id.
        job_id: String,
    },
    /// Ask the daemon to drain/cancel according to its clean-shutdown policy.
    Shutdown,
}

/// Events sent by daemons to clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum MicrodaemonEvent {
    /// Protocol handshake response.
    Hello {
        /// Daemon id that accepted the connection.
        daemon_id: MicrodaemonId,
        /// Human-readable capability tokens.
        capabilities: Vec<String>,
    },
    /// A submitted job was accepted.
    JobAccepted {
        /// Stable daemon job id.
        job_id: String,
    },
    /// Full job snapshot.
    JobSnapshot {
        /// Snapshot contents.
        snapshot: DurableJobSnapshot,
    },
    /// Full response to [`MicrodaemonCommand::List`].
    JobList {
        /// Current daemon jobs visible to the caller.
        jobs: Vec<DurableJobSnapshot>,
    },
    /// Progress-only update.
    Progress {
        /// Stable daemon job id.
        job_id: String,
        /// Latest progress.
        progress: JobProgress,
    },
    /// Terminal state update for a job.
    JobStateChanged {
        /// Stable daemon job id.
        job_id: String,
        /// New state.
        state: DurableJobState,
        /// Optional short diagnostic owned by the daemon.
        status_text: Option<String>,
    },
    /// Clean shutdown accepted.
    ShutdownAccepted,
    /// Protocol or job error.
    Error {
        /// Job id when the error belongs to a specific job.
        job_id: Option<String>,
        /// Stable machine-readable error code.
        code: String,
        /// Short diagnostic safe to log.
        message: String,
    },
}

/// Either a client command or a daemon event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MicrodaemonBody {
    /// Client-to-daemon command.
    Command {
        /// Command body.
        command: MicrodaemonCommand,
    },
    /// Daemon-to-client event.
    Event {
        /// Event body.
        event: MicrodaemonEvent,
    },
}

/// Versioned protocol frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicrodaemonFrame {
    /// Protocol version.
    pub version: u16,
    /// Target or source daemon id.
    pub daemon_id: MicrodaemonId,
    /// Frame body.
    pub body: MicrodaemonBody,
}

impl MicrodaemonFrame {
    /// Build a current-version command frame.
    #[must_use]
    pub const fn command(daemon_id: MicrodaemonId, command: MicrodaemonCommand) -> Self {
        Self {
            version: MICRODAEMON_PROTOCOL_VERSION,
            daemon_id,
            body: MicrodaemonBody::Command { command },
        }
    }

    /// Build a current-version event frame.
    #[must_use]
    pub const fn event(daemon_id: MicrodaemonId, event: MicrodaemonEvent) -> Self {
        Self {
            version: MICRODAEMON_PROTOCOL_VERSION,
            daemon_id,
            body: MicrodaemonBody::Event { event },
        }
    }

    /// Validate the protocol version.
    ///
    /// # Errors
    ///
    /// Returns [`MicrodaemonProtocolError::UnsupportedVersion`] when the frame
    /// was produced by a newer or invalid protocol.
    pub fn validate_version(&self) -> Result<(), MicrodaemonProtocolError> {
        if self.version == MICRODAEMON_PROTOCOL_VERSION {
            Ok(())
        } else {
            Err(MicrodaemonProtocolError::UnsupportedVersion {
                found: self.version,
                supported: MICRODAEMON_PROTOCOL_VERSION,
            })
        }
    }
}

/// Protocol codec and validation failures.
#[derive(Debug)]
pub enum MicrodaemonProtocolError {
    /// JSON serialize/parse failed.
    Json(serde_json::Error),
    /// Frame length is larger than [`MAX_MICRODAEMON_FRAME_BYTES`].
    FrameTooLarge {
        /// Actual frame bytes.
        actual: usize,
        /// Maximum accepted frame bytes.
        maximum: usize,
    },
    /// Buffer does not contain the promised frame bytes.
    TruncatedFrame {
        /// Number of bytes required by the length prefix plus header.
        needed: usize,
        /// Number of bytes supplied by the caller.
        available: usize,
    },
    /// Frame version is unsupported.
    UnsupportedVersion {
        /// Version found in the frame.
        found: u16,
        /// Highest version supported by this build.
        supported: u16,
    },
}

impl std::fmt::Display for MicrodaemonProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(error) => write!(f, "microdaemon json error: {error}"),
            Self::FrameTooLarge { actual, maximum } => {
                write!(
                    f,
                    "microdaemon frame has {actual} bytes; maximum is {maximum}"
                )
            }
            Self::TruncatedFrame { needed, available } => write!(
                f,
                "microdaemon frame needs {needed} bytes but only {available} bytes were supplied"
            ),
            Self::UnsupportedVersion { found, supported } => {
                write!(
                    f,
                    "microdaemon protocol version {found} is unsupported; expected {supported}"
                )
            }
        }
    }
}

impl std::error::Error for MicrodaemonProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::FrameTooLarge { .. }
            | Self::TruncatedFrame { .. }
            | Self::UnsupportedVersion { .. } => None,
        }
    }
}

impl From<serde_json::Error> for MicrodaemonProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Encode a frame as a four-byte length prefix followed by JSON bytes.
///
/// # Errors
///
/// Returns [`MicrodaemonProtocolError::FrameTooLarge`] when the serialized JSON
/// exceeds [`MAX_MICRODAEMON_FRAME_BYTES`], or [`MicrodaemonProtocolError::Json`]
/// when serialization fails.
pub fn encode_microdaemon_frame(
    frame: &MicrodaemonFrame,
) -> Result<Vec<u8>, MicrodaemonProtocolError> {
    frame.validate_version()?;
    let body = serde_json::to_vec(frame)?;
    if body.len() > MAX_MICRODAEMON_FRAME_BYTES {
        return Err(MicrodaemonProtocolError::FrameTooLarge {
            actual: body.len(),
            maximum: MAX_MICRODAEMON_FRAME_BYTES,
        });
    }
    let mut encoded = Vec::with_capacity(4 + body.len());
    encoded.extend_from_slice(&(body.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&body);
    Ok(encoded)
}

/// Decode one complete length-prefixed frame.
///
/// # Errors
///
/// Returns [`MicrodaemonProtocolError::TruncatedFrame`] for incomplete buffers,
/// [`MicrodaemonProtocolError::FrameTooLarge`] for oversized frames,
/// [`MicrodaemonProtocolError::Json`] for malformed JSON, or
/// [`MicrodaemonProtocolError::UnsupportedVersion`] for version mismatch.
pub fn decode_microdaemon_frame(
    bytes: &[u8],
) -> Result<MicrodaemonFrame, MicrodaemonProtocolError> {
    if bytes.len() < 4 {
        return Err(MicrodaemonProtocolError::TruncatedFrame {
            needed: 4,
            available: bytes.len(),
        });
    }
    let length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if length > MAX_MICRODAEMON_FRAME_BYTES {
        return Err(MicrodaemonProtocolError::FrameTooLarge {
            actual: length,
            maximum: MAX_MICRODAEMON_FRAME_BYTES,
        });
    }
    let needed = 4 + length;
    if bytes.len() < needed {
        return Err(MicrodaemonProtocolError::TruncatedFrame {
            needed,
            available: bytes.len(),
        });
    }
    let frame: MicrodaemonFrame = serde_json::from_slice(&bytes[4..needed])?;
    frame.validate_version()?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_frame_round_trips_through_length_prefixed_json() {
        let frame = MicrodaemonFrame::command(
            MicrodaemonId::BigFileopsd,
            MicrodaemonCommand::Submit {
                job: DurableJobSpec {
                    job_key: "copy-1".to_owned(),
                    job_class: "copy".to_owned(),
                    is_durable: true,
                    arguments: serde_json::json!({
                        "source": "/tmp/source",
                        "target": "/tmp/target"
                    }),
                },
            },
        );

        let encoded = encode_microdaemon_frame(&frame).expect("frame encodes");
        let decoded = decode_microdaemon_frame(&encoded).expect("frame decodes");

        assert_eq!(decoded, frame);
    }

    #[test]
    fn list_event_round_trips_empty_job_set() {
        let frame = MicrodaemonFrame::event(
            MicrodaemonId::BigFileopsd,
            MicrodaemonEvent::JobList { jobs: Vec::new() },
        );

        let encoded = encode_microdaemon_frame(&frame).expect("frame encodes");
        let decoded = decode_microdaemon_frame(&encoded).expect("frame decodes");

        assert_eq!(decoded, frame);
    }

    #[test]
    fn restart_and_recover_commands_round_trip() {
        for command in [
            MicrodaemonCommand::Restart {
                job_id: "job-1".to_owned(),
            },
            MicrodaemonCommand::Recover {
                job_id: "job-1".to_owned(),
            },
        ] {
            let frame = MicrodaemonFrame::command(MicrodaemonId::BigConvertd, command);
            let encoded = encode_microdaemon_frame(&frame).expect("frame encodes");
            let decoded = decode_microdaemon_frame(&encoded).expect("frame decodes");

            assert_eq!(decoded, frame);
        }
    }

    #[test]
    fn decode_rejects_truncated_prefix_and_body() {
        let short = decode_microdaemon_frame(&[0, 0, 0]);
        assert!(matches!(
            short,
            Err(MicrodaemonProtocolError::TruncatedFrame {
                needed: 4,
                available: 3
            })
        ));

        let truncated = decode_microdaemon_frame(&[0, 0, 0, 8, b'{', b'}']);
        assert!(matches!(
            truncated,
            Err(MicrodaemonProtocolError::TruncatedFrame {
                needed: 12,
                available: 6
            })
        ));
    }

    #[test]
    fn decode_rejects_unsupported_version() {
        let mut frame =
            MicrodaemonFrame::command(MicrodaemonId::BigConvertd, MicrodaemonCommand::Ping);
        frame.version = MICRODAEMON_PROTOCOL_VERSION + 1;
        let body = serde_json::to_vec(&frame).expect("manual frame serializes");
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&(body.len() as u32).to_be_bytes());
        encoded.extend_from_slice(&body);

        let decoded = decode_microdaemon_frame(&encoded);

        assert!(matches!(
            decoded,
            Err(MicrodaemonProtocolError::UnsupportedVersion {
                found,
                supported: MICRODAEMON_PROTOCOL_VERSION
            }) if found == MICRODAEMON_PROTOCOL_VERSION + 1
        ));
    }

    #[test]
    fn progress_clamps_completed_units_to_known_total() {
        assert_eq!(
            JobProgress::new(120, Some(100)),
            JobProgress {
                completed_units: 100,
                total_units: Some(100),
            }
        );
    }

    #[test]
    fn durable_job_journal_round_trips_restart_specs() {
        let dir = std::env::temp_dir().join(format!("big-os-kit-journal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let journal_path = dir.join("jobs.json");
        let spec = DurableJobSpec {
            job_key: "copy-1".to_owned(),
            job_class: "copy".to_owned(),
            is_durable: true,
            arguments: serde_json::json!({"source": "/tmp/a", "target": "/tmp/b"}),
        };
        let records = [DurableJobJournalRecord {
            snapshot: DurableJobSnapshot {
                job_id: "copy-1".to_owned(),
                job_class: "copy".to_owned(),
                state: DurableJobState::Running,
                progress: Some(JobProgress::new(1, Some(2))),
                status_text: Some("copying".to_owned()),
                details: None,
            },
            restart_spec: Some(spec.clone()),
        }];

        persist_durable_job_journal(&journal_path, 2, &records).expect("persist journal");
        let loaded =
            load_durable_job_journal(&journal_path, &[1, 2]).expect("load persisted journal");

        assert_eq!(loaded, records);
        assert_eq!(loaded[0].restart_spec, Some(spec));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn durable_job_journal_quarantines_corrupt_file() {
        let dir =
            std::env::temp_dir().join(format!("big-os-kit-corrupt-journal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let journal_path = dir.join("jobs.json");
        std::fs::write(&journal_path, "{not-json").expect("write corrupt journal");

        let loaded =
            load_durable_job_journal(&journal_path, &[1, 2]).expect("load corrupt journal");

        assert!(loaded.is_empty());
        assert!(!journal_path.exists());
        let quarantine_count = std::fs::read_dir(&dir)
            .expect("read temp dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains("corrupt"))
            .count();
        assert_eq!(quarantine_count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn daemon_ids_match_contract_socket_names() {
        let pairs = [
            (
                MicrodaemonId::BigTerminalPtyd,
                "big-terminal-ptyd",
                "ptyd.sock",
            ),
            (MicrodaemonId::BigFileopsd, "big-fileopsd", "fileopsd.sock"),
            (MicrodaemonId::BigConvertd, "big-convertd", "convertd.sock"),
            (MicrodaemonId::BigRecordd, "big-recordd", "recordd.sock"),
            (MicrodaemonId::BigCamerad, "big-camerad", "camerad.sock"),
            (
                MicrodaemonId::BigShellSessiond,
                "big-shell-sessiond",
                "shell-sessiond.sock",
            ),
            (MicrodaemonId::BigIndexd, "big-indexd", "indexd.sock"),
        ];

        for (daemon_id, expected_id, expected_socket) in pairs {
            assert_eq!(daemon_id.as_str(), expected_id);
            assert_eq!(daemon_id.socket_file_name(), expected_socket);
        }
    }
}
