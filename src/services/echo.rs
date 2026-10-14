//! Resolve automatic echo cancellation from the current PipeWire output route.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{EchoCancelConfig, EchoMode};
use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use serde_json::Value;

pub fn settle(config: &mut EchoCancelConfig) {
    match config.mode {
        EchoMode::Always => config.enabled = true,
        EchoMode::Never => config.enabled = false,
        EchoMode::Automatic => {
            let Ok(output) = BigSubprocessSpec::builder()
                .program("/usr/bin/pw-dump")
                .stderr(BigSubprocessOutputMode::Null)
                .allow_list(["/usr/bin/pw-dump"])
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .run()
            else {
                return;
            };
            if output.status.success()
                && let Ok(graph) = serde_json::from_slice::<Vec<Value>>(&output.stdout)
                && let Some(wanted) = from_graph(&graph)
            {
                config.enabled = wanted;
            }
        }
    }
}

/// What a detected output-route change means for a *running* canceller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteAction {
    /// The route flips whether AEC is wanted at all: reconcile toggles it.
    Toggle,
    /// AEC stays on but the acoustic path changed, so its coarse delay is now
    /// locked to the old route. Restart the AEC so it re-estimates the delay
    /// for the new path (review AEC-02).
    RepriimeAec,
    /// AEC stays off, or is not in play: nothing to do.
    Nothing,
}

/// Decide what a default-sink change requires of the canceller.
///
/// `enabled` is the current persisted AEC state; `wanted` is whether the *new*
/// route implies AEC (from [`RouteWatch`]). The old watcher only reacted when
/// the route flipped `enabled`, so switching between two sinks while AEC stayed
/// on left the coarse delay pinned to the previous speaker path — no echo was
/// cancelled on the new one (review AEC-02).
#[must_use]
pub fn route_action(mode: EchoMode, enabled: bool, wanted: bool) -> RouteAction {
    match mode {
        EchoMode::Automatic if enabled != wanted => RouteAction::Toggle,
        EchoMode::Automatic if wanted => RouteAction::RepriimeAec,
        EchoMode::Always => RouteAction::RepriimeAec,
        EchoMode::Never | EchoMode::Automatic => RouteAction::Nothing,
    }
}

/// Incremental view of the graph as `pw-dump --monitor` reports it: the first
/// batch is every object, later ones carry only what changed, and
/// `{"id":N,"info":null}` means the object went away.
///
/// This exists so the watcher does not have to ask for the graph. Running
/// `pw-dump` per monitor event made it feed itself — one dump connects a
/// client, which the monitor reports three times (add, update, remove), and
/// each of those spawned another dump.
#[derive(Default)]
pub struct RouteWatch {
    objects: BTreeMap<u64, Value>,
    last: Option<OutputRoute>,
}

impl RouteWatch {
    /// Report changes of the acoustic route, not merely changes of the AEC
    /// on/off decision. Two different loudspeakers can both require AEC.
    /// Unrelated events and volume-only updates must not restart the engine.
    pub fn absorb(&mut self, batch: Vec<Value>) -> Option<bool> {
        for object in batch {
            let Some(id) = object["id"].as_u64() else {
                continue;
            };
            // A removal is `info` *present and null*. Testing whether it
            // resolves to null instead drops every object that has no `info`
            // at all — which is all seven `Metadata` objects on a live graph,
            // and `default.audio.sink` is one of their keys.
            if object.get("info").is_some_and(Value::is_null) {
                self.objects.remove(&id);
            } else if object.get("metadata").is_some() {
                // A `Metadata` monitor event carries only the entries that
                // changed, not the whole set (verified against `pw-dump
                // --monitor`: a `default.audio.sink` write arrives as a
                // one-entry event on the same object that also holds
                // `default.configured.audio.sink`). Replacing the object would
                // drop the keys that did not change, so a later unrelated write
                // erases the cached `default.audio.sink`. Merge by
                // (subject, key); a null value deletes just that entry.
                match self.objects.get_mut(&id) {
                    Some(existing) if existing.get("metadata").is_some() => {
                        merge_metadata(existing, &object);
                    }
                    _ => {
                        self.objects.insert(id, object);
                    }
                }
            } else {
                self.objects.insert(id, object);
            }
        }
        let Some(route) = output_route(self.objects.values()) else {
            // A disappeared device followed by the same name is a new path too.
            self.last = None;
            return None;
        };
        let changed = self.last.as_ref() != Some(&route);
        let wanted = route.wanted;
        self.last = Some(route);
        changed.then_some(wanted)
    }
}

