// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serialized Apply/Health correlation for the microphone Relm4 root.

use super::state::{ApplyGeneration, ApplyRequest, ApplyRevision};

#[derive(Debug, Default)]
pub(super) struct SettingsLoadTracker {
    pub(super) generation: u64,
    pub(super) pending: Option<u64>,
}

impl SettingsLoadTracker {
    pub(super) fn begin(&mut self) -> Option<u64> {
        let generation = self.generation.checked_add(1)?;
        self.generation = generation;
        self.pending = Some(generation);
        Some(generation)
    }

    pub(super) fn accept(&mut self, generation: u64) -> bool {
        if self.pending == Some(generation) {
            self.pending = None;
            true
        } else {
            false
        }
    }
}

/// Correlation identity for a health probe started after an apply revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HealthRequest {
    pub(super) revision: ApplyRevision,
    pub(super) generation: ApplyGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveWork {
    Apply(ApplyRequest),
    Health(HealthRequest),
}

#[derive(Debug, Default)]
pub(super) struct ApplyTracker {
    revision: ApplyRevision,
    generation: ApplyGeneration,
    applied_revision: Option<ApplyRevision>,
    active: Option<ActiveWork>,
    ready_revision: Option<ApplyRevision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ApplyTrackingCompletion {
    pub(super) is_settled: bool,
    pub(super) next: Option<ApplyRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HealthTrackingCompletion {
    pub(super) is_current: bool,
    pub(super) next: Option<ApplyRequest>,
}

impl ApplyTracker {
    pub(super) fn settings_changed(&mut self) -> ApplyRevision {
        self.revision = self.revision.next();
        self.ready_revision = None;
        self.revision
    }

    #[cfg(test)]
    pub(super) fn has_active_apply(&self) -> bool {
        matches!(self.active, Some(ActiveWork::Apply(_)))
    }

    pub(super) fn has_active_health(&self) -> bool {
        matches!(self.active, Some(ActiveWork::Health(_)))
    }

    pub(super) fn is_current_revision(&self, revision: ApplyRevision) -> bool {
        self.revision == revision
    }

    pub(super) fn mark_current_ready(&mut self) -> Option<ApplyRequest> {
        self.mark_ready(self.revision)
    }

    pub(super) fn mark_ready(&mut self, revision: ApplyRevision) -> Option<ApplyRequest> {
        if revision != self.revision {
            return None;
        }
        self.ready_revision = Some(revision);
        self.start_ready_apply()
    }

    fn start_ready_apply(&mut self) -> Option<ApplyRequest> {
        if self.active.is_some() {
            return None;
        }
        let revision = self.ready_revision.take()?;
        self.generation = self.generation.next();
        let request = ApplyRequest::new(revision, self.generation);
        self.active = Some(ActiveWork::Apply(request));
        Some(request)
    }

    pub(super) fn complete_apply(
        &mut self,
        request: ApplyRequest,
    ) -> Option<ApplyTrackingCompletion> {
        if self.active != Some(ActiveWork::Apply(request)) {
            return None;
        }
        self.active = None;
        let is_latest_revision = request.revision() == self.revision;
        let next = self.start_ready_apply();
        Some(ApplyTrackingCompletion {
            is_settled: is_latest_revision && next.is_none(),
            next,
        })
    }

    pub(super) fn record_applied(&mut self, request: ApplyRequest) {
        self.applied_revision = Some(request.revision());
    }

    pub(super) fn begin_health(&mut self) -> Option<HealthRequest> {
        if self.active.is_some()
            || self.ready_revision.is_some()
            || self.applied_revision != Some(self.revision)
        {
            return None;
        }
        self.generation = self.generation.next();
        let request = HealthRequest {
            revision: self.revision,
            generation: self.generation,
        };
        self.active = Some(ActiveWork::Health(request));
        Some(request)
    }

    pub(super) fn complete_health(
        &mut self,
        request: HealthRequest,
    ) -> Option<HealthTrackingCompletion> {
        if self.active != Some(ActiveWork::Health(request)) {
            return None;
        }
        self.active = None;
        let is_current = request.revision == self.revision;
        let next = self.start_ready_apply();
        Some(HealthTrackingCompletion { is_current, next })
    }
}
