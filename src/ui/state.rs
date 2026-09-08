//! Shared UI state + worker-owned "apply" pipeline.
//!
//! The Relm4 root owns this main-thread object. Workers receive owned
//! [`ApplyWork`] values and return typed [`ApplyCompletion`] values; there is
//! no callback bag, parallel settings snapshot, or `Arc<Mutex>` state. Two
//! concerns live side by side:
//!
//! 1. The typed [`AppSettings`] snapshot the user is editing.
//! 2. A remembered copy of the last-applied settings so we can detect
//!    mic topology changes without restarting services unnecessarily.
//!
//! The "apply" pipeline runs in four distinct tiers, sorted by how
//! intrusive they are:
//!
//! | Tier | Example change | Cost |
//! |------|----------------|------|
//! | 1. Save settings | any field | ~1 ms disk write, zero audio impact |
//! | 2. Rewrite drop-ins | any field | ~2 ms disk write, zero audio impact (next login only) |
//! | 3. Push live Props | sliders, toggles on existing nodes | **zero audio interruption** |
//! | 4. Restart mic unit | first-time load, EQ topology change | brief (~400 ms) drop of `mic-biglinux` virtual source only |
//!
//! Each chain runs in its own `biglinux-microphone-pwloader` process,
//! so the lifecycles are independent: mic-chain topology change restarts
//! `biglinux-microphone-mic.service` only, AEC toggle restarts
//! `biglinux-microphone-aec.service` only, and master output toggle
//! restarts `biglinux-microphone-output.service` only. WirePlumber is
//! **never restarted**.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use log::{debug, error, info};

use crate::config::AppSettings;
use crate::services::loopback::Loopback;
#[cfg(test)]
use crate::services::reconcile::{needs_mic_reload, output_topology_changed};

const MAX_UNCERTAIN_LOCAL_WRITES: usize = 8;

/// Settings revision attached to one apply operation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ApplyRevision(u64);

impl ApplyRevision {
    pub(super) fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("audio settings revision space exhausted"),
        )
    }
}

/// Unique generation for one worker or health-probe command.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ApplyGeneration(u64);

impl ApplyGeneration {
    pub(super) fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("audio apply generation space exhausted"),
        )
    }
}

/// Correlation identity for one serialized apply worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ApplyRequest {
    revision: ApplyRevision,
    generation: ApplyGeneration,
}

impl ApplyRequest {
    pub(super) const fn new(revision: ApplyRevision, generation: ApplyGeneration) -> Self {
        Self {
            revision,
            generation,
        }
    }

    pub(super) const fn revision(self) -> ApplyRevision {
        self.revision
    }

    #[cfg(test)]
    pub(super) const fn generation(self) -> ApplyGeneration {
        self.generation
    }
}

/// What the main thread should do once an offloaded apply pass completes.
enum ApplyStatus {
    /// Full Tier 1–4 sequence ran — record the snapshot as last-applied.
    Applied,
    /// Some step failed. The snapshot is *not* recorded as applied. The typed
    /// completion carries the cause to the shell, which exposes a deliberate
    /// "Try again" action rather than creating a silent retry loop.
    Failed(String),
}

/// Result of an offloaded apply pass, carried back to the main thread.
struct ApplyOutcome {
    /// The settings snapshot processed by the worker.
    snapshot: AppSettings,
    /// Self-listen loopback handle after reconciliation — possibly the same
    /// one passed in, a freshly spawned one, or `None`.
    loopback: Option<Loopback>,
    /// The settings snapshot reached durable storage even if a later live or
    /// service step failed.
    was_persisted: bool,
    status: ApplyStatus,
}

/// Complete, owned input for one blocking apply worker.
///
/// The Relm4 root obtains this value on the GTK thread and moves it into
/// `relm4::spawn_blocking`; no callback or shared mutable state crosses the
/// worker boundary.
pub(super) struct ApplyWork {
    request: ApplyRequest,
    previous: Option<AppSettings>,
    baseline: AppSettings,
    snapshot: AppSettings,
    loopback: Option<Loopback>,
}

impl ApplyWork {
    pub(super) const fn request(&self) -> ApplyRequest {
        self.request
    }

    /// Run all blocking PipeWire/persistence work and return a typed result.
    pub(super) fn run(self) -> ApplyCompletion {
        let outcome = run_apply(self.previous, self.baseline, self.snapshot, self.loopback);
        ApplyCompletion {
            request: self.request,
            outcome: Some(outcome),
        }
    }
}

