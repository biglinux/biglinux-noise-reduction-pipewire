//! Get a microphone back when the session is left without one.
//!
//! WirePlumber restores the profile a card carried last time. When the device no
//! longer offers that profile — a webcam whose analog input mapping is gone after
//! a reboot, a USB interface that enumerated differently — the restore fails and
//! the card stays `off`. If that was the only capture card, the graph then has no
//! `Audio/Source` at all: the picker is empty, our filter chain has nothing to
//! filter, and PipeWire hands applications a sink monitor as the default source,
//! so people "record" their own speakers.
//!
//! The hardware never went anywhere; only the profile choice is stale. So the
//! recovery is: pick the best profile still on offer that publishes a capture
//! source, activate it, and point the session default at the source it creates.

use std::io;
use std::thread::sleep;
use std::time::{Duration, Instant};

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use log::{info, warn};
use serde_json::Value;

use super::sources::{Source, default_source_name, list_sources, set_default_source};

const PACTL: &str = "/usr/bin/pactl";
/// A profile change republishes the device's nodes asynchronously.
const NODE_WAIT: Duration = Duration::from_secs(3);
const NODE_POLL: Duration = Duration::from_millis(250);
/// Rounds of "say it again and check" while WirePlumber settles its own defaults.
const DEFAULT_ROUNDS: usize = 3;
const DEFAULT_SETTLE: Duration = Duration::from_millis(400);

/// A profile that is still on offer and would publish at least one capture source.
#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    card: String,
    profile: String,
    /// Whether anything is actually plugged into the way in. Measured here: a
    /// desktop codec with an empty microphone jack still offers an "available"
    /// duplex profile, and activating it publishes a source that records silence
    /// — which then became the system default over a real USB microphone.
    port_backed: bool,
    /// Keeping whatever the card currently plays outranks the profile's own
    /// priority: a microphone must never be recovered by silencing the speakers
    /// on the same card, which on an internal codec is every speaker there is.
    keeps_sinks: bool,
    priority: i64,
}

/// Make sure the session has a default microphone; name the device when this
/// call is what put one there.
///
/// Returns `None` both when nothing was wrong and when nothing could be done —
/// the caller's next step is the same either way, and both are already logged.
pub fn recover_capture() -> Option<String> {
    let mut sources = list_sources().unwrap_or_default();
    if sources.is_empty() {
        let candidate = best_candidate()?;
        info!(
            "cards: no capture source in the graph; activating {} on {}",
            candidate.profile, candidate.card
        );
        if let Err(error) = set_profile(&candidate.card, &candidate.profile) {
            warn!("cards: {error}");
            return None;
        }
        sources = await_sources();
        if sources.is_empty() {
            warn!("cards: {} published no capture source", candidate.profile);
            return None;
        }
    } else if default_source_name()
        .is_some_and(|name| sources.iter().any(|source| source.node_name == name))
    {
        return None;
    }
    // Either the profile switch just created this source, or the default was
    // pointing at something that is not a microphone (typically a sink monitor).
    let source = sources.first()?;
    if !pin_default(source) {
        return None;
    }
    info!("cards: default capture → {}", source.node_name);
    Some(source.description.clone())
}

/// Make the choice stick.
///
/// A card profile change makes WirePlumber re-evaluate its own defaults, and
/// that evaluation can land after ours and overwrite it — measured here, with a
/// sink's name left sitting under `default.audio.source`. So say it again until
/// the graph agrees, and report the failure rather than a success nobody got.
fn pin_default(source: &Source) -> bool {
    for _ in 0..DEFAULT_ROUNDS {
        if let Err(error) = set_default_source(source.node_id) {
            warn!("cards: {error}");
            return false;
        }
        sleep(DEFAULT_SETTLE);
        if default_source_name().as_deref() == Some(source.node_name.as_str()) {
            return true;
        }
    }
    warn!(
        "cards: {} did not stay the default source",
        source.node_name
    );
    false
}

fn await_sources() -> Vec<Source> {
    let deadline = Instant::now() + NODE_WAIT;
    loop {
        let sources = list_sources().unwrap_or_default();
        if !sources.is_empty() || Instant::now() >= deadline {
            return sources;
        }
        sleep(NODE_POLL);
    }
}

