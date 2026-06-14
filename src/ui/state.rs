//! Shared UI state + debounced "apply" pipeline.
//!
//! GTK callbacks run on a single thread, so the widgets cooperate on a
//! plain [`Rc<RefCell<...>>`] rather than an `Arc<Mutex>`. Three
//! concerns live side by side:
//!
//! 1. The typed [`AppSettings`] snapshot the user is editing.
//! 2. A debouncer that batches rapid setting changes.
//! 3. A remembered copy of the last-applied settings so we can detect
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

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use glib::SourceId;
use log::{debug, error, info};

use crate::config::AppSettings;
use crate::pipeline;
use crate::pipeline::OUTPUT_NODE_NAME;
use crate::services::loopback::{Loopback, LoopbackOptions};
use crate::services::pipewire::{
    apply_live, default_sink_name, reload_mic_chain, restart_output_service, start_aec_service,
    start_mic_service, start_output_service, stop_aec_service, stop_mic_service,
    stop_output_service,
};

/// What the main thread should do once an offloaded apply pass completes.
enum ApplyStatus {
    /// Full Tier 1–4 sequence ran — record the snapshot as last-applied.
    Applied,
    /// A Tier 1/2 disk write failed — leave `last_applied` and re-arm so the
    /// pass retries (mirrors the old synchronous `dirty.set(true)`).
    Retry,
    /// Tier 3 (live control push) failed — Tier 4 was skipped and the snapshot
    /// is *not* recorded, but we don't retry (mirrors the old early `return`).
    Halted,
}

/// Result of an offloaded apply pass, carried back to the main thread.
struct ApplyOutcome {
    /// The processed snapshot (with any captured playback-target sink).
    snapshot: AppSettings,
    /// Self-listen loopback handle after reconciliation — possibly the same
    /// one passed in, a freshly spawned one, or `None`.
    loopback: Option<Loopback>,
    status: ApplyStatus,
}

/// Delay between the last edit and the apply phase. 150 ms merges slider
/// drags into a single pass without feeling sluggish.
const DEBOUNCE_MS: u32 = 150;