/// Typed result delivered by the Relm4 command channel.
pub(super) struct ApplyCompletion {
    request: ApplyRequest,
    /// `None` means the blocking worker panicked or was cancelled before it
    /// could return. The request identity is still delivered so the shell can
    /// release its in-flight slot and continue with a newer revision.
    outcome: Option<ApplyOutcome>,
}

/// Main-thread description of the snapshot a local worker can persist.
struct LocalApplySnapshot {
    request: ApplyRequest,
    snapshot: AppSettings,
}

impl fmt::Debug for ApplyCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplyCompletion")
            .field("request", &self.request)
            .field("worker_finished", &self.outcome.is_some())
            .finish_non_exhaustive()
    }
}

impl ApplyCompletion {
    pub(super) const fn request(&self) -> ApplyRequest {
        self.request
    }

    pub(super) const fn worker_failed(request: ApplyRequest) -> Self {
        Self {
            request,
            outcome: None,
        }
    }

    #[cfg(test)]
    fn from_outcome(request: ApplyRequest, outcome: ApplyOutcome) -> Self {
        Self {
            request,
            outcome: Some(outcome),
        }
    }
}

/// Shared GTK-main-thread state.
pub struct AppState {
    // The tuning page owns an independent draft and no Rc<AppState>. Keeping
    // this widget avoids losing unapplied choices on external JSON updates.
    tuning_page: RefCell<Option<gtk::Widget>>,
    active_page: RefCell<String>,
    settings: RefCell<AppSettings>,
    /// Last snapshot that was successfully applied. Used to decide
    /// whether the current change needs an expensive mic-chain reload
    /// or if the cheap live path is sufficient.
    last_applied: RefCell<Option<AppSettings>>,
    /// Last snapshot written by this process. File-monitor notifications for
    /// this exact payload are acknowledgements of our own atomic write, not an
    /// external request to roll newer in-memory edits back.
    last_persisted: RefCell<Option<AppSettings>>,
    /// Snapshot owned by the serialized apply worker, or the last worker whose
    /// completion was indeterminate. This closes both the normal window
    /// between `save()` and command completion and a delayed monitor event
    /// after a worker panic.
    local_apply_snapshot: RefCell<Option<LocalApplySnapshot>>,
    /// Recent workers that failed without an outcome. Any of them may have
    /// completed the atomic save before panicking, so their delayed monitor
    /// notifications remain local acknowledgements. The lane is serialized
    /// and the bounded history prevents an unbounded failure loop.
    uncertain_local_writes: RefCell<VecDeque<LocalApplySnapshot>>,
    /// `pw-loopback` subprocess used by the "hear my voice" feature.
    /// `None` when the user has self-listen off; the Drop impl handles
    /// cleanup on app shutdown.
    loopback: RefCell<Option<Loopback>>,
}

/// Owned close task: saves preferences and stops only GUI-owned resources.
/// It never queries PipeWire or starts/restarts an audio service.
pub(super) struct CloseWork {
    baseline: AppSettings,
    desired: AppSettings,
    loopback: Option<Loopback>,
}

impl CloseWork {
    pub(super) fn run(self) -> Result<(), String> {
        drop(self.loopback);
        let _guard =
            crate::config::storage::SettingsLock::acquire().map_err(|error| error.to_string())?;
        let latest = AppSettings::load_strict().map_err(|error| error.to_string())?;
        let merged = crate::config::storage::merge(&self.baseline, &self.desired, &latest)
            .map_err(|error| error.to_string())?;
        merged.save().map_err(|error| error.to_string())
    }
}

impl AppState {
    pub(super) fn tuning_page(&self) -> gtk::Widget {
        self.tuning_page.borrow_mut().get_or_insert_with(super::views::advanced::build).clone()
    }
    pub(super) fn active_page(&self) -> String { self.active_page.borrow().clone() }
    pub(super) fn remember_page(&self, name: &str) { *self.active_page.borrow_mut() = name.to_owned(); }

    pub(super) fn close_work(&self) -> CloseWork {
        CloseWork {
            baseline: self
                .last_persisted
                .borrow()
                .clone()
                .unwrap_or_else(|| self.settings.borrow().clone()),
            desired: self.settings.borrow().clone(),
            loopback: self.loopback.borrow_mut().take(),
        }
    }

