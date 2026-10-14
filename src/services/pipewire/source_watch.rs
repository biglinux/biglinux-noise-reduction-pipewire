//! Read-only, event-driven graph cache for the AGC control thread.
//! No capture, timer, subprocess or volume write occurs on this PipeWire loop.
//! Volume remains with WirePlumber (wpctl), preserving ALSA route/mixer policy.
use super::sources::{CHAIN_NODES, Source, is_virtual_source};
use crate::pipeline::{EC_SOURCE_NAME, MIC_NODE_NAME};
use pipewire as pw;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Keep listeners and proxies on their owning thread; drop the listener first.
type Metadata = (pw::metadata::MetadataListener, pw::metadata::Metadata);

#[derive(Default)]
struct State {
    ready: bool,
    cookie: Option<u32>,
    nodes: BTreeMap<u32, (Source, bool)>,
    links: BTreeMap<u32, (u32, u32)>,
    defaults: BTreeMap<u32, Option<String>>,
}
impl State {
    fn named(&self, name: &str) -> Option<Source> {
        if !self.ready {
            return None;
        }
        let mut matches = self.nodes.values().filter(|(s, _)| s.node_name == name);
        let (source, _) = matches.next()?;
        if matches.next().is_some() || source.object_serial.is_none() {
            return None;
        }
        let mut source = source.clone();
        source.core_cookie = Some(self.cookie?);
        Some(source)
    }
    fn default_source(&self) -> Option<Source> {
        if self.defaults.len() != 1 {
            return None;
        }
        let name = self.defaults.values().next()?.as_deref()?;
        let source = self.named(name)?;
        self.nodes
            .get(&source.node_id)
            .filter(|(_, hardware)| *hardware)?;
        Some(source)
    }
    fn has_consumer(&self) -> bool {
        self.ready
            && self.links.values().any(|(output, input)| {
                let Some((source, _)) = self.nodes.get(output) else {
                    return false;
                };
                let Some((consumer, _)) = self.nodes.get(input) else {
                    return false;
                };
                [MIC_NODE_NAME, EC_SOURCE_NAME].contains(&source.node_name.as_str())
                    && !CHAIN_NODES.contains(&consumer.node_name.as_str())
            })
    }
}

pub(crate) struct SourceWatch {
    state: Arc<Mutex<State>>,
    stop: pw::channel::Sender<()>,
    worker: Option<JoinHandle<()>>,
}
impl SourceWatch {
    /// Connect and require the first complete reading within two seconds, for
    /// one-off users such as calibration.
    pub(crate) fn start() -> Result<Self, String> {
        let watch = Self::connect()?;
        if watch.state.lock().is_ok_and(|s| s.ready) {
            Ok(watch)
        } else {
            Err("PipeWire graph initialization timed out or failed".into())
        }
    }

    /// Whether the connection behind this watch is still being served.
    pub(crate) fn is_alive(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
    }

