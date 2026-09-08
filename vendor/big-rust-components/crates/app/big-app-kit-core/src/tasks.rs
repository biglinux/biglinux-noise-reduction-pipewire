// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Long-running task contracts.

/// Lifecycle stage of a background task tracked by the task runner.
///
/// The state machine is linear: queued → running → (cancelling)? →
/// (succeeded | failed | cancelled). [`BigTaskProgress::is_terminal`]
/// reports whether a value belongs to the last three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigTaskState {
    /// Task is registered with the runner but no worker has picked it up
    /// yet.
    Queued,
    /// Worker is actively making progress; UI should show a spinner or
    /// progress bar.
    Running,
    /// User requested cancellation; the worker has acknowledged but not
    /// yet stopped. Used so UIs can dim the cancel button.
    Cancelling,
    /// Terminal state when the work completed without error.
    Succeeded,
    /// Terminal state when the worker reported an error.
    Failed,
    /// Terminal state reached when cancellation completed.
    Cancelled,
}

/// Progress sample emitted by the task runner to the UI.
///
/// Workers produce these on a throttled cadence so progress bars do not
/// thrash. `fraction = None` means "indeterminate" — the UI should fall
/// back to a pulsing spinner.
#[derive(Debug, Clone, PartialEq)]
pub struct BigTaskProgress {
    /// Current lifecycle position.
    pub state: BigTaskState,
    /// Completion fraction in `0.0..=1.0`, clamped on construction.
    /// `None` means progress is indeterminate.
    pub fraction: Option<f64>,
    /// Short human-readable status (`"Encoding chapter 2"`); empty when
    /// the worker has nothing to say.
    pub message: String,
}

impl BigTaskProgress {
    /// Construct a progress sample with no fraction. Use
    /// [`BigTaskProgress::fraction`] to attach a determinate value.
    #[must_use]
    pub fn new(state: BigTaskState, message: impl Into<String>) -> Self {
        Self {
            state,
            fraction: None,
            message: message.into(),
        }
    }

    /// Attach a clamped completion fraction. Inputs outside `0.0..=1.0`
    /// are clamped so adapters never have to defend against bogus
    /// progress bars.
    #[must_use]
    pub fn fraction(mut self, value: f64) -> Self {
        self.fraction = Some(value.clamp(0.0, 1.0));
        self
    }

    /// `true` once the task has reached `Succeeded`, `Failed`, or
    /// `Cancelled`. The runner uses this to drop the task from its
    /// active set.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            BigTaskState::Succeeded | BigTaskState::Failed | BigTaskState::Cancelled
        )
    }
}

/// Display-free description of a single task type.
///
/// Apps define one spec per kind of work (convert, sync, scan) and feed
/// it into the runner. [`BigTaskSpec::resolved`] computes which UI
/// affordances the spec implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTaskSpec {
    /// Stable identifier surfaced in logs and persisted state.
    pub id: String,
    /// Localized title shown next to the progress bar.
    pub title: String,
    /// `true` when the user is allowed to cancel mid-flight.
    pub cancellable: bool,
    /// `true` when failure should produce a "Retry" affordance.
    pub retryable: bool,
    /// `true` when the worker emits log lines that need redaction
    /// before being shown.
    pub emits_diagnostics: bool,
}

impl BigTaskSpec {
    /// Construct a spec with all three behavioural flags enabled.
    /// Callers disable them explicitly via [`BigTaskSpec::cancellable`]
    /// and [`BigTaskSpec::retryable`] when appropriate.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            cancellable: true,
            retryable: true,
            emits_diagnostics: true,
        }
    }

    /// Override [`BigTaskSpec::cancellable`].
    #[must_use]
    pub fn cancellable(mut self, enabled: bool) -> Self {
        self.cancellable = enabled;
        self
    }

    /// Override [`BigTaskSpec::retryable`].
    #[must_use]
    pub fn retryable(mut self, enabled: bool) -> Self {
        self.retryable = enabled;
        self
    }

    /// Compute the [`BigTaskResolved`] derived from this spec. Each
    /// boolean tells the widget adapter whether to wire up a worker
    /// thread, a cancel output, a retry output, and a redacted log
    /// sink.
    #[must_use]
    pub fn resolved(&self) -> BigTaskResolved {
        BigTaskResolved {
            needs_worker: true,
            needs_cancel_output: self.cancellable,
            needs_retry_output: self.retryable,
            needs_log_redaction: self.emits_diagnostics,
        }
    }
}

