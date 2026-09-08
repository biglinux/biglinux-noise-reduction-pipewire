// SPDX-License-Identifier: GPL-3.0-or-later
//! **Which noise model to run** — §38 of the audio guide, as a policy rather than a list.
//!
//! §38 asks for three choices and gives the automatic one a list of inputs: what the
//! processor can do, how loaded the graph is, whether blocks are being missed, whether the
//! machine is on battery, and how many filters are running at once. Two sentences of that
//! section carry the design: *"a política precisa ser reproduzível e testável"* and
//! *"não trocar modelo a cada pequena variação — usar histerese"*.
//!
//! So this is one pure function over one struct of readings, and every rule in it is
//! checked by a test. Nothing here touches the graph, reads a file, or looks at a clock.
//!
//! **Hysteresis is the whole reason the current model is an input.** A policy that only
//! looked at the readings would move between models whenever the load crossed a line, and
//! swapping a model reloads the filter chain: the person would hear a gap every time
//! something else on the machine got busy. So a model in force keeps its place until the
//! readings are clearly past the line rather than merely over it, and the two bands are
//! far enough apart that ordinary variation never reaches the far one.
//!
//! **The lightest model is the floor, never a refusal.** Whatever the readings say, the
//! automatic choice is a model, because the alternative to a heavy model is a light one
//! and never silence.

use serde::{Deserialize, Serialize};

use super::NoiseModel;

/// What the person asked for (§38's three rows).
///
/// The serialised word is spelled out on each variant rather than derived, and it is the
/// same word [`Quality::word`] writes and the command line accepts. Left to `rename_all`
/// the file said `automatic` while the command said `auto`, which is one state with two
/// names — and the reader on the other side of the file is a different program.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    /// Let this computer decide, and change its mind when the machine changes.
    #[default]
    #[serde(rename = "auto")]
    Automatic,
    /// The best model that will load, whatever it costs.
    #[serde(rename = "best")]
    Best,
    /// The cheapest model, so the machine stays free for everything else.
    #[serde(rename = "cheapest")]
    Cheapest,
    /// Somebody picked a model by name in the enhancer's own expert list.
    ///
    /// Not one of §38's three rows and never offered as one: it is the state the settings
    /// are already in when a person has been through that list, and a policy that quietly
    /// overwrote their choice on the next login would be a list that does not work. Any of
    /// the three above replaces it.
    #[serde(rename = "manual")]
    Manual,
}

impl Quality {
    /// Parse the word a command line or a settings file carries.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "auto" | "automatic" => Self::Automatic,
            "best" | "quality" => Self::Best,
            "cheapest" | "economy" | "light" => Self::Cheapest,
            "manual" => Self::Manual,
            _ => return None,
        })
    }

    /// The word this writes back, which is the one `parse` prefers.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Automatic => "auto",
            Self::Best => "best",
            Self::Cheapest => "cheapest",
            Self::Manual => "manual",
        }
    }

    /// Whether the model is this policy's to write.
    #[must_use]
    pub const fn decides_the_model(self) -> bool {
        !matches!(self, Self::Manual)
    }
}

/// What the machine looks like right now, in the terms §38 lists.
///
/// Every field is something a caller can measure without guessing. `None` means the
/// reading was not available on this machine, and a policy that treats an absent reading
/// as a bad one would put every machine with no battery into economy mode.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Machine {
    /// How many processors the scheduler can use.
    pub cores: usize,
    /// Running on battery rather than mains, where that is knowable.
    pub on_battery: Option<bool>,
    /// The share of its available time the sound graph is using, 0 to 1.
    pub graph_load: Option<f32>,
    /// Blocks missed recently — any at all is the strongest signal here, because it is
    /// the only one the person can actually hear.
    pub recent_xruns: u32,
    /// How many filter chains are loaded at once.
    pub filters: usize,
    /// A call is in progress, where that is knowable.
    pub in_a_call: Option<bool>,
    /// What the heavy model measured on this machine: the share of one callback deadline
    /// its audio thread spends, at p99.
    ///
    /// This is the only input here that is about the model rather than the machine, and
    /// it is the one that actually answers the question. Core count says nothing about
    /// how fast those cores are, and a policy built on it promotes a slow eight-core
    /// laptop and refuses a fast quad.
    pub heavy_share: Option<f32>,
}