/// Fold a partial `Metadata` monitor event into the cached object: replace or
/// insert each incoming `(subject, key)` entry, and drop the ones whose value
/// arrived as null. `serde_json::Value` is not `Hash`, so this matches entries
/// with a linear scan — a metadata object holds only a handful of keys.
fn merge_metadata(existing: &mut Value, incoming: &Value) {
    let Some(updates) = incoming["metadata"].as_array() else {
        return;
    };
    let ident = |entry: &Value| (entry["subject"].clone(), entry["key"].clone());
    // pw-dump 1.6.8 can emit [] after deleting a key, without a tombstone.
    // Invalidate rather than retaining a deleted default indefinitely. We cannot
    // recover unreported keys from this lossy protocol; next writes repopulate.
    let mut merged = if updates.is_empty() {
        Vec::new()
    } else {
        existing["metadata"].as_array().cloned().unwrap_or_default()
    };
    for update in updates {
        if update.get("key").is_some_and(Value::is_null) {
            merged.retain(|entry| entry["subject"] != update["subject"]);
            continue;
        }
        let key = ident(update);
        merged.retain(|entry| ident(entry) != key);
        if !update.get("value").is_some_and(Value::is_null) {
            merged.push(update.clone());
        }
    }
    existing["metadata"] = Value::Array(merged);
    for key in ["props", "type"] {
        if let Some(value) = incoming.get(key) {
            existing[key] = value.clone();
        }
    }
}

/// `graph` is iterated more than once, hence the `Clone` bound — it is
/// satisfied by a slice iterator and by `BTreeMap::values`, so neither caller
/// has to copy the graph to ask this question.
fn from_graph<'a>(graph: impl IntoIterator<Item = &'a Value> + Clone) -> Option<bool> {
    output_route(graph).map(|route| route.wanted)
}

#[derive(Debug, PartialEq)]
struct OutputRoute {
    wanted: bool,
    // Only path identity/timing, deliberately not state, volume, mute or links
    // created by our own service restart (which would cause a restart loop).
    identity: Value,
}

fn numeric(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

pub(crate) fn default_name(graph: &[&Value], key: &str) -> Option<String> {
    let mut metadata = graph.iter().filter(|o| {
        o["type"] == "PipeWire:Interface:Metadata" && o["props"]["metadata.name"] == "default"
    });
    let object = metadata.next()?;
    if metadata.next().is_some() {
        return None;
    }
    let mut entries = object["metadata"]
        .as_array()?
        .iter()
        .filter(|e| e["key"] == key && numeric(&e["subject"]) == Some(0));
    let value = &entries.next()?["value"];
    if entries.next().is_some() {
        return None;
    }
    let value = match value.as_str() {
        Some(text) => serde_json::from_str::<Value>(text).ok()?,
        None => value.clone(),
    };
    value
        .get("name")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn named_node<'a>(graph: &[&'a Value], name: &str) -> Option<&'a Value> {
    let mut nodes = graph.iter().copied().filter(|o| {
        o["type"] == "PipeWire:Interface:Node"
            && o["info"]["props"]["node.name"].as_str() == Some(name)
    });
    let node = nodes.next()?;
    nodes.next().is_none().then_some(node)
}

