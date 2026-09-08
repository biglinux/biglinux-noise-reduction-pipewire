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
use crate::pipeline;
use crate::services::loopback::Loopback;
use crate::services::pipewire::{
    apply_live, restart_mic_service, restart_output_service, start_aec_service, start_mic_service,
    start_output_service, stop_aec_service, stop_mic_service, stop_output_service,
};

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
    snapshot: AppSettings,
    loopback: Option<Loopback>,
}

impl ApplyWork {
    pub(super) const fn request(&self) -> ApplyRequest {
        self.request
    }

    /// Run all blocking PipeWire/persistence work and return a typed result.
    pub(super) fn run(self) -> ApplyCompletion {
        let outcome = run_apply(self.previous, self.snapshot, self.loopback);
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

impl AppState {
    #[must_use]
    pub fn new(settings: AppSettings) -> Rc<Self> {
        Rc::new(Self {
            settings: RefCell::new(settings),
            last_applied: RefCell::new(None),
            last_persisted: RefCell::new(None),
            local_apply_snapshot: RefCell::new(None),
            uncertain_local_writes: RefCell::new(VecDeque::new()),
            loopback: RefCell::new(None),
        })
    }

    #[must_use]
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
        *self.settings.borrow_mut() = new;
        true
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
    mut snapshot: AppSettings,
    loopback_in: Option<Loopback>,
) -> ApplyOutcome {
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

    // Tier 2 — rewrite on-disk drop-ins so the next login reproduces the state.
    if let Err(e) = pipeline::apply(&snapshot) {
        error!("state: failed to write pipeline configs: {e}");
        return ApplyOutcome {
            snapshot,
            loopback: loopback_in,
            was_persisted: true,
            status: ApplyStatus::Failed(format!("could not write audio configuration: {e}")),
        };
    }

    // Tier 3 — push live control values. No restart, no dropout.
    let live = match apply_live(&snapshot) {
        Ok(o) => o,
        Err(e) => {
            error!("state: live control update failed: {e}");
            return ApplyOutcome {
                snapshot,
                loopback: loopback_in,
                was_persisted: true,
                status: ApplyStatus::Failed(format!("live control update failed: {e}")),
            };
        }
    };

    // Tier 4 — drive each pwloader unit independently. AEC first because the
    // mic chain pins `target.object = "echo-cancel-source"` when AEC is on, so
    // the EC source must already exist by the time the mic loader resolves its
    // capture target. Unit failures are collected instead of swallowed: the
    // snapshot must not be recorded as applied (and the user must be told)
    // when a required service did not actually reach its target state.
    let mut unit_errors: Vec<String> = Vec::new();
    if let Err(e) = reconcile_aec_service(prev.as_ref(), &snapshot) {
        error!("state: {e}");
        unit_errors.push(e);
    }

    let need_mic_reload = needs_mic_reload(prev.as_ref(), &snapshot) || !live.mic_pushed;
    if need_mic_reload {
        if pipeline::mic_chain_wanted(&snapshot) {
            let was_running = prev.as_ref().is_some_and(pipeline::mic_chain_wanted);
            if was_running {
                info!("state: mic args changed — restarting mic loader");
                if let Err(e) = restart_mic_service() {
                    error!("state: failed to reload mic loader: {e}");
                    unit_errors.push(format!("microphone filter reload failed: {e}"));
                }
            } else {
                info!("state: mic chain wanted — starting mic loader");
                if let Err(e) = start_mic_service() {
                    error!("state: failed to start mic loader: {e}");
                    unit_errors.push(format!("microphone filter start failed: {e}"));
                }
            }
        } else if let Err(e) = stop_mic_service() {
            error!("state: failed to stop mic loader: {e}");
            unit_errors.push(format!("microphone filter stop failed: {e}"));
        }
    } else {
        debug!("state: mic controls pushed live, no reload");
    }

    if let Err(e) = reconcile_output_service(prev.as_ref(), &snapshot) {
        error!("state: {e}");
        unit_errors.push(e);
    }
    let loopback = reconcile_self_listen(prev.as_ref(), &snapshot, loopback_in);

    let status = if unit_errors.is_empty() {
        ApplyStatus::Applied
    } else {
        ApplyStatus::Failed(unit_errors.join("; "))
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
) -> Option<Loopback> {
    let was_on = prev.is_some_and(|s| s.monitor.enabled);
    let is_on = now.monitor.enabled;

    if !is_on {
        if loopback.take().is_some() {
            debug!("state: stopped self-listen loopback");
        }
        return None;
    }

    // is_on: bring the loopback up if it isn't already alive.
    let alive = loopback.as_mut().is_some_and(Loopback::is_alive);
    if alive && was_on && prev.is_some_and(|p| p.monitor.delay_ms == now.monitor.delay_ms) {
        return loopback;
    }

    // Either fresh start, delay changed, or process died — recreate.
    drop(loopback.take());
    match Loopback::start(now.monitor.delay_ms) {
        Ok(handle) => {
            info!("state: self-listen loopback started");
            Some(handle)
        }
        Err(e) => {
            error!("state: self-listen loopback failed: {e}");
            None
        }
    }
}

/// Drive the standalone output unit so its `pipewire -c` worker only
/// exists while the user actually wants the output filter. Disabling
/// the master tears the virtual sink down — Chromium-based browsers
/// pause playback when their target sink disappears, but that is the
/// explicit price the user pays for not having an idle pipewire worker
/// hanging around. Re-enabling spawns the worker again and `apply_live`
/// repopulates the controls.
///
/// Topology changes (EQ band layout) can only take effect via a real
/// restart, and we only attempt that when the master is on — i.e. the
/// user is actively listening through the filter and a brief reload is
/// expected.
fn reconcile_output_service(prev: Option<&AppSettings>, now: &AppSettings) -> Result<(), String> {
    let was_enabled = prev.is_some_and(|s| s.output_filter.enabled);
    let is_enabled = now.output_filter.enabled;

    if is_enabled && !was_enabled {
        start_output_service().map_err(|e| format!("output filter start failed: {e}"))
    } else if is_enabled && output_topology_changed(prev, now) {
        restart_output_service().map_err(|e| format!("output filter restart failed: {e}"))
    } else if !is_enabled && was_enabled {
        stop_output_service().map_err(|e| format!("output filter stop failed: {e}"))
    } else {
        // is_enabled && !topology_changed → live update covered it.
        // !is_enabled && !was_enabled → nothing to do.
        Ok(())
    }
}

/// Drive `biglinux-microphone-aec.service`. The EC source is the
/// upstream of the mic chain when enabled, so we start/restart it
/// before the caller touches the mic loader. Topology of the AEC
/// args body is fixed (only the `enabled` flag toggles its existence),
/// so any rewrite is purely "exists or not" — no in-process restart
/// needed when the toggle stays true.
///
/// The "not wanted" branch always issues a `stop`, even when the
/// previous snapshot also had AEC off. `systemctl stop` on an already
/// inactive unit is a cheap no-op; the redundancy is what guarantees
/// we collect any orphaned AEC pwloader that an out-of-process actor or
/// stale mic-unit dependency left running. Without it, that AEC loader
/// would keep consuming CPU even after the user disabled the toggle.
fn reconcile_aec_service(prev: Option<&AppSettings>, now: &AppSettings) -> Result<(), String> {
    let was_on = prev.is_some_and(|settings| settings.echo_cancel.enabled);
    let is_on = now.echo_cancel.enabled;
    if is_on {
        if !was_on {
            start_aec_service().map_err(|e| format!("echo-cancellation start failed: {e}"))?;
        }
        Ok(())
    } else {
        stop_aec_service().map_err(|e| format!("echo-cancellation stop failed: {e}"))
    }
}

fn needs_mic_reload(prev: Option<&AppSettings>, now: &AppSettings) -> bool {
    let was_wanted = prev.is_some_and(pipeline::mic_chain_wanted);
    let now_wanted = pipeline::mic_chain_wanted(now);
    if was_wanted != now_wanted {
        return true;
    }
    if let Some(p) = prev {
        let voice_was_on =
            p.stereo.enabled && p.stereo.mode == crate::config::StereoMode::VoiceChanger;
        let voice_is_on =
            now.stereo.enabled && now.stereo.mode == crate::config::StereoMode::VoiceChanger;
        let voice_changer_topology_changed = voice_was_on != voice_is_on
            || (voice_is_on && (p.stereo.width - now.stereo.width).abs() > f32::EPSILON);
        let ai_topology_changed =
            pipeline::ai_node_in_mic_chain(p) != pipeline::ai_node_in_mic_chain(now);
        // Selecting a different denoiser backend swaps the LADSPA
        // plugin — different .so, different control
        // surface, different port names. Only a reload picks that up.
        // While an attenuation-only backend is active a separate SWH gate node also rides
        // alongside `ai`, so toggling the gate flag has to reload too
        // (instead of being a pure live update like with GTCRN's
        // integrated gate).
        let denoiser_topology_changed = p.noise_reduction.model != now.noise_reduction.model
            || (now.noise_reduction.model.is_attenuation_only()
                && p.gate.enabled != now.gate.enabled);
        // `target.object = "echo-cancel-source"` is added on the capture
        // side only when AEC is on. Toggling AEC rewrites that prop, so
        // the chain must be reloaded before the graph can use/bypass the
        // cleaned source.
        let ec_target_changed = p.echo_cancel.enabled != now.echo_cancel.enabled;
        // HPF is a 2-biquad cascade when enabled and a single
        // pass-through node when disabled — toggling it adds/removes
        // `hpf_pre` from the graph, so we must reload, not live-update.
        let hpf_topology_changed = p.hpf.enabled != now.hpf.enabled;
        p.equalizer.bands != now.equalizer.bands
            || p.equalizer.preset != now.equalizer.preset
            || p.equalizer.enabled != now.equalizer.enabled
            || p.compressor.enabled != now.compressor.enabled
            || voice_changer_topology_changed
            || ai_topology_changed
            || denoiser_topology_changed
            || ec_target_changed
            || hpf_topology_changed
    } else {
        now_wanted
    }
}

fn output_topology_changed(prev: Option<&AppSettings>, now: &AppSettings) -> bool {
    // GTCRN is permanently wired in the output graph; NR / master
    // toggles flip its `Enable` port via the live update path. EQ
    // band/preset changes rewrite the graph, and so does selecting a
    // different denoiser backend (GTCRN vs attenuation-only) since the LADSPA
    // plugin and port names differ.
    prev.is_some_and(|p| {
        p.output_filter.equalizer.bands != now.output_filter.equalizer.bands
            || p.output_filter.equalizer.preset != now.output_filter.equalizer.preset
            || p.output_filter.equalizer.enabled != now.output_filter.equalizer.enabled
            || p.output_filter.noise_reduction.model != now.output_filter.noise_reduction.model
    })
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