fn best_candidate() -> Option<Candidate> {
    let cards = match cards_json() {
        Ok(cards) => cards,
        Err(error) => {
            warn!("cards: {error}");
            return None;
        }
    };
    cards.iter().filter_map(card_candidate).max_by_key(rank)
}

/// The best capture profile this one card could switch to, if any.
fn card_candidate(card: &Value) -> Option<Candidate> {
    let name = card["name"].as_str()?;
    let profiles = card["profiles"].as_object()?;
    let port_backed = input_port_backed(card["ports"].as_object());
    let active_sinks = card["active_profile"]
        .as_str()
        .and_then(|active| profiles.get(active))
        .and_then(|profile| profile["sinks"].as_i64())
        .unwrap_or(0);
    profiles
        .iter()
        .filter(|(_, profile)| {
            profile["available"].as_bool() == Some(true)
                && profile["sources"].as_i64().unwrap_or(0) > 0
        })
        .map(|(profile_name, profile)| Candidate {
            card: name.to_owned(),
            profile: profile_name.clone(),
            port_backed,
            keeps_sinks: profile["sinks"].as_i64().unwrap_or(0) >= active_sinks,
            priority: profile["priority"].as_i64().unwrap_or(0),
        })
        .max_by_key(rank)
}

fn rank(candidate: &Candidate) -> (bool, bool, i64) {
    (
        candidate.port_backed,
        candidate.keeps_sinks,
        candidate.priority,
    )
}

/// Is anything plugged into this card's way in?
///
/// The question belongs to the card, not to one profile: `pro-audio` lists no
/// ports at all, so asking per profile let a desktop codec with three empty
/// jacks pass as a microphone — measured here, it then beat a connected USB
/// microphone and published silence as the system default.
///
/// "Availability unknown" is what a USB capsule reports about itself, and a card
/// with no ports (a webcam) says nothing either; neither is evidence against the
/// card. Only a card whose every capture port explicitly says "not available" is
/// a dead end, and it still ranks below anything else rather than being
/// discarded: silence beats the sink monitor PipeWire hands out when a machine
/// has no source at all.
fn input_port_backed(ports: Option<&serde_json::Map<String, Value>>) -> bool {
    let Some(ports) = ports else {
        return true;
    };
    let mut saw_input = false;
    for (name, port) in ports {
        if !(name.contains("input") || port["type"] == "Mic") {
            continue;
        }
        saw_input = true;
        if port["availability"] != "not available" {
            return true;
        }
    }
    !saw_input
}

fn cards_json() -> io::Result<Vec<Value>> {
    let output = BigSubprocessSpec::builder()
        .program(PACTL)
        .args(["-f", "json", "list", "cards"])
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list([PACTL])
        .build()
        .run()
        .map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "pactl list cards exited with {:?}",
            output.status.code()
        )));
    }
    serde_json::from_str(&output.stdout_lossy()).map_err(io::Error::other)
}