// A Route.device is the profile's device index, NOT the PipeWire global device.id.
// Never pick the first of several output/input routes belonging to other PCMs.
fn endpoint(graph: &[&Value], node: &Value, input: bool) -> OutputRoute {
    let props = &node["info"]["props"];
    let id = numeric(&props["device.id"]);
    let device = id.and_then(|id| {
        graph
            .iter()
            .copied()
            .find(|o| numeric(&o["id"]) == Some(id))
    });
    let form = device
        .and_then(|d| d["info"]["props"]["device.form-factor"].as_str())
        .or_else(|| props["device.form-factor"].as_str());
    let profile_device =
        numeric(&props["card.profile.device"]).or_else(|| numeric(&props["device.profile.device"]));
    let routes = device.and_then(|d| d["info"]["params"]["Route"].as_array());
    let mut matching = routes.into_iter().flatten().filter(|r| {
        let direction = u64::from(!input);
        (r["direction"] == if input { "Input" } else { "Output" }
            || numeric(&r["direction"]) == Some(direction))
            && profile_device.is_none_or(|index| numeric(&r["device"]) == Some(index))
    });
    let first = matching.next();
    let route = if matching.next().is_none() {
        first
    } else {
        None
    };
    let port = route
        .and_then(|r| r["name"].as_str())
        .or_else(|| props["port.name"].as_str());
    let profile = device.and_then(|d| {
        d["info"]["params"]["Profile"]
            .as_array()?
            .first()?
            .get("index")
    });
    OutputRoute {
        wanted: microphone_can_hear_it(form, port),
        identity: serde_json::json!([
            props["node.name"],
            node["id"],
            props["object.serial"],
            id,
            profile_device,
            port,
            route.map(|r| &r["index"]),
            profile,
            props["node.latency"],
            props["api.alsa.headroom"],
            props["api.alsa.period-size"]
        ]),
    }
}

fn real_source(node: &Value) -> bool {
    let p = &node["info"]["props"];
    let name = p["node.name"].as_str().unwrap_or("");
    node["type"] == "PipeWire:Interface:Node"
        && matches!(
            p["media.class"].as_str(),
            Some("Audio/Source" | "Audio/Duplex")
        )
        && !name.is_empty()
        && !name.ends_with(".monitor")
        && !matches!(name, "mic-biglinux" | "echo-cancel-source")
        && p["node.virtual"] != true
        && p["node.virtual"] != "true"
        && p["node.link-group"].is_null()
}

fn selected_source<'a>(graph: &[&'a Value]) -> Option<&'a Value> {
    for key in ["default.configured.audio.source", "default.audio.source"] {
        if let Some(node) = default_name(graph, key).and_then(|name| named_node(graph, &name))
            && real_source(node)
        {
            return Some(node);
        }
    }
    // Same priority order as the packaged Lua selector; stable name breaks ties.
    graph
        .iter()
        .copied()
        .filter(|node| real_source(node))
        .max_by_key(|node| {
            let p = &node["info"]["props"];
            (
                numeric(&p["priority.session"])
                    .or_else(|| numeric(&p["priority.driver"]))
                    .or_else(|| numeric(&node["id"]))
                    .unwrap_or(0),
                p["node.name"].as_str().unwrap_or(""),
            )
        })
}

