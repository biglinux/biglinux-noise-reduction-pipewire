//! Resolve automatic echo cancellation from the current PipeWire output route.

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

fn from_graph(graph: &[Value]) -> Option<bool> {
    let name = graph
        .iter()
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
        .iter()
        .find(|object| object["info"]["props"]["node.name"] == name["name"])?;
    let props = &node["info"]["props"];
    let device_id = props["device.id"]
        .as_u64()
        .or_else(|| props["device.id"].as_str()?.parse().ok());
    let route = graph
        .iter()
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
}
