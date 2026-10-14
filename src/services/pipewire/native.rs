//! Control-plane-only PipeWire client. A bound proxy, never an old global ID,
//! receives Props. Keep all PipeWire objects on this one thread (Node is !Send).
//! The caller is serialized and waits with a deadline; never call from audio RT.

use crate::pipeline::{MIC_CAPTURE_NODE_NAME, OUTPUT_NODE_NAME};
use pipewire as pw;
use pw::spa::{
    param::ParamType,
    pod::{
        Object, Pod, Property, PropertyFlags, Value, deserialize::PodDeserializer,
        serialize::PodSerializer,
    },
    utils::result::AsyncSeq,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    io::{self, Cursor},
    rc::Rc,
    sync::{Mutex, OnceLock, mpsc},
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_millis(1500);
const POLL: Duration = Duration::from_millis(20);
static CLIENT: OnceLock<Mutex<Option<mpsc::SyncSender<Request>>>> = OnceLock::new();

struct Request {
    name: String,
    id: u32,
    serial: String,
    cookie: u32,
    controls: Vec<(String, f64)>,
    expires: Instant,
    reply: mpsc::SyncSender<Result<(), String>>,
}

pub(super) fn set_props(
    name: &str,
    id: u32,
    serial: &str,
    cookie: u32,
    controls: &[(String, f64)],
) -> io::Result<()> {
    if !owned(name) || serial.is_empty() || controls.is_empty() || controls.len() > 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid owned-node update",
        ));
    }
    // Validate conversion to the actual POD type, not only the input f64.
    let mut keys = HashSet::new();
    if controls.iter().any(|(key, value)| {
        key.is_empty()
            || key.len() > 256
            || key.chars().any(char::is_control)
            || !keys.insert(key.as_str())
            || !(*value as f32).is_finite()
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid/duplicate control",
        ));
    }
    let mut client = CLIENT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| io::Error::other("live client lock poisoned"))?;
    if client.is_none() {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("audio-live-control".into())
            .spawn(move || {
                if let Err(error) = serve(rx) {
                    log::debug!("native live control: {error}");
                }
            })
            .map_err(io::Error::other)?;
        *client = Some(tx);
    }
    let (reply, result) = mpsc::sync_channel(1);
    let request = Request {
        name: name.into(),
        id,
        serial: serial.into(),
        cookie,
        controls: controls.to_vec(),
        expires: Instant::now() + DEADLINE,
        reply,
    };
    if client.as_ref().unwrap().try_send(request).is_err() {
        *client = None; // next call opens a new connection, never replay this write
        return Err(io::Error::other("native live connection unavailable"));
    }
    match result.recv_timeout(DEADLINE + Duration::from_millis(250)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(io::Error::other(error)),
        Err(_) => {
            *client = None;
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "native live confirmation timed out",
            ))
        }
    }
}

fn owned(name: &str) -> bool {
    name == MIC_CAPTURE_NODE_NAME || name == OUTPUT_NODE_NAME
}

struct Endpoint {
    name: String,
    serial: String,
    // Drop the listener before destroying the proxy.
    _listener: pw::node::NodeListener,
    node: Rc<pw::node::Node>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Inspect,
    Applied,
    Verify,
}
struct Pending {
    request: Request,
    node: Rc<pw::node::Node>,
    phase: Phase,
    barrier: Option<AsyncSeq>,
    done: bool,
    received: bool,
    values: BTreeMap<String, f64>,
    error: Option<String>,
}
#[derive(Default)]
struct State {
    endpoints: HashMap<u32, Endpoint>,
    announced: HashMap<u32, String>,
    cookie: Option<u32>,
    queued: Option<Request>,
    pending: Option<Pending>,
    boot: Option<AsyncSeq>,
    ready: bool,
    seq: i32,
}
impl State {
    fn next_seq(&mut self) -> i32 {
        self.seq = if self.seq >= 0x3fff_ff00 {
            1
        } else {
            self.seq + 1
        };
        self.seq
    }
    fn same_endpoint(&self, p: &Pending) -> bool {
        self.cookie == Some(p.request.cookie)
            && self
                .announced
                .values()
                .filter(|name| *name == &p.request.name)
                .count()
                == 1
            && self
                .endpoints
                .get(&p.request.id)
                .is_some_and(|e| e.serial == p.request.serial && Rc::ptr_eq(&e.node, &p.node))
    }
}