/// A processor with fewer than this many cores does not get a heavy model at all.
///
/// Four is where the models this ships stop fitting alongside a desktop: below it the
/// realtime thread and the compositor are competing for the same two cores.
const ENOUGH_CORES: usize = 4;

/// Above this share of the period, the graph has no room for a heavier model.
const CROWDED: f32 = 0.55;

/// Above this share of the deadline, the heavy model does not belong on this machine.
///
/// Half, because the measurement is taken on an idle machine and the graph has to carry
/// the echo canceller, the resampler and whatever the person is doing. On the reference
/// i5-13400 the heavy model measures 1.3 % and DeepFilterNet3 measures 62 %, so a machine
/// would have to be forty times slower than this one to be refused.
const TOO_DEAR: f32 = 0.5;

/// And below this share it has room again. The gap between the two is the hysteresis
/// §38 asks for: a graph oscillating around 0.55 would otherwise reload the chain every
/// few seconds, and a reload is a gap somebody hears.
const ROOMY: f32 = 0.35;

/// The heaviest model this policy will choose, and the lightest.
///
/// Named rather than computed from the enum's order: the enum is a catalogue that
/// includes models meant for offline pipelines, and "the last one" is not a policy.
///
/// Both come from measurement on an i5-13400 at a 1920-sample block, one run over the
/// VoiceBank+DEMAND test split, driving the shipped plugins rather than their model
/// files. What decides it is the middle column: the share of the callback deadline the
/// **audio thread** spends, which is what a dropout is made of.
///
/// | model | p99 of deadline | whole CPU | PESQ | delay |
/// | --- | --- | --- | --- | --- |
/// | DeepFilterNet3 | 63 % | 46 % | 3.34 | 20 ms |
/// | DPDFNet v2 48k HR | 0.8 % | 33 % | 3.12 | 60 ms |
/// | GTCRN DNS3 | 29 % | 13 % | 2.53 | 24 ms |
/// | DPDFNet v8 48k HR | 0.8 % | 57 % | 3.00 | 60 ms |
///
/// DeepFilterNet3 scores highest and runs its network on the audio thread, so two thirds
/// of the deadline is gone before the rest of the graph is served — on a machine slower
/// than this one that is a dropout, which is the fault this whole stack exists to avoid.
/// DPDFNet answers from a worker and leaves the callback almost empty, for 0.22 of PESQ
/// and 36 ms of delay. Sixty milliseconds is well inside what a call tolerates, and a
/// dropout is not, so the automatic choice takes the safe one.
///
/// `DpdfnetV8Hr` is in the catalogue and is never a step here: it costs almost twice the
/// CPU of v2 and scored *below* it on every intrusive metric.
const HEAVY: NoiseModel = NoiseModel::DpdfnetV2Hr;
const LIGHT: NoiseModel = NoiseModel::GtcrnDns3;

/// What "best" means when a person asks for it by name.
///
/// The row above chooses for somebody who did not choose, so it refuses a risk they did
/// not take on. "Best" is the opposite promise — highest score, whatever it costs — and
/// answering it with the safe model would be a setting that does not do what it says.
const BEST: NoiseModel = NoiseModel::DeepFilterNet3;

/// Choose the model to run.
///
/// `standing` is the model in force, which is what makes the choice sticky. Pass `None`
/// when nothing is running yet and the answer is whatever the readings say outright.
#[must_use]
pub fn choose(quality: Quality, machine: &Machine, standing: Option<NoiseModel>) -> NoiseModel {
    match quality {
        // The two explicit choices are not policy at all, and must not be: a person who
        // picked "best" and got the cheap model because the machine was briefly busy has
        // been overruled by a setting that said it would obey.
        Quality::Best => BEST,
        Quality::Cheapest => LIGHT,
        Quality::Automatic => automatic(machine, standing),
        // Whatever is running stays running. `settle` never calls this for a manual
        // choice, and answering with a model anyway would make a caller that forgot the
        // check silently overwrite somebody's pick.
        Quality::Manual => standing.unwrap_or(LIGHT),
    }
}