    #[must_use]
    pub fn new(settings: AppSettings) -> Rc<Self> {
        let persisted = settings.clone();
        Rc::new(Self {
            tuning_page: RefCell::new(None),
            active_page: RefCell::new("mic".to_owned()),
            settings: RefCell::new(settings),
            last_applied: RefCell::new(None),
            last_persisted: RefCell::new(Some(persisted)),
            local_apply_snapshot: RefCell::new(None),
            uncertain_local_writes: RefCell::new(VecDeque::new()),
            loopback: RefCell::new(None),
        })
    }

    #[must_use]
    pub(super) fn has_active_apply(&self) -> bool {
        self.local_apply_snapshot.borrow().is_some()
    }

    pub fn settings(&self) -> std::cell::Ref<'_, AppSettings> {
        self.settings.borrow()
    }

    pub fn mutate<F: FnOnce(&mut AppSettings)>(&self, f: F) {
        {
            let mut guard = self.settings.borrow_mut();
            f(&mut guard);
        }
    }

    /// Replace the in-memory settings with a snapshot that was applied
    /// **outside** this process. Returns `true` when an actual change was
    /// absorbed so the shell can correlate a fresh apply and rebuild mirrored
    /// widgets. The last-applied snapshot is deliberately preserved until that
    /// worker succeeds, preventing an older in-flight completion from winning.
    pub fn external_replace(&self, new: AppSettings) -> bool {
        let is_local_write = self
            .local_apply_snapshot
            .borrow()
            .as_ref()
            .is_some_and(|snapshot| snapshot.snapshot == new)
            || self
                .uncertain_local_writes
                .borrow()
                .iter()
                .any(|snapshot| snapshot.snapshot == new)
            || self.last_persisted.borrow().as_ref() == Some(&new);
        if is_local_write {
            return false;
        }
        if *self.settings.borrow() == new {
            return false;
        }
        let baseline = self
            .last_persisted
            .borrow()
            .clone()
            .unwrap_or_else(|| self.settings.borrow().clone());
        let merged = crate::config::storage::merge(&baseline, &self.settings.borrow(), &new);
        match merged {
            Ok(merged) => {
                *self.last_persisted.borrow_mut() = Some(new);
                *self.settings.borrow_mut() = merged;
                true
            }
            Err(error) => {
                log::warn!("settings: preserving local edits after external conflict: {error}");
                false
            }
        }
    }

    /// Move an immutable apply snapshot and the current loopback handle into a
    /// worker-owned value. The shell serializes requests before calling this.
    pub(super) fn apply_work(&self, request: ApplyRequest) -> ApplyWork {
        let prev = self.last_applied.borrow().clone();
        let snapshot = self.settings.borrow().clone();
        *self.local_apply_snapshot.borrow_mut() = Some(LocalApplySnapshot {
            request,
            snapshot: snapshot.clone(),
        });
        ApplyWork {
            request,
            previous: prev,
            baseline: self
                .last_persisted
                .borrow()
                .clone()
                .unwrap_or_else(|| snapshot.clone()),
            snapshot,
            loopback: self.loopback.borrow_mut().take(),
        }
    }

    /// Reconcile a worker completion on the GTK thread. The tracker validates
    /// request identity before the shell calls this method.
    pub(super) fn finish_apply(&self, completion: ApplyCompletion) -> Result<(), String> {
        let local_snapshot = if self
            .local_apply_snapshot
            .borrow()
            .as_ref()
            .is_some_and(|snapshot| snapshot.request == completion.request)
        {
            self.local_apply_snapshot.borrow_mut().take()
        } else {
            None
        };
        let Some(outcome) = completion.outcome else {
            // A panic can happen after the atomic save but before the worker
            // returns. Retain the local marker so a delayed notification for
            // that possible write cannot roll newer edits back.
            if let Some(snapshot) = local_snapshot {
                let mut uncertain = self.uncertain_local_writes.borrow_mut();
                if uncertain.len() == MAX_UNCERTAIN_LOCAL_WRITES {
                    uncertain.pop_front();
                }
                uncertain.push_back(snapshot);
            }
            return Err("internal error while applying settings".to_owned());
        };
        *self.loopback.borrow_mut() = outcome.loopback;
        if outcome.was_persisted {
            if let Some(local) = &local_snapshot {
                let rebased = crate::config::storage::rebase_local(
                    &local.snapshot,
                    &self.settings.borrow(),
                    &outcome.snapshot,
                );
                *self.settings.borrow_mut() = rebased;
            }
            *self.last_persisted.borrow_mut() = Some(outcome.snapshot.clone());
        }
        match outcome.status {
            ApplyStatus::Applied => {
                *self.last_applied.borrow_mut() = Some(outcome.snapshot);
                Ok(())
            }
            ApplyStatus::Failed(message) => Err(message),
        }
    }
}

