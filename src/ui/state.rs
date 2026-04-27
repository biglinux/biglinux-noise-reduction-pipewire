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
//! The output filter and its standalone `pipewire -c` unit follow the
//! same tier order: enable/disable toggles tier 4 on `biglinux-microphone-output.service`, which affects only the
//! `output-biglinux` virtual sink. WirePlumber is **never restarted**.

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
    apply_live, default_sink_name, reload_mic_chain, restart_echo_cancel_service,
    restart_output_service, start_output_service, stop_echo_cancel_service,
    stop_filter_chain_service,
};

/// Delay between the last edit and the apply phase. 150 ms merges slider
/// drags into a single pass without feeling sluggish.
const DEBOUNCE_MS: u32 = 150;

/// Shared GTK-main-thread state.
pub struct AppState {
    settings: RefCell<AppSettings>,
    debounce_timer: RefCell<Option<SourceId>>,
    dirty: Cell<bool>,
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

    /// Flush pending changes immediately (used on window close).
    pub fn flush(self: &Rc<Self>) {
        if let Some(id) = self.debounce_timer.borrow_mut().take() {
            id.remove();
        }
        self.apply_now();
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

    fn apply_now(&self) {
        if !self.dirty.replace(false) {
            return;
        }
        let prev = self.last_applied.borrow().clone();

        // Capture the current default sink as the output filter's
        // playback target *before* the conf is rendered. Done only on
        // the false→true transition: any later change reuses whatever
        // we captured the first time, so toggling the master off and
        // back on doesn't accidentally store `output-biglinux` (which
        // would loop).
        let was_enabled = prev.as_ref().is_some_and(|s| s.output_filter.enabled);
        let is_enabled = self.settings.borrow().output_filter.enabled;
        let needs_capture = is_enabled
            && !was_enabled
            && self
                .settings
                .borrow()
                .output_filter
                .target_sink_name
                .is_none();
        if needs_capture {
            if let Some(target) = capture_external_default_sink() {
                info!("state: captured playback target sink = {target}");
                self.settings.borrow_mut().output_filter.target_sink_name = Some(target);
            }
        }

        let snapshot = self.settings.borrow().clone();

        // Tier 1 — persist settings.
        if let Err(e) = snapshot.save() {
            error!("state: failed to save settings: {e}");
            self.dirty.set(true);
            return;
        }

        // Tier 2 — rewrite on-disk drop-ins so the next login reproduces
        // the current state.
        if let Err(e) = pipeline::apply(&snapshot) {
            error!("state: failed to write pipeline configs: {e}");
            self.dirty.set(true);
            return;
        }

        // Tier 3 — push live control values. No restart, no dropout.
        let outcome = match apply_live(&snapshot) {
            Ok(o) => o,
            Err(e) => {
                error!("state: live control update failed: {e}");
                return;
            }
        };

        // Tier 4 — bring EC up first so the mic chain reload can pin its
        // capture to a `echo-cancel-source` that already exists.
        reconcile_echo_cancel_service(prev.as_ref(), &snapshot);

        let need_mic_reload = needs_mic_reload(prev.as_ref(), &snapshot) || !outcome.mic_pushed;
        if need_mic_reload {
            if pipeline::mic_chain_wanted(&snapshot) {
                info!("state: mic topology changed — restarting filter-chain.service");
                if let Err(e) = reload_mic_chain() {
                    error!("state: failed to reload mic chain: {e}");
                }
            } else if let Err(e) = stop_filter_chain_service() {
                error!("state: failed to stop mic chain: {e}");
            }
        } else {
            debug!("state: mic controls pushed live, no reload");
        }

        reconcile_output_service(prev.as_ref(), &snapshot);
        self.reconcile_self_listen(prev.as_ref(), &snapshot);

        *self.last_applied.borrow_mut() = Some(snapshot);
    }

    /// Spawn or kill the `pw-loopback` subprocess so the user can hear
    /// their own microphone. Idempotent — only acts when the toggle
    /// actually changed, or when an in-flight loopback died on its own
    /// and the user still wants it on.
    fn reconcile_self_listen(&self, prev: Option<&AppSettings>, now: &AppSettings) {
        let was_on = prev.is_some_and(|s| s.monitor.enabled);
        let is_on = now.monitor.enabled;

        if !is_on {
            if let Some(handle) = self.loopback.borrow_mut().take() {
                handle.stop();
                debug!("state: stopped self-listen loopback");
            }
            return;
        }

        // is_on: bring the loopback up if it isn't already alive.
        let mut slot = self.loopback.borrow_mut();
        let alive = slot.as_mut().is_some_and(Loopback::is_alive);
        if alive && was_on && prev.is_some_and(|p| p.monitor.delay_ms == now.monitor.delay_ms) {
            return;
        }

        // Either fresh start, delay changed, or process died — recreate.
        slot.take();
        let opts = LoopbackOptions {
            delay_ms: now.monitor.delay_ms,
            ..LoopbackOptions::default()
        };
        match Loopback::start(&opts) {
            Ok(handle) => {
                *slot = Some(handle);
                info!("state: self-listen loopback started");
            }
            Err(e) => error!("state: self-listen loopback failed: {e}"),
        }
    }
}

/// Bring the standalone output unit up the first time the user enables
/// the master switch in this session, then promote `output-biglinux`
/// as the system default sink. Switching the master back off **does
/// not** stop the unit: doing so would tear down the virtual sink and
/// Chromium-based browsers pause playback the moment their target sink
/// disappears. The bypass values pushed by `apply_live` make the
/// still-running graph transparent instead.
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
    }
    // is_enabled && !output_topology_changed → live update already
    // pushed the new control values, no service action needed.
    // !is_enabled → leave the unit running in bypass; do NOT stop it.
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