fn output_route<'a>(graph: impl IntoIterator<Item = &'a Value> + Clone) -> Option<OutputRoute> {
    let graph: Vec<&Value> = graph.into_iter().collect();
    let name = default_name(&graph, "default.audio.sink")?;
    let start = named_node(&graph, &name)?;
    let mut queue = vec![numeric(&start["id"])?];
    let mut seen = BTreeSet::new();
    let mut physical = BTreeMap::new();
    let mut incomplete = false;
    // Follow playback links and the paired stream of a filter-chain link-group.
    // Only terminal device identity is retained, not ephemeral links from our
    // own restarts. Stop at a hardware sink before considering monitor outputs.
    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        if seen.len() > 64 {
            incomplete = true;
            break;
        } // never infer headphones from a truncated graph
        let Some(node) = graph
            .iter()
            .copied()
            .find(|n| numeric(&n["id"]) == Some(id))
        else {
            incomplete = true;
            continue;
        };
        let p = &node["info"]["props"];
        if numeric(&p["device.id"]).is_some()
            && matches!(
                p["media.class"].as_str(),
                Some("Audio/Sink" | "Audio/Duplex") | None
            )
        {
            physical.insert(id, endpoint(&graph, node, false));
            continue;
        }
        if let Some(group) = p["node.link-group"].as_str() {
            for sibling in &graph {
                let sp = &sibling["info"]["props"];
                if sp["node.link-group"].as_str() == Some(group)
                    && sp["media.class"] == "Stream/Output/Audio"
                    && let Some(other) = numeric(&sibling["id"])
                {
                    queue.push(other);
                }
            }
        }
        for link in &graph {
            if link["type"] == "PipeWire:Interface:Link"
                && numeric(&link["info"]["output-node-id"]) == Some(id)
                && matches!(link["info"]["state"].as_str(), Some("active" | "paused"))
                && let Some(other) = numeric(&link["info"]["input-node-id"])
            {
                queue.push(other);
            }
        }
    }
    // A disconnected virtual sink can still have a configured hardware target.
    if physical.is_empty() {
        let configured = default_name(&graph, "default.configured.audio.sink")
            .and_then(|n| named_node(&graph, &n));
        if let Some(node) =
            configured.filter(|n| numeric(&n["info"]["props"]["device.id"]).is_some())
        {
            physical.insert(numeric(&node["id"])?, endpoint(&graph, node, false));
        } else {
            // Unknown routing stays conservatively enabled. Do not encode the
            // ephemeral ID of our own virtual sink into the restart decision.
            incomplete = true;
        }
    }
    let wanted = incomplete || physical.values().any(|r| r.wanted);
    let output: Vec<_> = physical.values().map(|r| &r.identity).collect();
    let input = selected_source(&graph).map(|n| endpoint(&graph, n, true).identity);
    Some(OutputRoute {
        wanted,
        identity: serde_json::json!([name, output, input, incomplete]),
    })
}