/// Run the full apply sequence (Tiers 0.5–4) on a worker thread. Every step is
/// a blocking subprocess (`pw-cli`, `systemctl`, `pw-loopback`) and follows the
/// documented tier order. `loopback_in` is the self-listen handle moved off the
/// main thread so it can be stopped/respawned here; the (possibly new) handle is
/// returned in the outcome.
fn run_apply(
    prev: Option<AppSettings>,
    baseline: AppSettings,
    mut snapshot: AppSettings,
    loopback_in: Option<Loopback>,
) -> ApplyOutcome {
    let transaction = crate::config::storage::SettingsLock::acquire().and_then(|guard| {
        let latest = AppSettings::load_strict()?;
        let merged = crate::config::storage::merge(&baseline, &snapshot, &latest)?;
        Ok((guard, merged))
    });
    let (_settings_lock, merged) = match transaction {
        Ok(transaction) => transaction,
        Err(error) => {
            return ApplyOutcome {
                snapshot,
                loopback: loopback_in,
                was_persisted: false,
                status: ApplyStatus::Failed(error.to_string()),
            };
        }
    };
    snapshot = merged;
    if prev.is_none() {
        crate::pipeline::purge_legacy_files();
    }

    // Tier 0.5 — put §38's choice into effect before anything is written.
    //
    // Only the command line did this, so a person who set the quality row in the window
    // kept whatever model was already there until their next login. It belongs here and
    // not on the main thread: reading the machine times the model, which takes seconds
    // the first time a plugin is seen.
    snapshot.settle_quality();
    crate::services::echo::settle(&mut snapshot.echo_cancel);

    // Tier 1 — persist settings.
    if let Err(e) = snapshot.save() {
        error!("state: failed to save settings: {e}");
        return ApplyOutcome {
            snapshot,
            loopback: loopback_in,
            was_persisted: false,
            status: ApplyStatus::Failed(format!("could not save settings: {e}")),
        };
    }

    if let Err(error) = crate::services::reconcile::apply(&snapshot, false) {
        return ApplyOutcome {
            snapshot,
            loopback: loopback_in,
            was_persisted: true,
            status: ApplyStatus::Failed(error.to_string()),
        };
    }
    let (loopback, status) = match reconcile_self_listen(prev.as_ref(), &snapshot, loopback_in) {
        Ok(loopback) => (loopback, ApplyStatus::Applied),
        Err(error) => (None, ApplyStatus::Failed(error)),
    };
    ApplyOutcome {
        snapshot,
        loopback,
        was_persisted: true,
        status,
    }
}

/// Spawn or kill the `pw-loopback` subprocess so the user can hear their own
/// microphone. Idempotent — only acts when the toggle actually changed, or when
/// the in-flight loopback died on its own and the user still wants it on. Takes
/// the current handle and returns the reconciled one (runs on the worker).
fn reconcile_self_listen(
    prev: Option<&AppSettings>,
    now: &AppSettings,
    mut loopback: Option<Loopback>,
) -> Result<Option<Loopback>, String> {
    let was_on = prev.is_some_and(|s| s.monitor.enabled);
    let is_on = now.monitor.enabled;

    if !is_on {
        if loopback.take().is_some() {
            debug!("state: stopped self-listen loopback");
        }
        return Ok(None);
    }

    // is_on: bring the loopback up if it isn't already alive.
    let alive = loopback.as_mut().is_some_and(Loopback::is_alive);
    if alive && was_on && prev.is_some_and(|p| p.monitor.delay_ms == now.monitor.delay_ms) {
        return Ok(loopback);
    }

    // Either fresh start, delay changed, or process died — recreate.
    drop(loopback.take());
    match Loopback::start(now.monitor.delay_ms) {
        Ok(handle) => {
            info!("state: self-listen loopback started");
            Ok(Some(handle))
        }
        Err(e) => {
            error!("state: self-listen loopback failed: {e}");
            Err(format!("Could not start self-listen: {e}"))
        }
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