/// Follow the core: its cookie identifies this graph generation, `done` marks
/// our boot and per-request barriers, and an error on the core itself ends the
/// loop rather than leaving a request waiting on a connection that is gone.
fn watch_core(
    core: &pw::core::CoreRc,
    state: &Rc<RefCell<State>>,
    main_loop: &pw::main_loop::MainLoopRc,
) -> pw::core::Listener {
    let weak = Rc::downgrade(state);
    let weak_error = weak.clone();
    let weak_info = weak.clone();
    let quit = main_loop.downgrade();
    core.add_listener_local()
        .info(move |info| {
            if let Some(state) = weak_info.upgrade() {
                state.borrow_mut().cookie = Some(info.cookie());
            }
        })
        .done(move |id, seq| {
            if id != pw::core::PW_ID_CORE {
                return;
            }
            if let Some(state) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if s.boot == Some(seq) {
                    s.ready = true;
                    s.boot = None;
                }
                if let Some(p) = &mut s.pending
                    && p.barrier == Some(seq)
                {
                    p.done = true;
                }
            }
        })
        .error(move |id, _, code, message| {
            if let Some(state) = weak_error.upgrade()
                && let Some(p) = &mut state.borrow_mut().pending
            {
                p.error = Some(format!("PipeWire {id}: {code}: {message}"));
            }
            if id == pw::core::PW_ID_CORE
                && let Some(main_loop) = quit.upgrade()
            {
                main_loop.quit();
            }
        })
        .register()
}

/// Collect the Props this node reports, but only while it is the node the
/// in-flight request named.
fn follow_props(
    node: &pw::node::Node,
    state: &Rc<RefCell<State>>,
    id: u32,
) -> pw::node::NodeListener {
    let weak = Rc::downgrade(state);
    node.add_listener_local()
        .param(move |_seq, kind, _, _, pod| {
            if kind != ParamType::Props {
                return;
            }
            let Some(state) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            let Some(p) = &mut s.pending else { return };
            if p.request.id != id {
                return;
            }
            if let Some(pod) = pod {
                match values_from_pod(pod) {
                    Ok(values) => {
                        p.values.extend(values);
                        p.received = true;
                    }
                    Err(error) => p.error = Some(error),
                }
            }
        })
        .register()
}

/// Keep the endpoint table in step with the graph: every owned node this
/// process may address, bound so its Props can be read back.
fn watch_registry(
    registry: &pw::registry::RegistryRc,
    state: &Rc<RefCell<State>>,
) -> pw::registry::Listener {
    let weak = Rc::downgrade(state);
    let weak_remove = weak.clone();
    let binder = registry.clone();
    registry
        .add_listener_local()
        .global(move |global| {
            if global.type_ != pw::types::ObjectType::Node {
                return;
            }
            let Some(props) = global.props else { return };
            let Some(name) = props.get("node.name").filter(|n| owned(n)) else {
                return;
            };
            let Some(state) = weak.upgrade() else { return };
            state.borrow_mut().announced.insert(global.id, name.into());
            let Some(serial) = props.get("object.serial") else {
                return;
            };
            let Ok(node) = binder.bind::<pw::node::Node, _>(global) else {
                return;
            };
            let node = Rc::new(node);
            let listener = follow_props(&node, &state, global.id);
            state.borrow_mut().endpoints.insert(
                global.id,
                Endpoint {
                    name: name.into(),
                    serial: serial.into(),
                    _listener: listener,
                    node,
                },
            );
        })
        .global_remove(move |id| {
            if let Some(state) = weak_remove.upgrade() {
                // This is the registry callback, not the node callback being dropped.
                let mut s = state.borrow_mut();
                s.announced.remove(&id);
                s.endpoints.remove(&id);
            }
        })
        .register()
}