/// Shared GTK-main-thread state.
pub struct AppState {
    settings: RefCell<AppSettings>,
    debounce_timer: RefCell<Option<SourceId>>,
    dirty: Cell<bool>,
    /// True while an apply pass is running on a worker thread. Serialises
    /// applies: a debounce that fires mid-pass just leaves `dirty` set, and the
    /// completing pass re-arms so the latest edit lands without two passes
    /// racing `systemctl` against each other.
    applying: Cell<bool>,
    /// Last snapshot that was successfully applied. Used to decide
    /// whether the current change needs an expensive mic-chain reload
    /// or if the cheap live path is sufficient.
    last_applied: RefCell<Option<AppSettings>>,
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
            debounce_timer: RefCell::new(None),
            dirty: Cell::new(false),
            applying: Cell::new(false),
            last_applied: RefCell::new(None),
            loopback: RefCell::new(None),
        })
    }

    #[must_use]
    pub fn settings(&self) -> std::cell::Ref<'_, AppSettings> {
        self.settings.borrow()
    }

    pub fn mutate<F: FnOnce(&mut AppSettings)>(self: &Rc<Self>, f: F) {
        {
            let mut guard = self.settings.borrow_mut();
            f(&mut guard);
        }
        self.dirty.set(true);
        self.arm_debounce();
    }

    /// Replace the in-memory settings with a snapshot that was applied
    /// **outside** this process (the CLI / plasmoid wrote settings.json
    /// and ran the apply pipeline themselves). Drops any pending
    /// debounce, marks the new value as already-applied, and returns
    /// `true` when an actual change was absorbed so the caller can
    /// rebuild the widgets that mirror these fields.
    pub fn external_replace(self: &Rc<Self>, new: AppSettings) -> bool {
        if *self.settings.borrow() == new {
            return false;
        }
        if let Some(id) = self.debounce_timer.borrow_mut().take() {
            id.remove();
        }
        *self.settings.borrow_mut() = new.clone();
        *self.last_applied.borrow_mut() = Some(new);
        self.dirty.set(false);
        true
    }

    /// Flush pending changes immediately (used on window close). Runs the apply
    /// pass **synchronously** — the process is about to exit, so an offloaded
    /// pass might never complete; a brief block here is the right trade. (The
    /// debounced mid-session path, by contrast, runs off the main loop.)
    pub fn flush(self: &Rc<Self>) {
        if let Some(id) = self.debounce_timer.borrow_mut().take() {
            id.remove();
        }
        if !self.dirty.replace(false) {
            return;
        }
        let prev = self.last_applied.borrow().clone();
        let snapshot = self.settings.borrow().clone();
        let was_enabled = prev.as_ref().is_some_and(|s| s.output_filter.enabled);
        let needs_capture = snapshot.output_filter.enabled
            && !was_enabled
            && snapshot.output_filter.target_sink_name.is_none();
        if self.applying.get() {
            // A worker apply is still in flight and will finish on its thread;
            // starting a second pass here would race it against `systemctl`.
            // Just guarantee the latest edit is persisted so the next login
            // reproduces it, and let the in-flight pass settle the services.
            if let Err(e) = snapshot.save() {
                error!("state: failed to save settings on close: {e}");
            }
            return;
        }
        let loopback_in = self.loopback.borrow_mut().take();
        let outcome = run_apply(prev, snapshot, needs_capture, loopback_in);
        self.reconcile_after_apply(outcome);
    }

    fn arm_debounce(self: &Rc<Self>) {
        if let Some(id) = self.debounce_timer.borrow_mut().take() {
            id.remove();
        }
        let me_weak = Rc::downgrade(self);
        let id =
            glib::timeout_add_local(Duration::from_millis(u64::from(DEBOUNCE_MS)), move || {
                if let Some(me) = me_weak.upgrade() {
                    let _ = me.debounce_timer.borrow_mut().take();
                    me.apply_now();
                }
                glib::ControlFlow::Break
            });
        *self.debounce_timer.borrow_mut() = Some(id);
    }

    /// Kick off an apply pass. The expensive part — capturing the default sink,
    /// pushing live controls, and restarting the pwloader units (all `systemctl`
    /// / `pw-cli` subprocesses) — runs on a worker thread so dragging a slider
    /// never freezes the UI. Only the cheap decision-gathering happens here.
    fn apply_now(self: &Rc<Self>) {
        if !self.dirty.get() {
            return;
        }
        if self.applying.get() {
            // A pass is already running on a worker; it re-arms on completion
            // (`dirty` stays set), so we never race two passes' `systemctl`.
            return;
        }
        self.dirty.set(false);

        let prev = self.last_applied.borrow().clone();
        let snapshot = self.settings.borrow().clone();
        // Capture the default sink as the output filter's playback target
        // *before* the conf is rendered — only on the false→true master
        // transition, so toggling off/on doesn't overwrite it with
        // `output-biglinux` (which would loop). The capture itself runs on the
        // worker (it's a `pw-cli` call), keyed off this flag.
        let was_enabled = prev.as_ref().is_some_and(|s| s.output_filter.enabled);
        let needs_capture = snapshot.output_filter.enabled
            && !was_enabled
            && snapshot.output_filter.target_sink_name.is_none();

        let loopback_in = self.loopback.borrow_mut().take();
        self.applying.set(true);

        let me_weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let outcome =
                gio::spawn_blocking(move || run_apply(prev, snapshot, needs_capture, loopback_in))
                    .await
                    .unwrap_or_else(|_| ApplyOutcome {
                        // Worker panicked — the moved-in loopback handle is gone
                        // and the pass is abandoned (Halted skips last_applied).
                        snapshot: AppSettings::default(),
                        loopback: None,
                        status: ApplyStatus::Halted,
                    });
            if let Some(me) = me_weak.upgrade() {
                me.finish_apply(outcome);
            }
        });
    }

    /// Main-thread continuation after an offloaded apply pass. Stores the new
    /// loopback handle, records / retries per the pass status, and re-arms the
    /// debounce if edits arrived while the worker ran.
    fn finish_apply(self: &Rc<Self>, outcome: ApplyOutcome) {
        self.applying.set(false);
        self.reconcile_after_apply(outcome);
        if self.dirty.get() {
            self.arm_debounce();
        }
    }

    /// Apply the worker's outcome to shared state. Shared by the async
    /// (`finish_apply`) and synchronous-close (`flush`) paths.
    fn reconcile_after_apply(&self, outcome: ApplyOutcome) {
        *self.loopback.borrow_mut() = outcome.loopback;
        match outcome.status {
            ApplyStatus::Applied => {
                // The worker may have captured the playback target sink; mirror
                // it into the live settings so the next needs_capture check sees
                // it (the main-thread settings predate the worker's capture).
                if let Some(target) = outcome.snapshot.output_filter.target_sink_name.clone() {
                    let mut settings = self.settings.borrow_mut();
                    if settings.output_filter.target_sink_name.is_none() {
                        settings.output_filter.target_sink_name = Some(target);
                    }
                }
                *self.last_applied.borrow_mut() = Some(outcome.snapshot);
            }
            ApplyStatus::Retry => self.dirty.set(true),
            ApplyStatus::Halted => {}
        }
    }
}