    /// Connect, and give the first reading up to two seconds; a reading that is
    /// late is not a failure while the connection is alive.
    ///
    /// **For a daemon that keeps the watch.** The AGC used to drop the watch on
    /// every failed attempt and connect again two seconds later. While
    /// WirePlumber was starting, that churn left WirePlumber announcing no change
    /// of the `default` metadata to any client for the rest of its life — the
    /// desktop's sound card, `pactl subscribe` and `pw-metadata -m` all kept the
    /// old default output. Measured: four restarts of WirePlumber with the AGC
    /// running, four sessions with no change events; three with it stopped,
    /// three with them.
    pub(crate) fn connect() -> Result<Self, String> {
        let state = Arc::new(Mutex::new(State::default()));
        let (stop, receiver) = pw::channel::channel();
        let shared = state.clone();
        let worker = thread::Builder::new()
            .name("agc-source-watch".into())
            .spawn(move || {
                // Also invalidates the cache on panic while unwinding this worker.
                struct Invalidate(Arc<Mutex<State>>);
                impl Drop for Invalidate {
                    fn drop(&mut self) {
                        if let Ok(mut s) = self.0.lock() {
                            s.ready = false;
                        }
                    }
                }
                let _guard = Invalidate(shared.clone());
                if let Err(error) = serve(shared, receiver) {
                    log::debug!("AGC graph watch: {error}");
                }
            })
            .map_err(|e| e.to_string())?;
        let watch = Self {
            state,
            stop,
            worker: Some(worker),
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if watch.state.lock().is_ok_and(|s| s.ready) {
                return Ok(watch);
            }
            if !watch.is_alive() {
                return Err("PipeWire graph initialization failed".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(watch)
    }
    pub(crate) fn default_source(&self) -> Option<Source> {
        self.state.lock().ok()?.default_source()
    }
    pub(crate) fn is_current(&self, source: &Source) -> bool {
        self.default_source()
            .is_some_and(|s| source.same_generation(&s))
    }
    /// Require both hardware and meter generations; a rebuilt filter invalidates
    /// learned normalization even if it kept the same node.name.
    pub(crate) fn meter_source(&self, hardware: &Source) -> Option<Source> {
        let state = self.state.lock().ok()?;
        if !hardware.same_generation(&state.default_source()?) {
            return None;
        }
        if state
            .nodes
            .values()
            .any(|(s, _)| s.node_name == MIC_NODE_NAME)
        {
            state.named(MIC_NODE_NAME) // ambiguity is failure, not a raw fallback
        } else {
            state.named(&hardware.node_name)
        }
    }
    pub(crate) fn has_consumer(&self) -> bool {
        self.state.lock().is_ok_and(|s| s.has_consumer())
    }
}
impl Drop for SourceWatch {
    fn drop(&mut self) {
        let _ = self.stop.send(()); // one shutdown message; no periodic wake-up
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(shared: Arc<Mutex<State>>, stop: pw::channel::Receiver<()>) -> Result<(), String> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(|e| e.to_string())?;
    let _stop = stop.attach(main_loop.loop_(), {
        let quit = main_loop.downgrade();
        move |()| {
            if let Some(main_loop) = quit.upgrade() {
                main_loop.quit();
            }
        }
    });
    let context = pw::context::ContextRc::new(&main_loop, None).map_err(|e| e.to_string())?;
    let core = context.connect_rc(None).map_err(|e| e.to_string())?;
    let registry = core.get_registry_rc().map_err(|e| e.to_string())?;
    let boot = Rc::new(RefCell::new(None));
    let _core = watch_core(&core, &boot, &shared, &main_loop);
    let metadata: Rc<RefCell<BTreeMap<u32, Metadata>>> = Rc::new(RefCell::new(BTreeMap::new()));
    let global_meta = metadata.clone();
    let removed_meta = metadata.clone();
    let sync_core = core.clone();
    let sync_boot = boot.clone();
    let added = shared.clone();
    let removed = shared.clone();
    let binder = registry.clone();
    let quit = main_loop.downgrade();
    let _registry = registry
        .add_listener_local()
        .global(move |global| {
            let Some(props) = global.props else { return };
            // Every branch answers the same question: is the cached view still
            // whole? A "no" from any of them is handled once, below.
            let healthy = {
                let Ok(mut s) = added.lock() else { return };
                if global.type_ == pw::types::ObjectType::Node {
                    cache_node(&mut s, global.id, props)
                } else if global.type_ == pw::types::ObjectType::Link {
                    cache_link(&mut s, global.id, props)
                } else if global.type_ == pw::types::ObjectType::Metadata
                    && props.get("metadata.name") == Some("default")
                {
                    if s.defaults.len() >= 8 {
                        false
                    } else {
                        s.defaults.insert(global.id, None);
                        s.ready = false;
                        drop(s); // never hold a cache mutex while binding a proxy
                        bind_default_metadata(
                            global,
                            &added,
                            &binder,
                            &sync_core,
                            &sync_boot,
                            &global_meta,
                        )
                    }
                } else {
                    true
                }
            };
            if !healthy {
                if let Ok(mut s) = added.lock() {
                    s.ready = false;
                }
                if let Some(m) = quit.upgrade() {
                    m.quit();
                }
            }
        })
        .global_remove(move |id| {
            if let Ok(mut s) = removed.lock() {
                s.nodes.remove(&id);
                s.links.remove(&id);
                s.defaults.remove(&id);
            }
            removed_meta.borrow_mut().remove(&id);
        })
        .register();
    *boot.borrow_mut() = Some(core.sync(0).map_err(|e| e.to_string())?);
    main_loop.run();
    Ok(())
}
/// Follow the core itself: its cookie identifies this graph generation, its
/// `done` for our boot sequence is what makes the cache readable, and any error
/// means a subscription failed, which leaves the view incomplete.
fn watch_core(
    core: &pw::core::CoreRc,
    boot: &Rc<RefCell<Option<pw::spa::utils::result::AsyncSeq>>>,
    shared: &Arc<Mutex<State>>,
    main_loop: &pw::main_loop::MainLoopRc,
) -> pw::core::Listener {
    let done_boot = boot.clone();
    let info_state = shared.clone();
    let done_state = shared.clone();
    let error_state = shared.clone();
    let quit = main_loop.downgrade();
    core.add_listener_local()
        .info(move |info| {
            if let Ok(mut s) = info_state.lock() {
                s.cookie = Some(info.cookie());
            }
        })
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE
                && *done_boot.borrow() == Some(seq)
                && let Ok(mut s) = done_state.lock()
            {
                s.ready = true;
            }
        })
        .error(move |_, _, _, _| {
            // Any failed subscription makes our view incomplete: fail closed.
            if let Ok(mut s) = error_state.lock() {
                s.ready = false;
            }
            if let Some(main_loop) = quit.upgrade() {
                main_loop.quit();
            }
        })
        .register()
}

/// Cache one node global. Returns false when the cache already holds its cap:
/// a truncated view is a torn view, and the caller fails closed rather than
/// answering questions from half a graph.
fn cache_node(s: &mut State, id: u32, props: &pw::spa::utils::dict::DictRef) -> bool {
    if s.nodes.len() >= 4096 {
        return false;
    }
    let Some(name) = props
        .get("node.name")
        .filter(|n| !n.is_empty() && n.len() <= 4096)
    else {
        return true; // a node without a usable name is not an error, just not ours
    };
    let hardware = props.get("media.class") == Some("Audio/Source") && !is_virtual_source(name);
    s.nodes.insert(
        id,
        (
            Source {
                node_id: id,
                node_name: name.into(),
                description: props
                    .get("node.description")
                    .unwrap_or(name)
                    .chars()
                    .take(4096)
                    .collect(),
                object_serial: props
                    .get("object.serial")
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|n| n.to_string()),
                core_cookie: None,
            },
            hardware,
        ),
    );
    true
}

/// Cache one link global, under the same cap rule as `cache_node`.
fn cache_link(s: &mut State, id: u32, props: &pw::spa::utils::dict::DictRef) -> bool {
    if s.links.len() >= 16384 {
        return false;
    }
    let output = props.get("link.output.node").and_then(|n| n.parse().ok());
    let input = props.get("link.input.node").and_then(|n| n.parse().ok());
    if let (Some(output), Some(input)) = (output, input) {
        s.links.insert(id, (output, input));
    }
    true
}

/// Bind the default-metadata object and follow `default.audio.source` on it.
/// Returns false if binding or the follow-up sync fails, which leaves the cache
/// unable to name a default and so must fail closed.
///
/// Binding happens while processing registry globals, AFTER the initial sync
/// request. The extra sync waits for metadata's own initial properties rather
/// than exposing an apparently ready but incomplete default.
fn bind_default_metadata(
    global: &pw::registry::GlobalObject<&pw::spa::utils::dict::DictRef>,
    state: &Arc<Mutex<State>>,
    registry: &pw::registry::RegistryRc,
    core: &pw::core::CoreRc,
    boot: &Rc<RefCell<Option<pw::spa::utils::result::AsyncSeq>>>,
    store: &Rc<RefCell<BTreeMap<u32, Metadata>>>,
) -> bool {
    let Ok(proxy) = registry.bind::<pw::metadata::Metadata, _>(global) else {
        return false;
    };
    let id = global.id;
    let owner = state.clone();
    let listener = proxy
        .add_listener_local()
        .property(move |subject, key, _, value| {
            if subject == 0
                && (key.is_none() || key == Some("default.audio.source"))
                && let Ok(mut s) = owner.lock()
            {
                let name = if key.is_none() {
                    None
                } else {
                    value.and_then(metadata_name)
                };
                s.defaults.insert(id, name);
            }
            0
        })
        .register();
    store.borrow_mut().insert(id, (listener, proxy));
    let Ok(sequence) = core.sync(0) else {
        return false;
    };
    *boot.borrow_mut() = Some(sequence);
    true
}

fn metadata_name(text: &str) -> Option<String> {
    if text.len() > 8192 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value
        .get("name")?
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(id: u32, name: &str) -> Source {
        Source {
            node_id: id,
            node_name: name.into(),
            description: name.into(),
            object_serial: Some(id.to_string()),
            core_cookie: None,
        }
    }
    #[test]
    fn graph_cache_rejects_ambiguous_defaults_and_recycled_generations() {
        let mut s = State {
            ready: true,
            cookie: Some(10),
            ..State::default()
        };
        s.nodes.insert(1, (source(1, "mic"), true));
        s.defaults.insert(2, Some("mic".into()));
        let before = s.default_source().unwrap();
        s.cookie = Some(11);
        assert!(!before.same_generation(&s.default_source().unwrap()));
        s.defaults.insert(3, Some("mic".into()));
        assert!(s.default_source().is_none());
        s.defaults.remove(&3);
        s.nodes.insert(4, (source(4, "mic"), true));
        assert!(s.default_source().is_none());
    }
    #[test]
    fn graph_cache_does_not_count_its_own_meter_as_demand() {
        let mut s = State {
            ready: true,
            ..State::default()
        };
        s.nodes.insert(1, (source(1, MIC_NODE_NAME), false));
        s.nodes.insert(
            2,
            (
                source(2, crate::services::audio_monitor::METER_NODE_NAME),
                false,
            ),
        );
        s.links.insert(3, (1, 2));
        assert!(!s.has_consumer());
        s.nodes.insert(4, (source(4, "Firefox"), false));
        s.links.insert(5, (1, 4));
        assert!(s.has_consumer());
        s.ready = false;
        assert!(!s.has_consumer());
    }
    #[test]
    fn malformed_or_removed_metadata_never_selects_a_microphone() {
        assert_eq!(metadata_name(r#"{"name":"mic"}"#), Some("mic".into()));
        for text in ["null", "[]", "{", r#"{"name":""}"#] {
            assert!(metadata_name(text).is_none());
        }
    }
}
