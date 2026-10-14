//! Reconcile saved intent, live controls and independently recoverable services.
//!
//! Call on a worker while holding SettingsLock. Unknown graph state is not
//! absence. A failed controls-only write must not destroy an existing endpoint;
//! leave the desired configuration pending and report the failure instead.
use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::pipewire::{self, LiveOutcome, UpdateTargets};
use crate::config::AppSettings;
use crate::pipeline;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Observed {
    pub mic_present: bool,
    pub output_present: bool,
    pub aec_present: bool,
}

impl Observed {
    fn wanted(settings: &AppSettings) -> Self {
        Self {
            mic_present: pipeline::mic_chain_wanted(settings),
            output_present: settings.output_filter.enabled,
            aec_present: settings.echo_cancel.enabled,
        }
    }
}

pub fn observe() -> io::Result<Observed> {
    pipewire::graph_snapshot().map(|graph| from_graph(&graph))
}

/// Configuration and observed endpoints are separate: an idle source is ready
/// for a client, whereas a saved switch alone says nothing about the graph.
pub fn status(settings: &AppSettings) -> Value {
    let graph = pipewire::graph_snapshot();
    status_from_graph(settings, graph.as_deref().ok())
}

fn status_from_graph(settings: &AppSettings, graph: Option<&[Value]>) -> Value {
    let observed = graph.map(from_graph).unwrap_or_default();
    let mut sources = graph.unwrap_or_default().iter().filter(|object| {
        object["type"] == "PipeWire:Interface:Node"
            && object["info"]["props"]["node.name"] == pipeline::MIC_NODE_NAME
    });
    let source = sources.next();
    let source = if sources.next().is_none() {
        source
    } else {
        None
    };
    let props = source.map(|node| &node["info"]["props"]);
    let model = props
        .and_then(|props| {
            props["biglinux.model"]
                .as_u64()
                .or_else(|| props["biglinux.model"].as_str()?.parse().ok())
        })
        .filter(|model| matches!(model, 11..=13));
    let denoising = props.and_then(|props| {
        props["biglinux.denoising"]
            .as_bool()
            .or_else(|| props["biglinux.denoising"].as_str()?.parse::<bool>().ok())
    });
    serde_json::json!({
        "settings": settings,
        "mic_enabled": pipeline::mic_chain_wanted(&settings.runtime_settings()),
        "output_enabled": settings.output_filter.enabled,
        "audio_available": graph.is_some(),
        "mic_running": observed.mic_present,
        "output_running": observed.output_present,
        "aec_running": observed.aec_present,
        "mic_state": source.and_then(|node| node["info"]["state"].as_str()),
        "mic_serial": props.and_then(|props| props["object.serial"].as_u64()
            .map(|serial| serial.to_string())
            .or_else(|| props["object.serial"].as_str().map(str::to_owned))),
        "effective_model": if observed.mic_present && denoising == Some(true) { model } else { None },
        "denoising": if observed.mic_present { denoising } else { None },
    })
}