fn automatic(machine: &Machine, standing: Option<NoiseModel>) -> NoiseModel {
    // A small processor never gets the heavy model, and no reading moves that: the
    // hysteresis below is about a machine that is busy, not a machine that is small.
    if machine.cores < ENOUGH_CORES {
        return LIGHT;
    }
    // Missed blocks are the one input the person can hear, so they end the question.
    if machine.recent_xruns > 0 {
        return LIGHT;
    }
    // Battery and a call each mean the machine has something better to spend on than the
    // difference between two models nobody asked to compare.
    if machine.on_battery == Some(true) || machine.in_a_call == Some(true) {
        return LIGHT;
    }
    // Several chains at once is the same argument measured a different way.
    if machine.filters > 1 {
        return LIGHT;
    }
    // What the model actually costs here, which is the only input that has been near the
    // model. An absent reading means the plugin would not load, and `loadable` below is
    // what answers that — refusing here as well would hide it.
    if machine.heavy_share.is_some_and(|share| share > TOO_DEAR) {
        return LIGHT;
    }

    let heavy_now = standing == Some(HEAVY);
    match machine.graph_load {
        // Staying heavy needs only that the graph is not crowded; becoming heavy needs
        // room to spare. That gap is the hysteresis.
        Some(load) if heavy_now => {
            if load > CROWDED {
                LIGHT
            } else {
                HEAVY
            }
        }
        Some(load) => {
            if load < ROOMY {
                HEAVY
            } else {
                LIGHT
            }
        }
        // No load reading at all. A machine with the cores, on mains, with nothing missed
        // and one chain running has said everything else it can say.
        None => HEAVY,
    }
}

impl Machine {
    /// Take the readings this process can take without watching anything.
    ///
    /// The graph's load and its missed blocks are deliberately absent: reading them means
    /// running PipeWire's Profiler, and §42.3 says to profile while a diagnosis runs
    /// rather than always. The shell's own diagnosis is where those two are measured, and
    /// [`Machine::graph_load`] exists so that a caller who has them can pass them in.
    #[must_use]
    pub fn read(filters: usize) -> Self {
        Self {
            cores: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
            on_battery: on_battery(),
            graph_load: None,
            recent_xruns: 0,
            filters,
            in_a_call: None,
            heavy_share: super::plugin_cost::audio_thread_share_cached(HEAVY),
        }
    }
}

/// Whether this machine is running off a battery.
///
/// `None` on a desktop, which has no supply of type `Mains` either — the absence of both
/// is what "this machine cannot say" looks like, and it must not read as "on battery".
fn on_battery() -> Option<bool> {
    let mut found_mains = false;
    for entry in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let kind = std::fs::read_to_string(entry.path().join("type")).unwrap_or_default();
        if kind.trim() != "Mains" {
            continue;
        }
        found_mains = true;
        if std::fs::read_to_string(entry.path().join("online"))
            .is_ok_and(|online| online.trim() == "1")
        {
            return Some(false);
        }
    }
    found_mains.then_some(true)
}

