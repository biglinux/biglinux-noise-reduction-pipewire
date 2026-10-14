//! What the microphone path costs, stage by stage.
//!
//! No single "your latency is N milliseconds" is true. Nothing cheap
//! can see the acoustic path from a mouth to a capsule, the device's own DMA
//! buffering, or how much audio the application on the other side holds before
//! it plays. A number that pretends otherwise is worse than no number, and the
//! project's own plan says so in as many words: *"não gravar '14,67 ms ponta a
//! ponta' na interface"*.
//!
//! So this reports the stages it can account for, each one saying whether it
//! was measured and where the figure came from, a total that is explicitly a
//! sum of those stages, and a list of what is deliberately missing from it.

use serde::Serialize;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

use crate::config::{AppSettings, NoiseModel};
use crate::pipeline::MIC_NODE_NAME;
use crate::services::pipewire::graph_snapshot;

/// One accountable contribution to the delay.
#[derive(Debug, Serialize)]
pub struct Stage {
    pub stage: &'static str,
    pub ms: f64,
    /// True only for an observed duration, never a requested buffer or model
    /// estimate. Reading a setting is not measuring audio delay.
    pub measured: bool,
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub total_ms: f64,
    pub total_is: &'static str,
    pub rate: u32,
    pub model: &'static str,
    pub model_id: u8,
    pub stages: Vec<Stage>,
    pub not_included: Vec<&'static str>,
}

/// The graph's own rate. Everything here is quoted at it.
const RATE: u32 = 48_000;
/// PipeWire's default quantum when the graph does not say otherwise.
const DEFAULT_QUANTUM: u32 = 1024;
/// The tool whose numbers people compare this report against.
const PW_TOP: &str = "/usr/bin/pw-top";

/// The plugins' own delay at 48 kHz, measured by correlation through them with
/// attenuation disabled (DPDFNet also declares its figure on a latency port).
fn model_delay(model: NoiseModel) -> (f64, &'static str) {
    match model {
        NoiseModel::Dfn3 => (
            1919.0,
            "native DFN3 W8A16 plugin, 1919 samples measured at 48 kHz; live build not verified",
        ),
        NoiseModel::Dfn3Ll => (
            959.0,
            "native DFN3-LL W8A16 plugin, 959 samples measured at 48 kHz; live build not verified",
        ),
        NoiseModel::Dpdfnet => (
            2880.0,
            "native DPDFNet-2 48 kHz HR plugin, 2880 samples declared and measured at 48 kHz; live build not verified",
        ),
    }
}

/// With the voice gate on, every denoiser plugin holds its output until the
/// detector has decided on it: 3168 samples in total, whatever the model.
const VOICE_GATE_TOTAL_SAMPLES: f64 = 3168.0;

fn frames_to_ms(frames: u32) -> f64 {
    f64::from(frames) * 1000.0 / f64::from(RATE)
}

/// The cycle the graph is actually running, in frames, from the same profiler
/// `pw-top` reads. `None` when pw-top is missing, says nothing, or the graph is
/// idle — none of which is a number to put on a screen.
///
/// Every node in a driver group runs the driver's cycle, so the figure is the
/// graph's, not one node's: pw-top prints it against the driver and leaves the
/// followers at zero, which is why reading our own node's row finds nothing.
fn realized_quantum() -> Option<u32> {
    // Two batches, not one: the first has no profiler data yet and prints every
    // node at zero, which reads exactly like an idle graph. The numbers are in
    // the second, so only what follows the last header is parsed.
    let output = BigSubprocessSpec::builder()
        .program(PW_TOP)
        .args(["-b", "-n", "2"])
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list([PW_TOP])
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .run()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    quant_from_pw_top(&output.stdout_lossy())
}