/// Pre-computed flags telling a Relm4 adapter which channels to wire
/// for a [`BigTaskSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTaskResolved {
    /// `true` when the adapter should spawn a background worker rather
    /// than running the task inline on the UI thread.
    pub needs_worker: bool,
    /// `true` when the adapter should expose a cancel output the host
    /// can wire to a "Cancel" button.
    pub needs_cancel_output: bool,
    /// `true` when the adapter should expose a retry output that the
    /// host wires to a "Retry" button after failure.
    pub needs_retry_output: bool,
    /// `true` when log lines flowing through the adapter need to pass
    /// the redaction filter before reaching the UI.
    pub needs_log_redaction: bool,
}

/// Display-free description of a queue of homogeneous jobs.
///
/// Used for converter queues, batch downloads, and similar features
/// where the user adds work items and the runner processes them with
/// bounded concurrency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigJobQueueSpec {
    /// Localized header shown above the queue.
    pub title: String,
    /// Maximum number of jobs the runner may execute at once. Clamped
    /// to at least 1 by [`BigJobQueueSpec::concurrent_jobs`].
    pub concurrent_jobs: usize,
    /// `true` when the UI should let the user drag rows to reorder
    /// pending work.
    pub allow_reorder: bool,
}

impl BigJobQueueSpec {
    /// Construct a queue spec with serial execution
    /// (`concurrent_jobs = 1`) and reorder enabled.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            concurrent_jobs: 1,
            allow_reorder: true,
        }
    }

    /// Override the concurrency cap. Values below 1 are coerced to 1
    /// so the queue is always able to drain.
    #[must_use]
    pub fn concurrent_jobs(mut self, value: usize) -> Self {
        self.concurrent_jobs = value.max(1);
        self
    }
}

/// Display-free description of a diagnostics surface (error dialog,
/// log viewer) emitted by a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDiagnosticsSpec {
    /// Localized header shown in the diagnostics surface.
    pub title: String,
    /// `true` when the UI must expose a "Copy" affordance for the
    /// diagnostic text.
    pub copyable: bool,
    /// `true` when the diagnostic text must pass through the secret
    /// redactor before being shown or copied.
    pub redact_sensitive_values: bool,
}

impl BigDiagnosticsSpec {
    /// Construct a spec with copy and redaction both enabled — the
    /// secure default for support-bundle exports.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            copyable: true,
            redact_sensitive_values: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_fraction_is_clamped() {
        let progress = BigTaskProgress::new(BigTaskState::Running, "work").fraction(2.0);
        assert_eq!(progress.fraction, Some(1.0));
    }

    #[test]
    fn progress_terminal_state_contract_separates_active_and_finished_work() {
        for active_state in [
            BigTaskState::Queued,
            BigTaskState::Running,
            BigTaskState::Cancelling,
        ] {
            assert!(!BigTaskProgress::new(active_state, "active").is_terminal());
        }

        for finished_state in [
            BigTaskState::Succeeded,
            BigTaskState::Failed,
            BigTaskState::Cancelled,
        ] {
            assert!(BigTaskProgress::new(finished_state, "finished").is_terminal());
        }
    }

    #[test]
    fn default_task_requires_worker_cancel_retry_and_redaction() {
        let resolved = BigTaskSpec::new("convert", "Convert").resolved();
        assert!(resolved.needs_worker);
        assert!(resolved.needs_cancel_output);
        assert!(resolved.needs_retry_output);
        assert!(resolved.needs_log_redaction);
    }

    #[test]
    fn job_queue_clamps_concurrency() {
        let queue = BigJobQueueSpec::new("Jobs").concurrent_jobs(0);
        assert_eq!(queue.concurrent_jobs, 1);
    }
}