/// The model actually installed, falling back to the light one.
///
/// A policy that picks a model whose plugin is not on this machine picks silence, and the
/// caller has no way to tell that apart from a model that simply failed.
#[must_use]
pub fn loadable(model: NoiseModel) -> NoiseModel {
    if model.plugin_available() {
        model
    } else {
        LIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine with nothing wrong with it.
    fn idle() -> Machine {
        Machine {
            cores: 8,
            on_battery: Some(false),
            graph_load: Some(0.2),
            recent_xruns: 0,
            filters: 1,
            in_a_call: Some(false),
            heavy_share: Some(0.02),
        }
    }

    /// The two explicit choices are obeyed exactly, on every machine. A setting that says
    /// "best" and quietly gives the cheap model is a setting that lied.
    #[test]
    fn what_the_person_chose_outright_is_never_second_guessed() {
        let busy = Machine {
            cores: 2,
            on_battery: Some(true),
            graph_load: Some(0.99),
            recent_xruns: 40,
            filters: 4,
            heavy_share: Some(0.95),
            in_a_call: Some(true),
        };
        assert_eq!(choose(Quality::Best, &busy, None), BEST);
        assert_eq!(choose(Quality::Cheapest, &idle(), None), LIGHT);
    }

    /// The automatic choice on a machine with room is the heavy model.
    #[test]
    fn a_machine_with_room_gets_the_better_model() {
        assert_eq!(choose(Quality::Automatic, &idle(), None), HEAVY);
    }

    /// A machine where the heavy model measured too dear gets the light one, and a
    /// machine that could not be measured at all is not punished for it.
    #[test]
    fn what_the_model_costs_here_is_enough_on_its_own() {
        let dear = Machine {
            heavy_share: Some(0.80),
            ..idle()
        };
        assert_eq!(choose(Quality::Automatic, &dear, None), LIGHT);

        let unmeasured = Machine {
            heavy_share: None,
            ..idle()
        };
        assert_eq!(choose(Quality::Automatic, &unmeasured, None), HEAVY);

        let just_under = Machine {
            heavy_share: Some(0.49),
            ..idle()
        };
        assert_eq!(choose(Quality::Automatic, &just_under, None), HEAVY);
    }

    /// Each of §38's inputs on its own is enough to step down, and none of them needs a
    /// second one to agree.
    #[test]
    fn every_reason_to_step_down_works_alone() {
        for machine in [
            Machine { cores: 2, ..idle() },
            Machine {
                recent_xruns: 1,
                ..idle()
            },
            Machine {
                on_battery: Some(true),
                ..idle()
            },
            Machine {
                in_a_call: Some(true),
                ..idle()
            },
            Machine {
                filters: 2,
                ..idle()
            },
        ] {
            assert_eq!(
                choose(Quality::Automatic, &machine, Some(HEAVY)),
                LIGHT,
                "{machine:?}"
            );
        }
    }

    /// §38's hysteresis, which is the rule the whole section turns on. A graph sitting at
    /// half its period must not flip the model back and forth, because every flip reloads
    /// the chain and every reload is a gap somebody hears.
    #[test]
    fn a_graph_hovering_in_the_middle_does_not_flip_the_model() {
        let middling = Machine {
            graph_load: Some(0.45),
            ..idle()
        };
        // Already heavy: 0.45 is not crowded, so it stays.
        assert_eq!(choose(Quality::Automatic, &middling, Some(HEAVY)), HEAVY);
        // Already light: 0.45 is not roomy either, so it stays there too.
        assert_eq!(choose(Quality::Automatic, &middling, Some(LIGHT)), LIGHT);
    }

    /// And past either end of the band it does move, or the hysteresis would be a way of
    /// never changing at all.
    #[test]
    fn past_the_band_it_does_move() {
        let crowded = Machine {
            graph_load: Some(0.8),
            ..idle()
        };
        assert_eq!(choose(Quality::Automatic, &crowded, Some(HEAVY)), LIGHT);
        let roomy = Machine {
            graph_load: Some(0.1),
            ..idle()
        };
        assert_eq!(choose(Quality::Automatic, &roomy, Some(LIGHT)), HEAVY);
    }

    /// A reading this machine cannot take is not a bad reading. Treating an absent
    /// battery as "on battery" would put every desktop into economy mode for ever.
    #[test]
    fn a_reading_that_is_not_available_is_not_counted_against_the_machine() {
        let unknown = Machine {
            on_battery: None,
            in_a_call: None,
            graph_load: None,
            ..idle()
        };
        assert_eq!(choose(Quality::Automatic, &unknown, None), HEAVY);
    }

    /// The same readings give the same answer every time — §38 asks for a reproducible
    /// policy, and this is what that means in a test.
    #[test]
    fn the_same_readings_always_give_the_same_answer() {
        let machine = idle();
        let once = choose(Quality::Automatic, &machine, None);
        for _ in 0..100 {
            assert_eq!(choose(Quality::Automatic, &machine, None), once);
        }
    }

    /// The word in the file and the word on the command line are the same word. They
    /// were not: `rename_all` wrote `automatic` while the command took `auto`, and the
    /// program reading that file is a different one.
    #[test]
    fn the_file_and_the_command_line_use_one_spelling() {
        for quality in [
            Quality::Automatic,
            Quality::Best,
            Quality::Cheapest,
            Quality::Manual,
        ] {
            let written = serde_json::to_string(&quality).expect("serialises");
            assert_eq!(written, format!("\"{}\"", quality.word()));
            let back: Quality = serde_json::from_str(&written).expect("reads back");
            assert_eq!(back, quality);
        }
    }

    /// The words survive a round trip, in both the spellings a person might type.
    #[test]
    fn the_words_round_trip() {
        for quality in [Quality::Automatic, Quality::Best, Quality::Cheapest] {
            assert_eq!(Quality::parse(quality.word()), Some(quality));
        }
        assert_eq!(Quality::parse("economy"), Some(Quality::Cheapest));
        assert_eq!(Quality::parse("quality"), Some(Quality::Best));
        assert_eq!(Quality::parse("louder"), None);
    }
}
