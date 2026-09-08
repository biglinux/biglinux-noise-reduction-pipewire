//! Reconcile saved intent, live controls and independently recoverable services.
//!
//! Call on a worker while holding SettingsLock. A failed fast path is a
//! reason to rebuild its chain, never a prerequisite for repairing another.
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
        Self {
            aec: action(
                settings.echo_cancel.enabled,
                observed.aec_present,
                previous.is_some_and(|s| s.echo_cancel.enabled),
                false,
                changes.force,
                true,
            ),
            mic: action(
                pipeline::mic_chain_wanted(settings),
                observed.mic_present,
                previous.is_some_and(pipeline::mic_chain_wanted),
                changes.mic_topology && changes.microphone,
                changes.force,
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

pub fn apply(settings: &AppSettings, force: bool) -> io::Result<()> {
    let settings = settings.runtime_settings();
    pipeline::apply(&settings)?;
    let previous: Option<AppSettings> = std::fs::read(baseline_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let changes = Changes::between(previous.as_ref(), &settings, force);
    let (graph, observation_error) = match pipewire::graph_snapshot() {
        Ok(graph) => (graph, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let observed = from_graph(&graph);
    let report = pipewire::apply_live_to_graph(&settings, &graph, changes.live_targets(&settings));
    for failure in &report.failures {
        log::warn!("{failure}; rebuilding the affected chain");
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
        if let Some(cause) = observation_error {
            causes.push(cause);
        }
        causes.push(error.to_string());
        return Err(io::Error::other(causes.join("; ")));
    }
    if !plan.changed()
        && let Some(cause) = observation_error
    {
        return Err(io::Error::other(cause));
    }
    let bytes = serde_json::to_vec(&settings).map_err(io::Error::other)?;
    let path = baseline_path();
    if !std::fs::read(&path).is_ok_and(|existing| existing == bytes) {
        crate::config::atomic_write_private(&path, &bytes)?;
    }
    Ok(())
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
    let aec_ready = match backend.service(Chain::Aec, plan.aec).and_then(|()| {
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
    if wanted.aec_present && !aec_ready && plan.mic == Action::Restart {
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
    // GTCRN is permanently wired in the output graph; NR / master
    // toggles flip its `Enable` port via the live update path. EQ
    // band/preset changes rewrite the graph, and so does selecting a
    // different denoiser backend (GTCRN vs attenuation-only) since the LADSPA
    // plugin and port names differ.
    prev.is_none_or(|p| {
        p.gain_safety != now.gain_safety
            || p.output_filter.channel_mode != now.output_filter.channel_mode
            || p.output_filter.noise_reduction.enabled != now.output_filter.noise_reduction.enabled
            || p.output_filter.equalizer.bands != now.output_filter.equalizer.bands
            || p.output_filter.equalizer.preset != now.output_filter.equalizer.preset
            || p.output_filter.equalizer.enabled != now.output_filter.equalizer.enabled
            || p.output_filter.noise_reduction.model != now.output_filter.noise_reduction.model
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_failed_microphone_push_does_not_restart_the_output() {
        let mut before = AppSettings::default();
        before.output_filter.enabled = true;
        let mut now = before.clone();
        now.noise_reduction.strength = 0.23;
        let changes = Changes::between(Some(&before), &now, false);
        let plan = Plan::new(
            &now,
            Some(&before),
            Observed::wanted(&now),
            changes,
            LiveOutcome::default(),
        );
        assert_eq!(plan.mic, Action::Restart);
        assert_eq!(plan.output, Action::Keep);
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
    }
    impl Backend for FakeBackend {
        fn service(&mut self, chain: Chain, action: Action) -> io::Result<()> {
            self.calls.push((chain, action));
            if chain == Chain::Aec {
                Err(io::Error::other("injected AEC failure"))
            } else {
                Ok(())
            }
        }
        fn wait_aec(&mut self) -> io::Result<()> {
            unreachable!()
        }
        fn wait_state(&mut self, _: Observed) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn aec_failure_does_not_prevent_independent_stop_and_output_repair() {
        let mut backend = FakeBackend::default();
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