impl State {
    /// Take one request off the channel when nothing is in flight. Returns
    /// false when the sending side is gone, which ends the loop.
    fn pump(&mut self, rx: &mpsc::Receiver<Request>) -> bool {
        if self.pending.is_some() || self.queued.is_some() {
            return true;
        }
        match rx.try_recv() {
            Ok(request) => self.queued = Some(request),
            Err(mpsc::TryRecvError::Disconnected) => return false,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        true
    }

    /// Promote the queued request to in-flight, or refuse it. A request is
    /// refused when the graph generation it named is no longer the one in front
    /// of us: a write onto a different node is a write onto somebody else.
    fn start(&mut self, core: &pw::core::CoreRc) {
        if self.pending.is_some() {
            return;
        }
        let overdue = self
            .queued
            .as_ref()
            .is_some_and(|r| Instant::now() >= r.expires);
        if !self.ready && !overdue {
            return;
        }
        let Some(request) = self.queued.take() else {
            return;
        };
        let node = self
            .endpoints
            .get(&request.id)
            .filter(|e| e.name == request.name && e.serial == request.serial)
            .map(|e| e.node.clone());
        let stale = !self.ready
            || self.cookie != Some(request.cookie)
            || Instant::now() >= request.expires
            || self
                .announced
                .values()
                .filter(|name| *name == &request.name)
                .count()
                != 1;
        let (false, Some(node)) = (stale, node) else {
            let _ = request
                .reply
                .try_send(Err("stale/ambiguous live node generation".into()));
            return;
        };
        let seq = self.next_seq();
        enum_props(&node, seq);
        let barrier = core.sync(seq).ok();
        self.pending = Some(Pending {
            request,
            node,
            phase: Phase::Inspect,
            barrier,
            done: false,
            received: false,
            values: BTreeMap::new(),
            error: None,
        });
    }
}

/// Every reason an in-flight request can no longer be completed honestly.
fn refused(s: &State, p: &mut Pending) -> Option<String> {
    // Last reason wins, as it always has: a listener's error is the base, and
    // each structural check below overrides it. The request is refused either
    // way; the order only decides which sentence the caller is told.
    let mut error = p.error.take();
    if !s.same_endpoint(p) {
        error = Some("live node was removed/replaced".into());
    }
    if Instant::now() >= p.request.expires {
        error = Some("Props confirmation deadline".into());
    }
    if p.barrier.is_none() {
        error = Some("core sync failed".into());
    }
    error
}

/// Narrow the requested controls to the ones this node actually has, then write
/// them. Returns false when the request has been answered with an error.
///
/// Ignore a whole optional stage that isn't instantiated, but never silently
/// ignore an unknown control of a present stage.
fn inspect(p: &mut Pending) -> bool {
    match supported_controls(&p.values, &p.request.controls) {
        Ok(controls) => p.request.controls = controls,
        Err(error) => {
            let _ = p.request.reply.try_send(Err(error));
            return false;
        }
    }
    let bytes = match encode(&p.request.controls) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = p.request.reply.try_send(Err(error));
            return false;
        }
    };
    let Some(pod) = bytes.pod() else {
        let _ = p
            .request
            .reply
            .try_send(Err("invalid serialized Props".into()));
        return false;
    };
    write_props(&p.node, pod);
    p.phase = Phase::Applied;
    true
}

