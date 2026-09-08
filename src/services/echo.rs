//! Resolve automatic echo cancellation from the current PipeWire output route.

use std::collections::BTreeMap;

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
    last: Option<bool>,
}

impl RouteWatch {
    /// Fold one monitor batch in and report the route decision, but only when
    /// it differs from the last one. `None` is the common answer: most events
    /// say nothing about where the sound is going.
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
            } else {
                self.objects.insert(id, object);
            }
        }
        let wanted = from_graph(self.objects.values())?;
        (self.last.replace(wanted) != Some(wanted)).then_some(wanted)
    }
}

/// `graph` is iterated more than once, hence the `Clone` bound — it is
/// satisfied by a slice iterator and by `BTreeMap::values`, so neither caller
/// has to copy the graph to ask this question.
fn from_graph<'a>(graph: impl IntoIterator<Item = &'a Value> + Clone) -> Option<bool> {
    let name = graph
        .clone()
        .into_iter()
        .filter_map(|object| object["metadata"].as_array())
        .flatten()
        .find(|entry| entry["key"] == "default.audio.sink")?["value"]
        .clone();
    let name = if let Some(text) = name.as_str() {
        serde_json::from_str::<Value>(text).ok()?
    } else {
        name
    };
    let node = graph
        .clone()
        .into_iter()
        .find(|object| object["info"]["props"]["node.name"] == name["name"])?;
    let props = &node["info"]["props"];
    let device_id = props["device.id"]
        .as_u64()
        .or_else(|| props["device.id"].as_str()?.parse().ok());
    let route = graph
        .into_iter()
        .find(|object| object["id"].as_u64() == device_id)
        .and_then(|device| device["info"]["params"]["Route"].as_array())
        .and_then(|routes| {
            routes
                .iter()
                .find(|route| route["direction"] == "Output" || route["direction"] == 1)
        })
        .and_then(|route| route["name"].as_str());
    Some(microphone_can_hear_it(
        props["device.form-factor"]
            .as_str()
            .or_else(|| props["device.form_factor"].as_str()),
        route.or_else(|| props["port.name"].as_str()),
    ))
}

fn microphone_can_hear_it(form_factor: Option<&str>, port: Option<&str>) -> bool {
    if let Some(form) = form_factor {
        return !matches!(form, "headphone" | "headset");
    }
    port.is_none_or(|port| {
        let port = port.to_ascii_lowercase();
        !port.contains("headphone") && !port.contains("headset")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn speakers_headphones_and_explicit_modes() {
        assert!(microphone_can_hear_it(None, None));
        assert!(microphone_can_hear_it(Some("speaker"), None));
        assert!(!microphone_can_hear_it(Some("headset"), None));
        assert!(!microphone_can_hear_it(
            None,
            Some("analog-output-headphones")
        ));
        let mut config = EchoCancelConfig {
            enabled: false,
            mode: EchoMode::Always,
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
            {"id":1,"metadata":[{"key":"default.audio.sink","value":{"name":"test-sink"}}]},
            {"id":2,"info":{"props":{"node.name":"test-sink","device.id":3}}},
            {"id":3,"info":{"params":{"Route":[{"direction":"Output","name":"analog-output-headphones"}]}}}
        ]);
        assert_eq!(from_graph(graph.as_array().unwrap()), Some(false));
        graph[2]["info"]["params"]["Route"][0]["name"] = "analog-output-speaker".into();
        assert_eq!(from_graph(graph.as_array().unwrap()), Some(true));
        assert_eq!(from_graph(&[]), None);
    }

    #[test]
    fn route_watch_folds_deltas_and_only_speaks_up_on_a_change() {
        let mut watch = RouteWatch::default();
        // Batch 0 is the whole graph, as `pw-dump --monitor` sends it.
        let full = serde_json::json!([
            {"id":1,"metadata":[{"key":"default.audio.sink","value":{"name":"test-sink"}}]},
            {"id":2,"info":{"props":{"node.name":"test-sink","device.id":3}}},
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
        let unrelated = serde_json::json!([{"id":249,"info":{"props":{"node.name":"pw-dump"}}}]);
        assert_eq!(watch.absorb(unrelated.as_array().unwrap().clone()), None);

        // Objects with no `info` key at all are ordinary graph members, not
        // removals — the metadata that names the default sink is one of them.
        let metadata_update = serde_json::json!([
            {"id":1,"metadata":[{"key":"default.audio.sink","value":{"name":"test-sink"}}]}
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
}