/// Whether the current output is acoustically coupled to the microphone.
///
/// True means the speaker can bleed back into the mic, so automatic mode runs
/// the canceller; false is a private path — headphones or a headset — where it
/// stays off. Two signals feed the decision and both are read, never the first
/// present:
///
/// - the device form-factor, which a Bluetooth or USB device sets to `headset`
///   / `headphone` / `speaker`; and
/// - the active output route, which is the only thing that separates the
///   headphone jack from the speaker on a built-in card whose form-factor is
///   the useless `internal`.
///
/// A built-in line-out jack has no detection at all — it reads `available`
/// forever and names no device — so it is left as coupled: automatic cannot
/// tell headphones from powered speakers there, and `Always` / `Never` are the
/// manual answers for that jack.
fn microphone_can_hear_it(form_factor: Option<&str>, port: Option<&str>) -> bool {
    let private = |value: &str| {
        let value = value.to_ascii_lowercase();
        value.contains("headphone") || value.contains("headset")
    };
    !(form_factor.is_some_and(private) || port.is_some_and(private))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn speakers_headphones_and_explicit_modes() {
        assert!(microphone_can_hear_it(None, None));
        assert!(microphone_can_hear_it(Some("speaker"), None));
        assert!(microphone_can_hear_it(None, Some("analog-output-speaker")));
        // A Bluetooth or USB headset names itself; the form-factor alone is
        // enough to keep the canceller off.
        assert!(!microphone_can_hear_it(Some("headset"), None));
        assert!(!microphone_can_hear_it(Some("headphone"), None));
        assert!(!microphone_can_hear_it(
            None,
            Some("analog-output-headphones")
        ));
        // A built-in card reports the useless `internal`, so the headphone jack
        // is only visible through the route: both signals are read, and the
        // form-factor must not short-circuit the route.
        assert!(!microphone_can_hear_it(
            Some("internal"),
            Some("analog-output-headphones")
        ));
        // The line-out jack has no detection and stays coupled — automatic
        // cannot tell headphones from powered speakers there.
        assert!(microphone_can_hear_it(None, Some("analog-output-lineout")));
        assert!(microphone_can_hear_it(
            Some("internal"),
            Some("analog-output-lineout")
        ));
        // HDMI names a display's speakers, which share the room with the mic.
        assert!(microphone_can_hear_it(None, Some("hdmi-output-0")));
        let mut config = EchoCancelConfig {
            enabled: false,
            mode: EchoMode::Always,
            ..Default::default()
        };
        settle(&mut config);
        assert!(config.enabled);
        config.mode = EchoMode::Never;
        settle(&mut config);
        assert!(!config.enabled);
    }
    #[test]
    fn default_metadata_resolves_active_hardware_route() {
        let mut graph = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[{"subject":0,"key":"default.audio.sink","value":{"name":"test-sink"}}]},
            {"id":2,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"test-sink","device.id":3}}},
            {"id":3,"info":{"params":{"Route":[{"direction":"Output","name":"analog-output-headphones"}]}}}
        ]);
        assert_eq!(from_graph(graph.as_array().unwrap()), Some(false));
        graph[2]["info"]["params"]["Route"][0]["name"] = "analog-output-speaker".into();
        assert_eq!(from_graph(graph.as_array().unwrap()), Some(true));
        assert_eq!(from_graph(&[]), None);
    }

    #[test]
    fn a_bluetooth_headset_is_read_from_the_device_not_the_silent_node() {
        // The sink node carries no form-factor and no headphone route — exactly
        // what a Bluetooth A2DP sink looks like. The device is where `headset`
        // lives, and reading it there is what keeps the canceller off.
        let graph = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[{"subject":0,"key":"default.audio.sink","value":{"name":"bt-sink"}}]},
            {"id":2,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"bt-sink","device.id":3}}},
            {"id":3,"info":{"props":{"device.form-factor":"headset"},"params":{}}}
        ]);
        assert_eq!(from_graph(graph.as_array().unwrap()), Some(false));
    }

    #[test]
    fn route_action_reprimes_a_still_active_aec_on_a_route_change() {
        use super::{RouteAction, route_action};
        // Automatic, route now wants AEC and it was already on → re-prime for
        // the new acoustic path instead of doing nothing.
        assert_eq!(
            route_action(EchoMode::Automatic, true, true),
            RouteAction::RepriimeAec
        );
        // Automatic flip (route no longer wants AEC) → toggle via reconcile.
        assert_eq!(
            route_action(EchoMode::Automatic, true, false),
            RouteAction::Toggle
        );
        assert_eq!(
            route_action(EchoMode::Automatic, false, true),
            RouteAction::Toggle
        );
        // Automatic, stays off → nothing.
        assert_eq!(
            route_action(EchoMode::Automatic, false, false),
            RouteAction::Nothing
        );
        // Always on → every route change re-primes, regardless of `wanted`.
        assert_eq!(
            route_action(EchoMode::Always, true, false),
            RouteAction::RepriimeAec
        );
        // Never → never touch it.
        assert_eq!(
            route_action(EchoMode::Never, false, true),
            RouteAction::Nothing
        );
    }

    #[test]
    fn route_watch_folds_deltas_and_only_speaks_up_on_a_change() {
        let mut watch = RouteWatch::default();
        // Batch 0 is the whole graph, as `pw-dump --monitor` sends it.
        let full = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[{"subject":0,"key":"default.audio.sink","value":{"name":"test-sink"}}]},
            {"id":2,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"test-sink","device.id":3}}},
            {"id":3,"info":{"params":{"Route":[
                {"direction":"Output","name":"analog-output-speaker"}]}}}
        ]);
        assert_eq!(
            watch.absorb(full.as_array().unwrap().clone()),
            Some(true),
            "the first decision is always new"
        );

        // An unrelated object appearing must not re-report the same answer,
        // otherwise the watcher does work on every event in the session.
        let unrelated = serde_json::json!([{"id":249,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"pw-dump"}}}]);
        assert_eq!(watch.absorb(unrelated.as_array().unwrap().clone()), None);

        // Objects with no `info` key at all are ordinary graph members, not
        // removals — the metadata that names the default sink is one of them.
        let metadata_update = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[{"subject":0,"key":"default.audio.sink","value":{"name":"test-sink"}}]}
        ]);
        assert_eq!(
            watch.absorb(metadata_update.as_array().unwrap().clone()),
            None,
            "re-reporting the same sink must not drop it from the view"
        );

        // Headphones plugged in: one delta on the device flips the answer.
        let delta = serde_json::json!([
            {"id":3,"info":{"params":{"Route":[
                {"direction":"Output","name":"analog-output-headphones"}]}}}
        ]);
        assert_eq!(watch.absorb(delta.as_array().unwrap().clone()), Some(false));

        // `info: null` is how the monitor reports a removal; dropping the
        // sink node leaves nothing to decide from.
        let removed = serde_json::json!([{"id":2,"info":null}]);
        assert_eq!(watch.absorb(removed.as_array().unwrap().clone()), None);
    }

    #[test]
    fn metadata_deltas_merge_instead_of_replacing_the_object() {
        // Reproduces the live `pw-dump --monitor` shape: one Metadata object
        // (id 1) holds several keys, and updates arrive one key at a time.
        let mut watch = RouteWatch::default();
        let full = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[
                {"subject":0,"key":"default.configured.audio.sink","value":{"name":"x"}},
                {"subject":0,"key":"default.audio.sink","value":{"name":"test-sink"}}]},
            {"id":2,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"test-sink","device.id":3}}},
            {"id":3,"info":{"params":{"Route":[
                {"direction":"Output","name":"analog-output-speaker"}]}}}
        ]);
        assert_eq!(watch.absorb(full.as_array().unwrap().clone()), Some(true));

        // A one-entry event for an UNRELATED key on the same object. Replacing
        // the object would drop default.audio.sink; merging keeps it, so the
        // folded view still resolves the route.
        let unrelated_key = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[
                {"subject":0,"key":"default.configured.audio.sink","value":{"name":"z"}}]}
        ]);
        watch.absorb(unrelated_key.as_array().unwrap().clone());
        assert_eq!(
            from_graph(watch.objects.values()),
            Some(true),
            "default.audio.sink must survive an unrelated metadata delta"
        );

        // A null value deletes only that entry; default.audio.sink stays.
        let delete_key = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[
                {"subject":0,"key":"default.configured.audio.sink","value":null}]}
        ]);
        watch.absorb(delete_key.as_array().unwrap().clone());
        assert_eq!(from_graph(watch.objects.values()), Some(true));
        let entries = watch.objects[&1]["metadata"].as_array().unwrap();
        assert!(
            entries
                .iter()
                .all(|e| e["key"] != "default.configured.audio.sink")
        );
        assert!(entries.iter().any(|e| e["key"] == "default.audio.sink"));
    }
}