/// Move the in-flight request one step: inspect, apply, verify. Returns false
/// once the request has been answered, in which case it is dropped.
fn advance(s: &mut State, p: &mut Pending, core: &pw::core::CoreRc) -> bool {
    if !(p.done && (p.received || p.phase == Phase::Applied)) {
        return true;
    }
    match p.phase {
        Phase::Inspect => {
            if !inspect(p) {
                return false;
            }
        }
        Phase::Applied => p.phase = Phase::Verify,
        Phase::Verify => {
            if confirms(&p.values, &p.request.controls) {
                let _ = p.request.reply.try_send(Ok(()));
                return false;
            }
            // Retry READ only: never replay a write on a new generation.
        }
    }
    let seq = s.next_seq();
    p.values.clear();
    p.received = false;
    p.done = false;
    if p.phase == Phase::Verify {
        enum_props(&p.node, seq);
    }
    p.barrier = core.sync(seq).ok();
    true
}

fn serve(rx: mpsc::Receiver<Request>) -> Result<(), String> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(|e| e.to_string())?;
    let context = pw::context::ContextRc::new(&main_loop, None).map_err(|e| e.to_string())?;
    let core = context.connect_rc(None).map_err(|e| e.to_string())?;
    let registry = core.get_registry_rc().map_err(|e| e.to_string())?;
    let state = Rc::new(RefCell::new(State::default()));
    let _core_listener = watch_core(&core, &state, &main_loop);
    let _registry_listener = watch_registry(&registry, &state);
    state.borrow_mut().boot = Some(core.sync(0).map_err(|e| e.to_string())?);

    let tick_state = state.clone();
    let tick_core = core.clone();
    let quit = main_loop.downgrade();
    let timer = main_loop.loop_().add_timer(move |_| {
        let mut s = tick_state.borrow_mut();
        if !s.pump(&rx)
            && let Some(main_loop) = quit.upgrade()
        {
            main_loop.quit();
        }
        s.start(&tick_core);
        let Some(mut p) = s.pending.take() else {
            return;
        };
        if let Some(error) = refused(&s, &mut p) {
            let _ = p.request.reply.try_send(Err(error));
            return;
        }
        if advance(&mut s, &mut p, &tick_core) {
            s.pending = Some(p);
        }
    });
    timer
        .update_timer(Some(POLL), Some(POLL))
        .into_result()
        .map_err(|e| e.to_string())?;
    main_loop.run();
    if let Some(p) = state.borrow_mut().pending.take() {
        let _ = p
            .request
            .reply
            .try_send(Err("PipeWire disconnected".into()));
    }
    // Objects/listeners are dropped here, on the owning control thread.
    Ok(())
}

// PipeWire returns the Props via Node.param events tagged with an async seq the
// server assigns, not the client's request seq (verified live: request 0x00412145
// came back as 0x40000005). pipewire-rs 0.10 discards that return and keeps the
// proxy pointer private, so the reader cannot match on it. Instead a core.sync
// barrier after enum_params drains every Props event before the request advances,
// so the single in-flight request accepts them by node identity alone.
fn enum_props(node: &pw::node::Node, seq: i32) {
    // null POD filter (no start offset, all params).
    node.enum_params(seq, Some(ParamType::Props), 0, u32::MAX);
}
fn write_props(node: &pw::node::Node, pod: &Pod) {
    // POD is aligned/live until the synchronous marshaller has copied it.
    node.set_param(ParamType::Props, 0, pod);
}

