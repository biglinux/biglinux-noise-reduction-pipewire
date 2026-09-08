//! One checked reconciliation path for the GUI, CLI and login watcher.
//!
//! Call while holding SettingsLock on a worker. The durable settings express
//! intent; this module separately observes the graph and records a baseline
//! only after the requested nodes exist. Suspended nodes are valid endpoints
//! but cannot accept live controls, so their arguments are reloaded instead.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use serde_json::Value;

use super::pipewire::{self, LiveOutcome};
use crate::config::AppSettings;
use crate::pipeline;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Observed {
    pub mic_present: bool,
    pub output_present: bool,
    pub aec_present: bool,
}

pub fn observe() -> io::Result<Observed> {
    let output = BigSubprocessSpec::builder()
        .program("/usr/bin/pw-dump")
        .allow_list(["/usr/bin/pw-dump"])
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(2))
        .build()
        .run()
        .map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other("PipeWire is not responding"));
    }
    let graph: Vec<Value> = serde_json::from_slice(&output.stdout).map_err(io::Error::other)?;
    Ok(from_graph(&graph))
}

fn from_graph(graph: &[Value]) -> Observed {
    let nodes: HashSet<&str> = graph
        .iter()
        .filter(|object| object["type"] == "PipeWire:Interface:Node")
        .filter(|object| object["info"]["state"] != "error")
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
        if present || was_wanted {
            Action::Stop
        } else {
            Action::Keep
        }
    } else if force || changed || !present || !pushed {
        // systemctl restart also starts an inactive unit; unlike start it
        // repairs an active process whose expected node disappeared.
        Action::Restart
    } else {
        Action::Keep
    }
}

fn baseline_path() -> PathBuf {
    dirs::runtime_dir().map_or_else(crate::config::config_dir, |path| path.join("biglinux-microphone"))
        .join("last-applied.json")
}

/// Apply saved settings and propagate every required service failure.
pub fn apply(settings: &AppSettings, force: bool) -> io::Result<()> {
    let effective = settings.runtime_settings();
    let settings = &effective;
    pipeline::apply(settings)?;
    let previous: Option<AppSettings> = std::fs::read(baseline_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let observed = observe()?;
    let live = pipewire::apply_live(settings)?;
    let force = force
        || previous
            .as_ref()
            .is_some_and(|old| old.runtime != settings.runtime);
    execute(settings, previous.as_ref(), observed, live, force)?;
    let bytes = serde_json::to_vec(settings).map_err(io::Error::other)?;
    let path = baseline_path();
    if !std::fs::read(&path).is_ok_and(|existing| existing == bytes) {
        crate::config::atomic_write_private(&path, &bytes)?;
    }
    Ok(())
}

fn execute(
    settings: &AppSettings,
    previous: Option<&AppSettings>,
    observed: Observed,
    live: LiveOutcome,
    force: bool,
) -> io::Result<()> {
    let aec = action(
        settings.echo_cancel.enabled,
        observed.aec_present,
        previous.is_some_and(|s| s.echo_cancel.enabled),
        false,
        force,
        true,
    );
    let mic = action(
        pipeline::mic_chain_wanted(settings),
        observed.mic_present,
        previous.is_some_and(pipeline::mic_chain_wanted),
        needs_mic_reload(previous, settings),
        force,
        live.mic_pushed,
    );
    let output = action(
        settings.output_filter.enabled,
        observed.output_present,
        previous.is_some_and(|s| s.output_filter.enabled),
        output_topology_changed(previous, settings),
        force,
        live.output_pushed,
    );

    // The cleaned source must be published before a microphone chain pins it.
    match aec {
        Action::Restart => {
            pipewire::restart_aec_service()?;
            wait_for(
                |state| state.aec_present,
                "echo-cancellation source did not appear",
            )?;
        }
        Action::Stop => pipewire::stop_aec_service()?,
        Action::Keep => {}
    }
    match mic {
        Action::Restart => pipewire::restart_mic_service()?,
        Action::Stop => pipewire::stop_mic_service()?,
        Action::Keep => {}
    }
    match output {
        Action::Restart => pipewire::restart_output_service()?,
        Action::Stop => pipewire::stop_output_service()?,
        Action::Keep => {}
    }
    if [aec, mic, output].iter().any(|step| *step != Action::Keep) {
        wait_for(
            |state| {
                state.mic_present == pipeline::mic_chain_wanted(settings)
                    && state.output_present == settings.output_filter.enabled
                    && state.aec_present == settings.echo_cancel.enabled
            },
            "The audio services did not reach the requested state",
        )?;
    }
    Ok(())
}

fn wait_for(predicate: impl Fn(Observed) -> bool, failure: &str) -> io::Result<()> {
    let started = Instant::now();
    loop {
        if predicate(observe()?) {
            return Ok(());
        }
        if started.elapsed() >= Duration::from_secs(3) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, failure.to_owned()));
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

pub fn output_topology_changed(prev: Option<&AppSettings>, now: &AppSettings) -> bool {
    // GTCRN is permanently wired in the output graph; NR / master
    // toggles flip its `Enable` port via the live update path. EQ
    // band/preset changes rewrite the graph, and so does selecting a
    // different denoiser backend (GTCRN vs attenuation-only) since the LADSPA
    // plugin and port names differ.
    prev.is_none_or(|p| {
        p.output_filter.channel_mode != now.output_filter.channel_mode
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
    fn a_missing_enabled_output_or_aec_is_recovered_without_a_setting_change() {
        assert_eq!(
            action(true, false, true, false, false, false),
            Action::Restart
        );
        assert_eq!(
            action(true, false, true, false, false, true),
            Action::Restart
        );
        assert_eq!(action(true, true, true, false, false, true), Action::Keep);
        assert_eq!(action(true, true, true, true, false, true), Action::Restart);
        assert_eq!(action(false, true, true, false, false, false), Action::Stop);
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