#[cfg(test)]
mod route_identity_regressions {
    use super::*;
    use serde_json::json;

    fn initial() -> Vec<Value> {
        vec![
            json!({"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[{"subject":0,"key":"default.audio.sink","value":{"name":"speaker-a"}}]}),
            json!({"id":2,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"speaker-a","object.serial":22,"device.id":3}}}),
            json!({"id":3,"info":{"params":{"Route":[{"direction":"Output","name":"analog-output-speaker"}]}}}),
            json!({"id":4,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"speaker-b","object.serial":44,"device.id":5}}}),
            json!({"id":5,"info":{"params":{"Route":[{"direction":"Output","name":"hdmi-output-0"}]}}}),
        ]
    }

    #[test]
    fn speaker_to_speaker_reaches_the_reprime_decision() {
        let mut watch = RouteWatch::default();
        assert_eq!(watch.absorb(initial()), Some(true));
        let event = watch.absorb(vec![json!({"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[
            {"subject":0,"key":"default.audio.sink","value":{"name":"speaker-b"}}
        ]})]);
        assert_eq!(
            event,
            Some(true),
            "AEC stays enabled but its reference path changed"
        );
        assert_eq!(
            route_action(EchoMode::Automatic, true, event.unwrap()),
            RouteAction::RepriimeAec
        );
        assert_eq!(watch.absorb(vec![]), None);
    }

    #[test]
    fn same_node_new_port_or_new_generation_is_a_route_change() {
        let mut watch = RouteWatch::default();
        watch.absorb(initial());
        assert_eq!(
            watch.absorb(vec![json!({"id":3,"info":{"params":{"Route":[
                {"direction":"Output","name":"analog-output-lineout"}
            ]}}})]),
            Some(true)
        );
        assert_eq!(
            watch.absorb(vec![
                json!({"id":2,"type":"PipeWire:Interface:Node","info":{"props":{
                    "node.name":"speaker-a","object.serial":222,"device.id":3
                }}})
            ]),
            Some(true)
        );
    }

    #[test]
    fn volume_updates_do_not_form_a_restart_feedback_loop() {
        let mut watch = RouteWatch::default();
        watch.absorb(initial());
        assert_eq!(
            watch.absorb(vec![json!({"id":3,"info":{"params":{"Route":[
                {"direction":"Output","name":"analog-output-speaker","props":{"volume":0.25}}
            ]}}})]),
            None
        );
    }

    #[test]
    fn invalid_default_name_cannot_match_an_unrelated_null_property() {
        for value in [
            json!(false),
            json!(null),
            json!({"name":23}),
            json!({"name":""}),
        ] {
            let graph = vec![
                json!({"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[{"subject":0,"key":"default.audio.sink","value":value}]}),
            ];
            assert_eq!(from_graph(&graph), None);
        }
    }
    #[test]
    fn wrong_metadata_subject_or_instance_is_not_the_system_default() {
        let graph = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},
             "metadata":[{"subject":42,"key":"default.audio.sink","value":{"name":"x"}}]},
            {"id":2,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"x"}}}
        ]);
        assert_eq!(from_graph(graph.as_array().unwrap()), None);
        let mut graph = graph;
        graph[0]["metadata"][0]["subject"] = 0.into();
        graph[0]["props"]["metadata.name"] = "other".into();
        assert_eq!(from_graph(graph.as_array().unwrap()), None);
    }

    #[test]
    fn output_route_matches_profile_device_not_first_pcm() {
        let graph = serde_json::json!([
            {"id":2,"info":{"props":{"node.name":"x","device.id":3,"card.profile.device":7}}},
            {"id":3,"info":{"params":{"Route":[
                {"direction":"Output","device":2,"name":"headphones"},
                {"direction":"Output","device":7,"name":"speaker"}]}}}
        ]);
        let g: Vec<_> = graph.as_array().unwrap().iter().collect();
        assert!(endpoint(&g, &graph[0], false).wanted);
    }

    #[test]
    fn physical_microphone_identity_changes_reprime_without_sink_change() {
        let mut graph = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},
             "metadata":[{"subject":0,"key":"default.audio.sink","value":{"name":"speaker"}},
                         {"subject":0,"key":"default.configured.audio.source","value":{"name":"mic"}}]},
            {"id":2,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"speaker","device.id":3}}},
            {"id":4,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"mic","media.class":"Audio/Source","object.serial":40}}}
        ]);
        let mut watch = RouteWatch::default();
        assert_eq!(watch.absorb(graph.as_array().unwrap().clone()), Some(true));
        graph[2]["info"]["props"]["object.serial"] = 41.into();
        assert_eq!(watch.absorb(vec![graph[2].clone()]), Some(true));
        assert_eq!(watch.absorb(vec![graph[2].clone()]), None);
    }
}

#[cfg(test)]
mod metadata_deletion_tests {
    use super::*;
    #[test]
    fn empty_monitor_delta_invalidates_unreportable_deleted_keys() {
        let mut object = serde_json::json!({"metadata":[
            {"subject":0,"key":"default.audio.sink","value":{"name":"old"}}]});
        merge_metadata(&mut object, &serde_json::json!({"metadata":[]}));
        assert_eq!(object["metadata"], serde_json::json!([]));
    }
}