struct AlignedPod {
    words: Vec<u64>,
    len: usize,
}
impl AlignedPod {
    fn pod(&self) -> Option<&Pod> {
        // Fully initialized Vec<u64>, 8-byte aligned, len <= allocated bytes.
        let bytes =
            unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast::<u8>(), self.len) };
        Pod::from_bytes(bytes)
    }
}
fn encode(controls: &[(String, f64)]) -> Result<AlignedPod, String> {
    let values = controls
        .iter()
        .flat_map(|(k, v)| [Value::String(k.clone()), Value::Float(*v as f32)])
        .collect();
    let value = Value::Object(Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamProps.as_raw(),
        id: ParamType::Props.as_raw(),
        properties: vec![Property {
            key: pw::spa::sys::SPA_PROP_params,
            flags: PropertyFlags::empty(),
            value: Value::Struct(values),
        }],
    });
    let (cursor, _) = PodSerializer::serialize(Cursor::new(Vec::new()), &value)
        .map_err(|e| format!("serialize Props: {e:?}"))?;
    let data = cursor.into_inner();
    let mut words = vec![0_u64; data.len().div_ceil(8)];
    for (word, bytes) in words.iter_mut().zip(data.chunks(8)) {
        let mut chunk = [0_u8; 8];
        chunk[..bytes.len()].copy_from_slice(bytes);
        *word = u64::from_ne_bytes(chunk);
    }
    Ok(AlignedPod {
        words,
        len: data.len(),
    })
}
fn values_from_pod(pod: &Pod) -> Result<BTreeMap<String, f64>, String> {
    if pod.as_bytes().len() > 128 * 1024 {
        return Err("oversized Props".into());
    }
    let (_, value) = PodDeserializer::deserialize_any_from(pod.as_bytes())
        .map_err(|e| format!("decode Props: {e:?}"))?;
    let Value::Object(object) = value else {
        return Err("Props is not an object".into());
    };
    if object.type_ != pw::spa::utils::SpaTypes::ObjectParamProps.as_raw()
        || object.id != ParamType::Props.as_raw()
    {
        return Err("wrong Props type".into());
    }
    let mut out = BTreeMap::new();
    for property in object.properties {
        if property.key != pw::spa::sys::SPA_PROP_params {
            continue;
        }
        let Value::Struct(values) = property.value else {
            return Err("params is not a struct".into());
        };
        if values.len() % 2 != 0 {
            return Err("odd params list".into());
        }
        for pair in values.as_chunks::<2>().0 {
            let Value::String(key) = &pair[0] else {
                return Err("non-string control key".into());
            };
            let v = match pair[1] {
                Value::Float(v) => f64::from(v),
                Value::Double(v) => v,
                Value::Int(v) => f64::from(v),
                Value::Bool(v) => f64::from(u8::from(v)),
                _ => continue,
            };
            if !v.is_finite() || out.insert(key.clone(), v).is_some() {
                return Err("invalid/duplicate control readback".into());
            }
        }
    }
    Ok(out)
}
fn supported_controls(
    values: &BTreeMap<String, f64>,
    sent: &[(String, f64)],
) -> Result<Vec<(String, f64)>, String> {
    let mut out = Vec::new();
    for (key, value) in sent {
        if values.contains_key(key) {
            out.push((key.clone(), *value));
            continue;
        }
        let Some((stage, _)) = key.split_once(':') else {
            return Err(format!("unknown control {key}"));
        };
        if values
            .keys()
            .any(|k| k.split_once(':').is_some_and(|(s, _)| s == stage))
        {
            return Err(format!("unknown control on present stage: {key}"));
        }
    }
    if out.is_empty() {
        Err("no supported live controls".into())
    } else {
        Ok(out)
    }
}
fn confirms(values: &BTreeMap<String, f64>, sent: &[(String, f64)]) -> bool {
    !sent.is_empty()
        && sent.iter().all(|(key, desired)| {
            values.get(key).is_some_and(|actual| {
                actual.is_finite()
                    && (actual - f64::from(*desired as f32)).abs() <= 1e-5 * desired.abs().max(1.0)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aligned_props_roundtrip_and_confirmation() {
        let sent = vec![
            ("filter:Gain".into(), -12.25),
            ("filter:Enable".into(), 1.0),
        ];
        let bytes = encode(&sent).unwrap();
        assert_eq!((bytes.words.as_ptr() as usize) % 8, 0);
        let values = values_from_pod(bytes.pod().unwrap()).unwrap();
        assert!(confirms(&values, &sent));
        assert!(!confirms(&values, &[("filter:Gain".into(), 0.0)]));
        assert!(supported_controls(&values, &[("filter:Typo".into(), 0.0)]).is_err());
        assert!(supported_controls(&values, &[("absent:Gain".into(), 0.0)]).is_err());
    }
}