/// Resolve only the microphone's driver group in the last batch. A larger
/// quantum in an unrelated playback group says nothing about this microphone.
/// Unrecognized/ambiguous layouts fail closed to the labeled request estimate.
fn quant_from_pw_top(text: &str) -> Option<u32> {
    let last_block = text.rsplit_once("S   ID").map_or(text, |(_, tail)| tail);
    let mut driver = None;
    let mut found = None;
    let mut matches = 0;
    for line in last_block.lines() {
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns.len() < 10 || columns[1].parse::<u32>().is_err() {
            continue;
        }
        let name = *columns.last()?;
        let follower = columns[9..].iter().any(|s| *s == "+" || *s == "=")
            || name.starts_with('+')
            || name.starts_with('=');
        let own_quantum = if matches!(columns[0], "R" | "t" | "T")
            && columns[3].parse::<u32>().ok() == Some(RATE)
        {
            columns[2].parse::<u32>().ok().filter(|q| *q > 0)
        } else {
            None
        };
        if !follower {
            driver = own_quantum;
        }
        if name.trim_start_matches(['+', '=']) == MIC_NODE_NAME {
            matches += 1;
            found = if follower { driver } else { own_quantum };
        }
    }
    if matches == 1 { found } else { None }
}

/// Frames in a `node.latency` / quantum property spelled `"960/48000"`.
fn frames_of(property: &str) -> Option<u32> {
    let (frames, rate) = property.split_once('/')?;
    let frames: u32 = frames.trim().parse().ok()?;
    (rate.trim().parse::<u32>().ok()? == RATE && frames > 0).then_some(frames)
}

/// How much audio the chain's own nodes buffer per cycle.
///
/// This is `node.latency`: the buffer our chain ASKS the graph for, which is a
/// request and not an outcome. PipeWire runs one cycle for the whole driver
/// group, and its size is the smallest request among the active nodes, rounded
/// to what the driver allows — so another application asking for more, or a
/// device that cannot go that low, leaves our nodes running at something else.
///
/// The report used to call this "the same frames pw-top prints", which it is
/// not: pw-top's QUANT column is the realized cycle. Checked here with the
/// chain up, `node.latency` said 960 frames while pw-top printed 1024 on the
/// driver and 0 on our own nodes, which are followers. A number whose whole
/// point is being checkable has to say which of the two it is.
fn quantum() -> (u32, bool) {
    let requested = frames_of(crate::pipeline::AEC_NODE_LATENCY).unwrap_or(DEFAULT_QUANTUM);
    let Ok(graph) = graph_snapshot() else {
        return (requested, false);
    };
    let live = graph
        .iter()
        .filter(|object| object["type"] == "PipeWire:Interface:Node")
        .find(|object| object["info"]["props"]["node.name"] == MIC_NODE_NAME)
        .and_then(|object| object["info"]["props"]["node.latency"].as_str())
        .and_then(frames_of);
    match live {
        Some(frames) => (frames, true),
        None => (requested, false),
    }
}

/// Account for the stages of the microphone path under the current settings.
#[must_use]
pub fn report(settings: &AppSettings, model: Option<NoiseModel>) -> Report {
    build_report(settings, model, quantum(), realized_quantum())
}