fn set_profile(card: &str, profile: &str) -> io::Result<()> {
    let output = BigSubprocessSpec::builder()
        .program(PACTL)
        .args(["set-card-profile", card, profile])
        .stdout(BigSubprocessOutputMode::Null)
        .allow_list([PACTL])
        .build()
        .run()
        .map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "pactl set-card-profile {card} {profile} exited with {:?}",
            output.status.code()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The webcam that started this: its saved `input:analog-stereo` is gone and
    /// only `off` and `pro-audio` remain, so WirePlumber left the card silent.
    fn webcam() -> Value {
        serde_json::json!({
            "name": "alsa_card.usb-Webcam",
            "active_profile": "off",
            "profiles": {
                "off": {"sinks": 0, "sources": 0, "priority": 0, "available": true},
                "pro-audio": {"sinks": 0, "sources": 1, "priority": 1, "available": true},
            }
        })
    }

    #[test]
    fn a_card_left_off_offers_the_profile_that_still_has_an_input() {
        let candidate = card_candidate(&webcam()).unwrap();
        assert_eq!(candidate.profile, "pro-audio");
        assert!(candidate.keeps_sinks);
    }

    #[test]
    fn recovering_a_microphone_never_silences_the_speakers_on_that_card() {
        let codec = serde_json::json!({
            "name": "alsa_card.pci-codec",
            "active_profile": "output:analog-stereo",
            "profiles": {
                "output:analog-stereo": {"sinks": 1, "sources": 0, "priority": 6500, "available": true},
                // Higher priority, but it would take the only speakers away.
                "input:analog-stereo": {"sinks": 0, "sources": 1, "priority": 9000, "available": true},
                "output:analog-stereo+input:analog-stereo":
                    {"sinks": 1, "sources": 1, "priority": 6565, "available": true},
            }
        });
        let candidate = card_candidate(&codec).unwrap();
        assert_eq!(
            candidate.profile,
            "output:analog-stereo+input:analog-stereo"
        );
    }

    #[test]
    fn an_unavailable_profile_is_not_a_microphone() {
        let unplugged = serde_json::json!({
            "name": "alsa_card.pci-codec",
            "active_profile": "output:analog-stereo",
            "profiles": {
                "output:analog-stereo": {"sinks": 1, "sources": 0, "priority": 6500, "available": true},
                "input:analog-stereo": {"sinks": 0, "sources": 1, "priority": 65, "available": false},
            }
        });
        assert_eq!(card_candidate(&unplugged), None);
    }

    #[test]
    fn the_card_with_the_best_capture_profile_wins() {
        let cards = [
            webcam(),
            serde_json::json!({
                "name": "alsa_card.usb-Studio",
                "active_profile": "off",
                "profiles": {
                    "off": {"sinks": 0, "sources": 0, "priority": 0, "available": true},
                    "input:analog-stereo": {"sinks": 0, "sources": 1, "priority": 65, "available": true},
                }
            }),
        ];
        let best = cards
            .iter()
            .filter_map(card_candidate)
            .max_by_key(rank)
            .unwrap();
        assert_eq!(best.card, "alsa_card.usb-Studio");
    }

    /// Both cards as this machine reports them. The codec's duplex profile is
    /// "available" with an empty microphone jack, outranks everything on
    /// priority, and was picked over a connected USB microphone — publishing a
    /// source that records silence and making it the system default.
    #[test]
    fn an_empty_microphone_jack_loses_to_a_microphone_that_is_plugged_in() {
        let codec = serde_json::json!({
            "name": "alsa_card.pci-0000_00_1f.3",
            "active_profile": "output:analog-stereo",
            "profiles": {
                "output:analog-stereo+input:analog-stereo":
                    {"sinks": 1, "sources": 1, "priority": 6565, "available": true},
                // Lists no ports of its own, so the emptiness of the jacks has to
                // be read from the card or this profile walks straight past it.
                "pro-audio": {"sinks": 1, "sources": 2, "priority": 1, "available": true},
            },
            "ports": {
                "analog-input-front-mic": {"type": "Mic", "availability": "not available",
                    "profiles": ["output:analog-stereo+input:analog-stereo"]},
                "analog-input-rear-mic": {"type": "Mic", "availability": "not available",
                    "profiles": ["output:analog-stereo+input:analog-stereo"]},
                "analog-output-headphones": {"type": "Headphones", "availability": "available",
                    "profiles": ["output:analog-stereo+input:analog-stereo"]},
            }
        });
        let usb = serde_json::json!({
            "name": "alsa_card.usb-AKG",
            "active_profile": "output:analog-stereo",
            "profiles": {
                "input:analog-stereo": {"sinks": 0, "sources": 1, "priority": 65, "available": true},
            },
            // What a captive USB capsule reports about itself.
            "ports": {
                "analog-input-mic": {"type": "Mic", "availability": "availability unknown",
                    "profiles": ["input:analog-stereo"]},
            }
        });
        let best = [codec.clone(), usb]
            .iter()
            .filter_map(card_candidate)
            .max_by_key(rank)
            .unwrap();
        assert_eq!(best.card, "alsa_card.usb-AKG");
        // Still better than nothing when it is all there is: PipeWire's fallback
        // for a machine with no source is a sink monitor, i.e. the speakers.
        assert!(card_candidate(&codec).is_some());
    }
}