/// Lifecycle for the standalone WebRTC echo-cancel `pipewire -c` unit.
/// Opt-in feature, so unlike the output unit we **do** stop it when the
/// user turns AEC off — leaving it running would keep an unused
/// echo-cancel-source node in the graph and confuse picker UIs.
fn reconcile_echo_cancel_service(prev: Option<&AppSettings>, now: &AppSettings) {
    let was_enabled = prev.is_some_and(|s| s.echo_cancel.enabled);
    let is_enabled = now.echo_cancel.enabled;

    if is_enabled && !was_enabled {
        if let Err(e) = restart_echo_cancel_service() {
            error!("state: echo-cancel service start failed: {e}");
        }
    } else if !is_enabled && was_enabled {
        if let Err(e) = stop_echo_cancel_service() {
            error!("state: echo-cancel service stop failed: {e}");
        }
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
        // `target.object = "echo-cancel-source"` is added on the capture
        // side only when AEC is on. Toggling AEC rewrites that prop, so
        // the chain must be reloaded before the graph can use/bypass the
        // cleaned source.
        let ec_target_changed = p.echo_cancel.enabled != now.echo_cancel.enabled;
        p.equalizer.bands != now.equalizer.bands
            || p.equalizer.preset != now.equalizer.preset
            || p.equalizer.enabled != now.equalizer.enabled
            || voice_changer_topology_changed
            || ai_topology_changed
            || ec_target_changed
    } else {
        now_wanted
    }
}

fn output_topology_changed(prev: Option<&AppSettings>, now: &AppSettings) -> bool {
    // GTCRN is now permanently wired into the output graph: NR / master
    // toggles flip its `Enable` port via the live update path instead of
    // restarting the unit. Only EQ band/preset edits actually rewrite
    // the on-disk graph today, so that's the only signal here.
    prev.is_some_and(|p| {
        p.output_filter.equalizer.bands != now.output_filter.equalizer.bands
            || p.output_filter.equalizer.preset != now.output_filter.equalizer.preset
            || p.output_filter.equalizer.enabled != now.output_filter.equalizer.enabled
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