fn from_graph(graph: &[Value]) -> Observed {
    let nodes: HashSet<&str> = graph
        .iter()
        .filter(|object| object["type"] == "PipeWire:Interface:Node")
        .filter(|object| {
            matches!(
                object["info"]["state"].as_str(),
                Some("running" | "idle" | "suspended")
            )
        })
        .filter_map(|object| object["info"]["props"]["node.name"].as_str())
        .collect();
    Observed {
        mic_present: nodes.contains(pipeline::MIC_NODE_NAME)
            && nodes.contains(pipeline::MIC_CAPTURE_NODE_NAME),
        output_present: nodes.contains(pipeline::OUTPUT_NODE_NAME),
        aec_present: nodes.contains(pipeline::EC_SOURCE_NAME),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Keep,
    Restart,
    Stop,
}

fn action(
    wanted: bool,
    present: bool,
    was_wanted: bool,
    changed: bool,
    force: bool,
    pushed: bool,
) -> Action {
    if !wanted {
        if force || present || was_wanted {
            Action::Stop
        } else {
            Action::Keep
        }
    } else if force || changed || !present || !pushed {
        Action::Restart
    } else {
        Action::Keep
    }
}

#[derive(Debug, Clone, Copy)]
struct Changes {
    microphone: bool,
    output: bool,
    mic_topology: bool,
    output_topology: bool,
    force: bool,
}

impl Changes {
    fn between(previous: Option<&AppSettings>, settings: &AppSettings, force: bool) -> Self {
        Self {
            // These are graph projections, not window/UI preferences.
            microphone: previous.is_none_or(|old| {
                pipeline::build_mic_conf_for(old) != pipeline::build_mic_conf_for(settings)
            }),
            output: previous.is_none_or(|old| {
                pipeline::build_output_conf_for(old) != pipeline::build_output_conf_for(settings)
            }),
            mic_topology: needs_mic_reload(previous, settings),
            output_topology: output_topology_changed(previous, settings),
            force: force || previous.is_some_and(|old| old.runtime != settings.runtime),
        }
    }

    fn live_targets(self, settings: &AppSettings) -> UpdateTargets {
        UpdateTargets {
            microphone: !self.force
                && self.microphone
                && !self.mic_topology
                && pipeline::mic_chain_wanted(settings),
            output: !self.force
                && self.output
                && !self.output_topology
                && settings.output_filter.enabled,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Plan {
    aec: Action,
    mic: Action,
    output: Action,
}

impl Plan {
    fn new(
        settings: &AppSettings,
        previous: Option<&AppSettings>,
        observed: Observed,
        changes: Changes,
        live: LiveOutcome,
    ) -> Self {
        let aec = action(
            settings.echo_cancel.enabled,
            observed.aec_present,
            previous.is_some_and(|s| s.echo_cancel.enabled),
            previous.is_some_and(|old| {
                old.echo_cancel.gtcrn_async_delay != settings.echo_cancel.gtcrn_async_delay
                    || old.echo_cancel.gtcrn_track_delay != settings.echo_cancel.gtcrn_track_delay
                    || old.echo_cancel.gtcrn_hb_guard != settings.echo_cancel.gtcrn_hb_guard
                    || old.echo_cancel.gtcrn_reference_bypass
                        != settings.echo_cancel.gtcrn_reference_bypass
            }),
            changes.force,
            true,
        );
        Self {
            aec,
            mic: action(
                pipeline::mic_chain_wanted(settings),
                observed.mic_present,
                previous.is_some_and(pipeline::mic_chain_wanted),
                changes.mic_topology && changes.microphone,
                changes.force || aec != Action::Keep,
                live.mic_pushed || !changes.microphone,
            ),
            output: action(
                settings.output_filter.enabled,
                observed.output_present,
                previous.is_some_and(|s| s.output_filter.enabled),
                changes.output_topology && changes.output,
                changes.force,
                live.output_pushed || !changes.output,
            ),
        }
    }

    fn changed(self) -> bool {
        [self.aec, self.mic, self.output]
            .iter()
            .any(|step| *step != Action::Keep)
    }
}

fn baseline_path() -> PathBuf {
    dirs::runtime_dir()
        .map_or_else(crate::config::config_dir, |path| {
            path.join("biglinux-microphone")
        })
        .join("last-applied.json")
}

// A genuinely absent node can be repaired; a live/suspended node must not be
// replaced merely because a parameter write/read-back failed. Explicit repair
// has no live targets, so remains available through the normal forced path.
fn live_update_pending(targets: UpdateTargets, observed: Observed, live: LiveOutcome) -> bool {
    (targets.microphone && observed.mic_present && !live.mic_pushed)
        || (targets.output && observed.output_present && !live.output_pushed)
}

pub fn apply(settings: &AppSettings, force: bool) -> io::Result<()> {
    let settings = settings.runtime_settings();
    pipeline::apply(&settings)?;
    let previous: Option<AppSettings> = std::fs::read(baseline_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let changes = Changes::between(previous.as_ref(), &settings, force);
    // A failed observation cannot justify stopping a graph that may still be
    // carrying a call. Desired args are persisted, but not marked applied.
    let graph = pipewire::graph_snapshot()?;
    let observed = from_graph(&graph);
    let targets = changes.live_targets(&settings);
    let report = pipewire::apply_live_to_graph(&settings, &graph, targets);
    for failure in &report.failures {
        log::warn!("{failure}; live update not confirmed");
    }
    if live_update_pending(targets, observed, report.pushed) {
        let detail = if report.failures.is_empty() {
            "the existing node is not ready to accept controls".to_owned()
        } else {
            report.failures.join("; ")
        };
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("Audio settings pending; capture was not restarted: {detail}"),
        ));
    }
    let plan = Plan::new(
        &settings,
        previous.as_ref(),
        observed,
        changes,
        report.pushed,
    );
    let result = execute_plan(plan, Observed::wanted(&settings), &mut NativeBackend);
    if let Err(error) = result {
        let mut causes = report.failures;
        causes.push(error.to_string());
        return Err(io::Error::other(causes.join("; ")));
    }
    reconcile_feature_daemons(&settings);
    let bytes = serde_json::to_vec(&settings).map_err(io::Error::other)?;
    let path = baseline_path();
    if !std::fs::read(&path).is_ok_and(|existing| existing == bytes) {
        crate::config::atomic_write_private(&path, &bytes)?;
    }
    Ok(())
}

/// Run the two feature daemons exactly while their feature is on.
///
/// Neither is part of the audio path, so failing to start one is not a reason to
/// report the settings as unapplied — the filters the user asked for are already
/// running by this point. It is logged and left for the next reconcile.
///
/// Route watching is pointless in `Never`: every graph event would be parsed to
/// reach a decision that is always "do nothing".
fn reconcile_feature_daemons(settings: &AppSettings) {
    use crate::config::EchoMode;
    for (unit, wanted) in [
        // The level daemon exists for devices whose owner drew a window by
        // speaking close and far. With no such device there is nothing for it to
        // stay resident for.
        (
            pipewire::AGC_UNIT,
            settings
                .mic_calibration
                .devices
                .values()
                .any(|d| d.window().is_some()),
        ),
        (
            pipewire::ROUTE_UNIT,
            settings.echo_cancel.mode != EchoMode::Never,
        ),
    ] {
        if let Err(error) = pipewire::set_unit_running(unit, wanted) {
            log::warn!(
                "{unit} could not be {}: {error}",
                if wanted { "started" } else { "stopped" }
            );
        }
    }
}

/// Re-prime after a route change under the same lock as settings updates.
/// The dependent microphone must not retain state from the replaced source.
pub fn reprime_aec() -> io::Result<()> {
    let _guard = crate::config::storage::SettingsLock::acquire()?;
    let settings = AppSettings::load_strict()?.runtime_settings();
    if !settings.echo_cancel.enabled {
        return Ok(());
    }
    let wanted = Observed::wanted(&settings);
    execute_plan(
        Plan {
            aec: Action::Restart,
            mic: if wanted.mic_present {
                Action::Restart
            } else {
                Action::Keep
            },
            output: Action::Keep,
        },
        wanted,
        &mut NativeBackend,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chain {
    Aec,
    Microphone,
    Output,
}

trait Backend {
    fn service(&mut self, chain: Chain, action: Action) -> io::Result<()>;
    fn wait_aec(&mut self) -> io::Result<()>;
    fn wait_state(&mut self, wanted: Observed) -> io::Result<()>;
}

struct NativeBackend;

impl Backend for NativeBackend {
    fn service(&mut self, chain: Chain, action: Action) -> io::Result<()> {
        match (chain, action) {
            (_, Action::Keep) => Ok(()),
            (Chain::Aec, Action::Restart) => pipewire::restart_aec_service(),
            (Chain::Aec, Action::Stop) => pipewire::stop_aec_service(),
            (Chain::Microphone, Action::Restart) => pipewire::restart_mic_service(),
            (Chain::Microphone, Action::Stop) => pipewire::stop_mic_service(),
            (Chain::Output, Action::Restart) => pipewire::restart_output_service(),
            (Chain::Output, Action::Stop) => pipewire::stop_output_service(),
        }
    }
    fn wait_aec(&mut self) -> io::Result<()> {
        wait_for(
            |state| state.aec_present,
            "echo-cancellation source did not appear",
        )
    }
    fn wait_state(&mut self, wanted: Observed) -> io::Result<()> {
        wait_for(
            |state| state == wanted,
            "audio services did not reach the requested state",
        )
    }
}

fn execute_plan(plan: Plan, wanted: Observed, backend: &mut impl Backend) -> io::Result<()> {
    let mut failures = Vec::new();
    // Replacing an active AEC input can leave downstream DSP state poisoned.
    // Detach it first, then rebuild only after the replacement source exists.
    let detached = if plan.aec != Action::Keep && plan.mic == Action::Restart {
        match backend.service(Chain::Microphone, Action::Stop) {
            Ok(()) => true,
            Err(error) => {
                failures.push(format!("stop microphone before echo cancellation: {error}"));
                false
            }
        }
    } else {
        true
    };
    let aec_ready = detached
        && match backend.service(Chain::Aec, plan.aec).and_then(|()| {
            if plan.aec == Action::Restart {
                backend.wait_aec()
            } else {
                Ok(())
            }
        }) {
            Ok(()) => true,
            Err(error) => {
                failures.push(format!("echo cancellation: {error}"));
                false
            }
        };
    // Never prevent an independent stop or output repair because AEC failed.
    if !detached || (wanted.aec_present && !aec_ready && plan.mic == Action::Restart) {
        failures.push("microphone is waiting for echo cancellation".into());
    } else if let Err(error) = backend.service(Chain::Microphone, plan.mic) {
        failures.push(format!("microphone: {error}"));
    }
    if let Err(error) = backend.service(Chain::Output, plan.output) {
        failures.push(format!("output: {error}"));
    }
    if plan.changed()
        && let Err(error) = backend.wait_state(wanted)
    {
        failures.push(error.to_string());
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(failures.join("; ")))
    }
}

fn wait_for(predicate: impl Fn(Observed) -> bool, failure: &str) -> io::Result<()> {
    let started = Instant::now();
    let mut last_error = None;
    loop {
        match observe() {
            Ok(state) if predicate(state) => return Ok(()),
            Ok(_) => {}
            Err(error) => last_error = Some(error),
        }
        if started.elapsed() >= Duration::from_secs(3) {
            let detail = last_error
                .map_or_else(|| failure.to_owned(), |error| format!("{failure}: {error}"));
            return Err(io::Error::new(io::ErrorKind::TimedOut, detail));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn needs_mic_reload(prev: Option<&AppSettings>, now: &AppSettings) -> bool {
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
        // Selecting a different denoiser swaps the LADSPA plugin — different
        // .so, control surface and port names. The startup mute and the voice
        // gate's on/off and sensitivity are read when the plugin starts
        // running, and the SWH gate is a node of its own: only a reload picks
        // those up.
        let denoiser_topology_changed = p.noise_reduction.model != now.noise_reduction.model
            || p.noise_reduction.startup_mute_ms != now.noise_reduction.startup_mute_ms
            || p.noise_reduction.voice_gate_for(p.noise_reduction.model)
                != now
                    .noise_reduction
                    .voice_gate_for(now.noise_reduction.model)
            || p.noise_reduction.voice_sensitivity != now.noise_reduction.voice_sensitivity
            || p.gate.enabled != now.gate.enabled;
        // `target.object = "echo-cancel-source"` is added on the capture
        // side only when AEC is on. Toggling AEC rewrites that prop, so
        // the chain must be reloaded before the graph can use/bypass the
        // cleaned source.
        let ec_target_changed = p.echo_cancel.enabled != now.echo_cancel.enabled;
        // HPF is a 2-biquad cascade when enabled and a single
        // pass-through node when disabled — toggling it adds/removes
        // `hpf_pre` from the graph, so we must reload, not live-update.
        let hpf_topology_changed = p.hpf.enabled != now.hpf.enabled;
        p.gain_safety != now.gain_safety
            || p.equalizer.bands != now.equalizer.bands
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

pub fn output_topology_changed(prev: Option<&AppSettings>, now: &AppSettings) -> bool {
    // Master toggles set the denoiser's attenuation via the live update path.
    // EQ band/preset changes rewrite the graph, and so does selecting a
    // different denoiser, since the LADSPA plugin and port names differ.
    prev.is_none_or(|p| {
        p.gain_safety != now.gain_safety
            || p.output_filter.channel_mode != now.output_filter.channel_mode
            || p.output_filter.noise_reduction.enabled != now.output_filter.noise_reduction.enabled
            || p.output_filter.equalizer.bands != now.output_filter.equalizer.bands
            || p.output_filter.equalizer.preset != now.output_filter.equalizer.preset
            || p.output_filter.equalizer.enabled != now.output_filter.equalizer.enabled
            || p.output_filter.noise_reduction.model != now.output_filter.noise_reduction.model
            || p.output_filter.noise_reduction.startup_mute_ms
                != now.output_filter.noise_reduction.startup_mute_ms
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_requires_a_usable_graph_and_reports_its_model_not_saved_intent() {
        let settings = AppSettings::default();
        let mut graph = vec![
            serde_json::json!({"type": "PipeWire:Interface:Node", "info": {
                "state": "suspended", "props": {"node.name": pipeline::MIC_NODE_NAME,
                    "biglinux.model": "12", "biglinux.denoising": "true", "object.serial": 42}
            }}),
            serde_json::json!({"type": "PipeWire:Interface:Node", "info": {
                "state": "idle", "props": {"node.name": pipeline::MIC_CAPTURE_NODE_NAME}
            }}),
        ];
        let ready = status_from_graph(&settings, Some(&graph));
        assert_eq!(ready["mic_running"], true);
        assert_eq!(ready["effective_model"], 12);
        assert_eq!(ready["denoising"], true);
        assert_eq!(ready["mic_serial"], "42");
        graph[0]["info"]["props"]["biglinux.denoising"] = false.into();
        assert!(status_from_graph(&settings, Some(&graph))["effective_model"].is_null());
        graph[1]["info"]["state"] = "error".into();
        let failed = status_from_graph(&settings, Some(&graph));
        assert_eq!(failed["mic_running"], false);
        assert!(failed["effective_model"].is_null());
        assert!(failed["denoising"].is_null());
        assert_eq!(status_from_graph(&settings, None)["audio_available"], false);
        assert_eq!(
            status_from_graph(&settings, Some(&[]))["mic_running"],
            false
        );
    }

    #[test]
    fn voice_sensitivity_change_reloads_the_microphone() {
        let before = AppSettings::default();
        let mut now = before.clone();
        now.noise_reduction.voice_sensitivity = 5;
        assert!(needs_mic_reload(Some(&before), &now));
    }

    #[test]
    fn changing_a_gtcrn_experiment_restarts_the_dependent_microphone() {
        let before = AppSettings::default();
        let mut now = before.clone();
        now.echo_cancel.gtcrn_track_delay = !before.echo_cancel.gtcrn_track_delay;
        let changes = Changes::between(Some(&before), &now, false);
        let plan = Plan::new(
            &now,
            Some(&before),
            Observed::wanted(&before),
            changes,
            LiveOutcome::default(),
        );
        assert_eq!(plan.aec, Action::Restart);
        assert_eq!(plan.mic, Action::Restart);
        assert_eq!(plan.output, Action::Keep);
    }

    #[test]
    fn unchanged_aec_settings_keep_the_running_service() {
        let now = AppSettings::default();
        let changes = Changes::between(Some(&now), &now, false);
        let plan = Plan::new(
            &now,
            Some(&now),
            Observed::wanted(&now),
            changes,
            LiveOutcome::default(),
        );
        assert_eq!(plan.aec, Action::Keep);
    }

    #[test]
    fn forced_repair_does_not_depend_on_live_updates() {
        let settings = AppSettings::default();
        let changes = Changes::between(Some(&settings), &settings, true);
        let targets = changes.live_targets(&settings);
        assert!(!targets.microphone && !targets.output);
        let plan = Plan::new(
            &settings,
            Some(&settings),
            Observed::wanted(&settings),
            changes,
            LiveOutcome::default(),
        );
        assert_eq!(plan.mic, Action::Restart);
    }

    #[test]
    fn failed_controls_on_an_existing_endpoint_are_pending_not_restarted() {
        let mut before = AppSettings::default();
        before.output_filter.enabled = true;
        let mut now = before.clone();
        now.noise_reduction.strength = 0.23;
        let changes = Changes::between(Some(&before), &now, false);
        assert!(live_update_pending(
            changes.live_targets(&now),
            Observed::wanted(&now),
            LiveOutcome::default()
        ));
        // A suspended endpoint is present too. A missing mic, however, must
        // still be repairable, independently of a healthy output.
        let observed = Observed {
            mic_present: false,
            ..Observed::wanted(&now)
        };
        assert!(!live_update_pending(
            changes.live_targets(&now),
            observed,
            LiveOutcome::default()
        ));
        let forced = Changes::between(Some(&before), &now, true);
        assert!(!live_update_pending(
            forced.live_targets(&now),
            Observed::wanted(&now),
            LiveOutcome::default()
        ));
    }

    #[test]
    fn output_control_failure_is_also_non_destructive() {
        assert!(live_update_pending(
            UpdateTargets {
                microphone: false,
                output: true
            },
            Observed {
                output_present: true,
                ..Observed::default()
            },
            LiveOutcome::default(),
        ));
        assert!(!live_update_pending(
            UpdateTargets {
                microphone: true,
                output: true
            },
            Observed {
                mic_present: true,
                output_present: true,
                aec_present: true
            },
            LiveOutcome {
                mic_pushed: true,
                output_pushed: true
            },
        ));
    }

    #[test]
    fn unchanged_suspended_graphs_do_not_need_a_push_or_restart() {
        let before = AppSettings::default();
        let mut now = before.clone();
        now.window.width += 100;
        now.ui.show_advanced = !now.ui.show_advanced;
        let changes = Changes::between(Some(&before), &now, false);
        let targets = changes.live_targets(&now);
        assert!(!targets.microphone && !targets.output);
        let plan = Plan::new(
            &now,
            Some(&before),
            Observed::wanted(&now),
            changes,
            LiveOutcome::default(),
        );
        assert!(!plan.changed());
    }

    #[test]
    fn a_missing_endpoint_is_recovered_even_without_a_setting_change() {
        let settings = AppSettings::default();
        let changes = Changes::between(Some(&settings), &settings, false);
        let plan = Plan::new(
            &settings,
            Some(&settings),
            Observed::default(),
            changes,
            LiveOutcome::default(),
        );
        assert_eq!(plan.mic, Action::Restart);
    }

    #[derive(Default)]
    struct FakeBackend {
        calls: Vec<(Chain, Action)>,
        fail_aec: bool,
        fail_mic_stop: bool,
        aec_ready: bool,
    }
    impl Backend for FakeBackend {
        fn service(&mut self, chain: Chain, action: Action) -> io::Result<()> {
            self.calls.push((chain, action));
            if chain == Chain::Aec && self.fail_aec {
                Err(io::Error::other("injected AEC failure"))
            } else if chain == Chain::Microphone && action == Action::Stop && self.fail_mic_stop {
                Err(io::Error::other("injected microphone stop failure"))
            } else {
                if chain == Chain::Microphone && action == Action::Restart {
                    assert!(self.aec_ready, "microphone restarted before AEC was ready");
                }
                Ok(())
            }
        }
        fn wait_aec(&mut self) -> io::Result<()> {
            assert_eq!(self.calls.last(), Some(&(Chain::Aec, Action::Restart)));
            self.aec_ready = true;
            Ok(())
        }
        fn wait_state(&mut self, _: Observed) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn aec_failure_does_not_prevent_independent_stop_and_output_repair() {
        let mut backend = FakeBackend {
            fail_aec: true,
            ..Default::default()
        };
        let plan = Plan {
            aec: Action::Stop,
            mic: Action::Stop,
            output: Action::Restart,
        };
        assert!(execute_plan(plan, Observed::default(), &mut backend).is_err());
        assert!(backend.calls.contains(&(Chain::Microphone, Action::Stop)));
        assert!(backend.calls.contains(&(Chain::Output, Action::Restart)));
    }

    #[test]
    fn aec_replacement_detaches_microphone_and_waits_before_rebuilding_it() {
        let mut backend = FakeBackend::default();
        let plan = Plan {
            aec: Action::Restart,
            mic: Action::Restart,
            output: Action::Keep,
        };
        let wanted = Observed {
            aec_present: true,
            mic_present: true,
            output_present: false,
        };
        execute_plan(plan, wanted, &mut backend).unwrap();
        assert_eq!(
            backend.calls,
            [
                (Chain::Microphone, Action::Stop),
                (Chain::Aec, Action::Restart),
                (Chain::Microphone, Action::Restart),
                (Chain::Output, Action::Keep),
            ]
        );
    }

    #[test]
    fn failed_microphone_stop_prevents_aec_replacement_but_not_output_repair() {
        let mut backend = FakeBackend {
            fail_mic_stop: true,
            ..Default::default()
        };
        let plan = Plan {
            aec: Action::Restart,
            mic: Action::Restart,
            output: Action::Restart,
        };
        assert!(execute_plan(plan, Observed::default(), &mut backend).is_err());
        assert_eq!(
            backend.calls,
            [
                (Chain::Microphone, Action::Stop),
                (Chain::Output, Action::Restart),
            ]
        );
    }

    #[test]
    fn suspended_nodes_exist_but_error_nodes_do_not_count_as_ready() {
        let graph = serde_json::json!([
            {"type":"PipeWire:Interface:Node","info":{"state":"suspended","props":{"node.name":"output-biglinux"}}},
            {"type":"PipeWire:Interface:Node","info":{"state":"error","props":{"node.name":"echo-cancel-source"}}}
        ]);
        let result = from_graph(graph.as_array().unwrap());
        assert!(result.output_present);
        assert!(!result.aec_present);
    }
}