/// Run the full apply sequence (Tiers 0.5–4) on a worker thread. Every step is
/// a blocking subprocess (`pw-cli`, `systemctl`, `pw-loopback`); the ordering
/// is identical to the old synchronous `apply_now`. `loopback_in` is the
/// self-listen handle moved off the main thread so it can be stopped/respawned
/// here; the (possibly new) handle is returned in the outcome.
fn run_apply(
    prev: Option<AppSettings>,
    mut snapshot: AppSettings,
    needs_capture: bool,
    loopback_in: Option<Loopback>,
) -> ApplyOutcome {
    if needs_capture {
        if let Some(target) = capture_external_default_sink() {
            info!("state: captured playback target sink = {target}");
            snapshot.output_filter.target_sink_name = Some(target);
        }
    }

    // Tier 1 — persist settings.
    if let Err(e) = snapshot.save() {
        error!("state: failed to save settings: {e}");
        return ApplyOutcome {
            snapshot,
            loopback: loopback_in,
            status: ApplyStatus::Retry,
        };
    }

    // Tier 2 — rewrite on-disk drop-ins so the next login reproduces the state.
    if let Err(e) = pipeline::apply(&snapshot) {
        error!("state: failed to write pipeline configs: {e}");
        return ApplyOutcome {
            snapshot,
            loopback: loopback_in,
            status: ApplyStatus::Retry,
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
                status: ApplyStatus::Halted,
            };
        }
    };

    // Tier 4 — drive each pwloader unit independently. AEC first because the
    // mic chain pins `target.object = "echo-cancel-source"` when AEC is on, so
    // the EC source must already exist by the time the mic loader resolves its
    // capture target.
    reconcile_aec_service(prev.as_ref(), &snapshot);

    let need_mic_reload = needs_mic_reload(prev.as_ref(), &snapshot) || !live.mic_pushed;
    if need_mic_reload {
        if pipeline::mic_chain_wanted(&snapshot) {
            let was_running = prev.as_ref().is_some_and(pipeline::mic_chain_wanted);
            if was_running {
                info!("state: mic args changed — restarting mic loader");
                if let Err(e) = reload_mic_chain() {
                    error!("state: failed to reload mic loader: {e}");
                }
            } else {
                info!("state: mic chain wanted — starting mic loader");
                if let Err(e) = start_mic_service() {
                    error!("state: failed to start mic loader: {e}");
                }
            }
        } else if let Err(e) = stop_mic_service() {
            error!("state: failed to stop mic loader: {e}");
        }
    } else {
        debug!("state: mic controls pushed live, no reload");
    }

    reconcile_output_service(prev.as_ref(), &snapshot);
    let loopback = reconcile_self_listen(prev.as_ref(), &snapshot, loopback_in);

    ApplyOutcome {
        snapshot,
        loopback,
        status: ApplyStatus::Applied,
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
        if let Some(handle) = loopback.take() {
            handle.stop();
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
    let opts = LoopbackOptions {
        delay_ms: now.monitor.delay_ms,
        ..LoopbackOptions::default()
    };
    match Loopback::start(&opts) {
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
fn reconcile_output_service(prev: Option<&AppSettings>, now: &AppSettings) {
    let was_enabled = prev.is_some_and(|s| s.output_filter.enabled);
    let is_enabled = now.output_filter.enabled;

    if is_enabled && !was_enabled {
        if let Err(e) = start_output_service() {
            error!("state: output service start failed: {e}");
        }
    } else if is_enabled && output_topology_changed(prev, now) {
        if let Err(e) = restart_output_service() {
            error!("state: output service restart failed: {e}");
        }
    } else if !is_enabled && was_enabled {
        if let Err(e) = stop_output_service() {
            error!("state: output service stop failed: {e}");
        }
    }
    // is_enabled && !topology_changed → live update covered it.
    // !is_enabled && !was_enabled → nothing to do.
}

/// Read the current default sink and return its `node.name` *only* when
/// it's an external sink (anything other than our own `output-biglinux`
/// or any of the AEC virtual nodes). Used to capture the user's chosen
/// hardware sink so the smart-filter can pin to it unambiguously.
fn capture_external_default_sink() -> Option<String> {
    let name = default_sink_name()?;
    if name == OUTPUT_NODE_NAME {
        return None;
    }
    if name.starts_with("echo-cancel") {
        return None;
    }
    Some(name)
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
/// we collect any orphaned AEC pwloader that an older build (or an
/// out-of-process actor) left running. Without it, an AEC loader
/// inadvertently started via the mic unit's old `Wants=` chain would
/// keep consuming CPU even after the user disabled the toggle.
fn reconcile_aec_service(prev: Option<&AppSettings>, now: &AppSettings) {
    let was_on = prev.is_some_and(pipeline::echo_cancel_wanted);
    let is_on = pipeline::echo_cancel_wanted(now);
    if is_on {
        if !was_on {
            if let Err(e) = start_aec_service() {
                error!("state: AEC service start failed: {e}");
            }
        }
    } else if let Err(e) = stop_aec_service() {
        error!("state: AEC service stop failed: {e}");
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
        // plugin (GTCRN ↔ DFN3) — different .so, different control
        // surface, different port names. Only a reload picks that up.
        // While DFN3 is active a separate SWH gate node also rides
        // alongside `ai`, so toggling the gate flag has to reload too
        // (instead of being a pure live update like with GTCRN's
        // integrated gate).
        let denoiser_topology_changed = p.noise_reduction.model != now.noise_reduction.model
            || (now.noise_reduction.model.is_deepfilter() && p.gate.enabled != now.gate.enabled);
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
    // different denoiser backend (GTCRN ↔ DFN3) since the LADSPA
    // plugin and port names differ.
    prev.is_some_and(|p| {
        p.output_filter.equalizer.bands != now.output_filter.equalizer.bands
            || p.output_filter.equalizer.preset != now.output_filter.equalizer.preset
            || p.output_filter.equalizer.enabled != now.output_filter.equalizer.enabled
            || p.output_filter.noise_reduction.model != now.output_filter.noise_reduction.model
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        CompressorConfig, EqualizerConfig, GateConfig, HpfConfig, NoiseReductionConfig,
        OutputFilterSettings, StereoConfig,
    };

    fn all_off() -> AppSettings {
        AppSettings {
            noise_reduction: NoiseReductionConfig {
                enabled: false,
                ..NoiseReductionConfig::default()
            },
            gate: GateConfig {
                enabled: false,
                ..GateConfig::default()
            },
            hpf: HpfConfig {
                enabled: false,
                ..HpfConfig::default()
            },
            stereo: StereoConfig {
                enabled: false,
                ..StereoConfig::default()
            },
            equalizer: EqualizerConfig {
                enabled: false,
                ..EqualizerConfig::default()
            },
            compressor: CompressorConfig {
                enabled: false,
                ..CompressorConfig::default()
            },
            ..AppSettings::default()
        }
    }

    fn with_nr_enabled(enabled: bool) -> AppSettings {
        AppSettings {
            noise_reduction: NoiseReductionConfig {
                enabled,
                ..NoiseReductionConfig::default()
            },
            ..all_off()
        }
    }

    #[test]
    fn no_reload_when_only_control_values_change() {
        let prev = with_nr_enabled(true);
        let mut next = prev.clone();
        next.noise_reduction.strength = 0.5;
        assert!(!needs_mic_reload(Some(&prev), &next));
    }

    #[test]
    fn reload_when_chain_goes_from_unwanted_to_wanted() {
        let prev = with_nr_enabled(false);
        let next = with_nr_enabled(true);
        assert!(needs_mic_reload(Some(&prev), &next));
    }

    #[test]
    fn reload_when_eq_bands_change() {
        let prev = with_nr_enabled(true);
        let mut next = prev.clone();
        next.equalizer = EqualizerConfig {
            enabled: true,
            bands: vec![3.0; 10],
            ..EqualizerConfig::default()
        };
        assert!(needs_mic_reload(Some(&prev), &next));
    }

    #[test]
    fn reload_on_first_apply_when_chain_wanted() {
        let next = with_nr_enabled(true);
        assert!(needs_mic_reload(None, &next));
    }

    #[test]
    fn output_topology_unchanged_when_only_enable_flag_toggles() {
        let prev = AppSettings {
            output_filter: OutputFilterSettings {
                enabled: true,
                ..OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        let mut next = prev.clone();
        next.output_filter.noise_reduction.strength = 0.4;
        assert!(!output_topology_changed(Some(&prev), &next));
    }

    #[test]
    fn reload_when_mic_compressor_node_added_or_removed() {
        // Compressor is now a topology-conditional node — the SC4
        // LADSPA gets dropped from the graph entirely when off, so
        // toggling the flag must reload the chain instead of going
        // through the live-controls path.
        let prev = with_nr_enabled(true);
        let mut next = prev.clone();
        next.compressor.enabled = true;
        assert!(needs_mic_reload(Some(&prev), &next));
    }

    #[test]
    fn reload_when_mic_ai_node_added_or_removed() {
        // NR + gate both off → AI node skipped. Turning the gate on
        // brings the node back into the graph: that's a topology
        // change, the live-controls fast path can't satisfy it.
        let prev = AppSettings {
            gate: GateConfig {
                enabled: false,
                ..GateConfig::default()
            },
            hpf: HpfConfig {
                enabled: true,
                ..HpfConfig::default()
            },
            noise_reduction: NoiseReductionConfig {
                enabled: false,
                ..NoiseReductionConfig::default()
            },
            ..all_off()
        };
        let mut next = prev.clone();
        next.gate.enabled = true;
        assert!(needs_mic_reload(Some(&prev), &next));
    }

    #[test]
    fn reload_when_mic_hpf_toggles() {
        // Enabling HPF inserts a second `bq_highpass` (`hpf_pre`)
        // ahead of `hpf` to form a 4th-order Linkwitz-Riley cascade —
        // can't be done by live-updating control values alone.
        let prev = AppSettings {
            hpf: HpfConfig {
                enabled: false,
                ..HpfConfig::default()
            },
            ..AppSettings::default()
        };
        let mut next = prev.clone();
        next.hpf.enabled = true;
        assert!(needs_mic_reload(Some(&prev), &next));
    }

    #[test]
    fn output_topology_unchanged_when_only_nr_enabled_toggles() {
        // GTCRN stays in the output graph regardless of NR — toggling
        // its Enable port is a live update, not a restart trigger.
        let prev = AppSettings {
            output_filter: OutputFilterSettings {
                enabled: true,
                noise_reduction: NoiseReductionConfig {
                    enabled: false,
                    ..NoiseReductionConfig::default()
                },
                ..OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        let mut next = prev.clone();
        next.output_filter.noise_reduction.enabled = true;
        assert!(!output_topology_changed(Some(&prev), &next));
    }

    #[test]
    fn output_topology_changed_when_eq_bands_change() {
        let prev = AppSettings {
            output_filter: OutputFilterSettings {
                enabled: true,
                ..OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        let mut next = prev.clone();
        next.output_filter.equalizer = EqualizerConfig {
            enabled: true,
            bands: vec![4.0; 10],
            ..EqualizerConfig::default()
        };
        assert!(output_topology_changed(Some(&prev), &next));
    }
}