fn build_report(
    settings: &AppSettings,
    requested_model: Option<NoiseModel>,
    observation: (u32, bool),
    realized: Option<u32>,
) -> Report {
    let effective = settings.runtime_settings();
    let include_ai = requested_model.is_some() || crate::pipeline::ai_node_in_mic_chain(&effective);
    let model = requested_model.unwrap_or(effective.noise_reduction.model);
    let (requested_frames, running) = observation;
    let quantum_frames = realized.unwrap_or(requested_frames);
    let quantum_ms = frames_to_ms(quantum_frames);
    let (denoiser_samples, denoiser_source) = model_delay(model);

    let mut stages = vec![Stage {
        stage: "capture-buffer",
        ms: quantum_ms,
        // One cycle is a planning assumption for a buffer contribution, even
        // when that cycle's length was observed. pw-top is not a latency probe.
        measured: false,
        source: match (realized, running) {
            (Some(_), _) => {
                "one assumed capture period using the microphone driver's observed QUANT"
            }
            (None, true) => {
                "one assumed period using node.latency; actual driver duration is unavailable"
            }
            (None, false) => {
                "one assumed period using configured latency; no live duration was read"
            }
        },
        frames: Some(quantum_frames),
    }];

    if effective.echo_cancel.enabled && !effective.mic_bypass {
        stages.push(Stage {
            stage: "echo-cancel",
            ms: 16.0,
            measured: false,
            source: "the echo canceller's stated frame, not a reading of this machine",
            frames: None,
        });
    }

    if include_ai {
        stages.push(Stage {
            // Not a reading of this machine. It is the plugin's own algorithmic
            // delay, which is a property of the network and not of the computer
            // running it — but it was measured once, on this project's hardware,
            // and every figure here has to say which of those it is.
            stage: "denoiser",
            ms: denoiser_samples / 48.0,
            measured: false,
            source: denoiser_source,
            frames: None,
        });

        if effective.noise_reduction.voice_gate_for(model) {
            stages.push(Stage {
                stage: "voice-gate",
                ms: (VOICE_GATE_TOTAL_SAMPLES - denoiser_samples) / 48.0,
                measured: false,
                source: "the voice gate's hold until its detector decides, measured with the plugin at 48 kHz",
                frames: None,
            });
        }
    }
    if !effective.mic_bypass {
        stages.push(Stage {
            stage: "filter-output-buffer",
            ms: quantum_ms,
            measured: false,
            source: "one additional output period assumed by this estimate; not measured or inferred from QUANT",
            frames: Some(quantum_frames),
        });
    }
    Report {
        total_ms: (stages.iter().map(|stage| stage.ms).sum::<f64>() * 10.0).round() / 10.0,
        total_is: "a planning estimate with assumed buffer periods, not an end-to-end measurement",
        rate: RATE,
        model: model.plugin_and_label().1,
        model_id: model as u8,
        stages,
        not_included: vec![
            "the acoustic path from your mouth to the microphone",
            "the sound card's own buffering",
            "whatever the application receiving you holds before it plays",
            "the network, when there is one",
            "live plugin build/model delay validation and scheduling overlap between stages",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(settings: &AppSettings, model: Option<NoiseModel>) -> Report {
        build_report(
            settings,
            model,
            (
                frames_of(crate::pipeline::AEC_NODE_LATENCY).unwrap_or(DEFAULT_QUANTUM),
                false,
            ),
            None,
        )
    }

    #[test]
    fn every_stage_says_where_its_number_came_from() {
        let accounted = report(&AppSettings::default(), Some(NoiseModel::Dfn3));
        assert!(!accounted.stages.is_empty());
        for stage in &accounted.stages {
            assert!(
                !stage.source.is_empty(),
                "{} has no provenance",
                stage.stage
            );
            assert!(stage.ms >= 0.0);
        }
        // The total is the sum it claims to be, to the tenth it is rounded to.
        let sum: f64 = accounted.stages.iter().map(|stage| stage.ms).sum();
        assert!(
            (accounted.total_ms - sum).abs() <= 0.05,
            "{} vs {sum}",
            accounted.total_ms
        );
    }

    #[test]
    fn the_voice_gate_brings_every_model_to_the_same_delay() {
        let mut settings = AppSettings::default();
        settings.noise_reduction.dpdfnet_voice_gate = true;
        let gate_stage = |settings: &AppSettings, model| {
            report(settings, Some(model))
                .stages
                .iter()
                .any(|stage| stage.stage == "voice-gate")
        };
        for model in NoiseModel::ALL {
            let accounted = report(&settings, Some(model));
            let delay: f64 = accounted
                .stages
                .iter()
                .filter(|stage| matches!(stage.stage, "denoiser" | "voice-gate"))
                .map(|stage| stage.ms)
                .sum();
            assert!((delay - 66.0).abs() < 1e-9, "{model:?}: {delay}");
        }
        settings.noise_reduction.voice_gate = false;
        assert!(!gate_stage(&settings, NoiseModel::Dfn3));
        assert!(gate_stage(&settings, NoiseModel::Dpdfnet));
        settings.noise_reduction.dpdfnet_voice_gate = false;
        assert!(!gate_stage(&settings, NoiseModel::Dpdfnet));
    }

    #[test]
    fn the_buffer_stages_quote_the_chains_own_frames_not_pipewires_default() {
        // No live observation: the fallback must be the chain's requested
        // period, explicitly not a measured buffer delay.
        let requested = frames_of(crate::pipeline::AEC_NODE_LATENCY).expect("chain latency");
        assert_ne!(requested, DEFAULT_QUANTUM);
        let accounted = report(&AppSettings::default(), Some(NoiseModel::Dfn3));
        for stage in accounted
            .stages
            .iter()
            .filter(|stage| stage.frames.is_some())
        {
            // With a graph up this is the live reading; without one, the request.
            assert!(
                stage.frames == Some(requested) || stage.measured,
                "{stage:?}"
            );
        }
    }

    #[test]
    fn a_latency_property_is_only_frames_when_it_is_quoted_at_our_rate() {
        assert_eq!(frames_of("960/48000"), Some(960));
        assert_eq!(frames_of("512/44100"), None);
        assert_eq!(frames_of("0/48000"), None);
        assert_eq!(frames_of("nonsense"), None);
    }

    #[test]
    fn what_cannot_be_counted_is_listed_rather_than_folded_in() {
        let accounted = report(&AppSettings::default(), None);
        assert!(accounted.not_included.len() >= 3);
        assert!(accounted.total_is.contains("not an end-to-end"));
    }

    /// Real `pw-top -b -n 2` output from this workstation, trimmed. The first
    /// batch has no profiler data and prints everything at zero, which is why
    /// only the last one counts; followers print zero even while running, which
    /// is why the driver's row is the graph's cycle.
    #[test]
    fn the_cycle_comes_from_the_last_batch_pw_top_printed() {
        let dump = "\
S   ID  QUANT   RATE    WAIT    BUSY   W/Q   B/Q  ERR FORMAT           NAME
C   29      0      0    ---     ---   ---   ---     0                  Dummy-Driver
C   60      0      0    ---     ---   ---   ---     0                  alsa_output.analog-stereo
S   ID  QUANT   RATE    WAIT    BUSY   W/Q   B/Q  ERR FORMAT           NAME
R   60   1024  48000  60.2us  17.0us  0.00  0.00    6    S32LE 2 48000 alsa_output.analog-stereo
R  249      0      0   0.0us   0.0us  ???   ???     0     F32P 2 48000 + mic-biglinux
";
        assert_eq!(quant_from_pw_top(dump), Some(1024));
        let unrelated = format!("{dump}R 99 4096 48000 0 0 0 0 0 S32LE 2 48000 other-driver\n");
        assert_eq!(quant_from_pw_top(&unrelated), Some(1024));
        assert_eq!(
            quant_from_pw_top(&dump.replace("+ mic-biglinux", "= mic-biglinux")),
            Some(1024)
        );
        assert_eq!(
            quant_from_pw_top(&dump.replace("+ mic-biglinux", "other-source")),
            None
        );

        // A graph nobody is driving, and a rate that is not ours, are both "no
        // answer" rather than a number to print.
        assert_eq!(quant_from_pw_top("S   ID  QUANT   RATE\nC 29 0 0\n"), None);
        assert_eq!(
            quant_from_pw_top("S   ID  QUANT   RATE\nR 60 512 44100\n"),
            None
        );
        assert_eq!(quant_from_pw_top(""), None);
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::*;
    #[test]
    fn bypassed_path_does_not_charge_for_inactive_filters() {
        let settings = AppSettings {
            mic_bypass: true,
            ..Default::default()
        };
        let report = build_report(&settings, None, (960, true), Some(512));
        assert!(
            report
                .stages
                .iter()
                .all(|s| s.stage == "capture-buffer" && !s.measured)
        );
    }
    #[test]
    fn requested_model_remains_available_as_a_comparison_estimate() {
        let settings = AppSettings {
            mic_bypass: true,
            ..Default::default()
        };
        let report = build_report(&settings, Some(NoiseModel::Dfn3), (960, false), None);
        assert!(report.stages.iter().any(|s| s.stage == "denoiser"));
        assert!(report.stages.iter().all(|s| !s.measured));
    }
}
