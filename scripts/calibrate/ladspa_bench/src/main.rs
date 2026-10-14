//! Steady-state per-block CPU cost of an arbitrary LADSPA plugin.
//!
//! The quality harness times plugins through ffmpeg, which reloads the model on
//! every clip and so measures cold-start, not the audio loop. This drives the
//! `.so` the way an audio thread does instead: instantiate once at 48 kHz,
//! activate, then `run()` one quantum over and over — warm — pacing each call so
//! the plugin sees the idle a real graph gives it. What comes back is the
//! distribution of per-block processing time (p50/p95/p99/max) and the real-time
//! factor, host-consistent because there is no ffmpeg or python around it.
//!
//! Usage:
//!   ladspa_bench <plugin.so> <label> [--rate 48000] [--block 480]
//!               [--seconds 4] [--warmup-ms 1500]
//!               [--control "Name=value"]... [--json out.json]
//!
//! Controls default to the plugin's own LADSPA hint defaults; `--control` by
//! port name overrides one (e.g. --control "Min SNR gate (dB)=-40" to bench the
//! DeepFilterNet3 gate open).

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_ulong, c_void, CStr, CString};
use std::time::{Duration, Instant};

const PORT_INPUT: c_int = 0x1;
const PORT_AUDIO: c_int = 0x8;
const HINT_DEFAULT_MASK: c_int = 0x3C0;
const HINT_SAMPLE_RATE: c_int = 0x8;

#[repr(C)]
struct PortRangeHint {
    hint_descriptor: c_int,
    lower_bound: f32,
    upper_bound: f32,
}

#[repr(C)]
struct Descriptor {
    unique_id: c_ulong,
    label: *const c_char,
    properties: c_int,
    name: *const c_char,
    maker: *const c_char,
    copyright: *const c_char,
    port_count: c_ulong,
    port_descriptors: *const c_int,
    port_names: *const *const c_char,
    port_range_hints: *const PortRangeHint,
    implementation_data: *mut c_void,
    instantiate: Option<unsafe extern "C" fn(*const Descriptor, c_ulong) -> *mut c_void>,
    connect_port: Option<unsafe extern "C" fn(*mut c_void, c_ulong, *mut f32)>,
    activate: Option<unsafe extern "C" fn(*mut c_void)>,
    run: Option<unsafe extern "C" fn(*mut c_void, c_ulong)>,
    run_adding: *mut c_void,
    set_run_adding_gain: *mut c_void,
    deactivate: Option<unsafe extern "C" fn(*mut c_void)>,
    cleanup: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// The value a LADSPA host uses for a control nobody set (same rules the app's
/// plugin_cost.rs reads — leaving a processing threshold at zero benches a
/// bypass, not the model).
fn default_for(hint: &PortRangeHint, rate: u32) -> f32 {
    let scale = if hint.hint_descriptor & HINT_SAMPLE_RATE != 0 { rate as f32 } else { 1.0 };
    let low = hint.lower_bound * scale;
    let high = hint.upper_bound * scale;
    let interpolate = |w: f32| {
        if hint.hint_descriptor & 0x10 != 0 && low > 0.0 && high > 0.0 {
            (low.ln() * (1.0 - w) + high.ln() * w).exp()
        } else {
            high.mul_add(w, low * (1.0 - w))
        }
    };
    let value = match hint.hint_descriptor & HINT_DEFAULT_MASK {
        0x40 => low,
        0x80 => interpolate(0.25),
        0xC0 => interpolate(0.5),
        0x100 => interpolate(0.75),
        0x140 => high,
        0x240 => 1.0,
        0x280 => 100.0,
        0x2C0 => 440.0,
        _ => 0.0,
    };
    if hint.hint_descriptor & 0x20 != 0 { value.round() } else { value }
}

/// Speech-like input, so a model that skips silence cheaply is not scored free.
fn speech_like(buffer: &mut [f32], block: usize, quantum: usize, rate: u32) {
    for (i, sample) in buffer.iter_mut().enumerate() {
        let n = (block * quantum + i) as f32;
        let t = n / rate as f32;
        let hiss = (n * 12.9898).sin().mul_add(43758.547, 0.0).fract() - 0.5;
        *sample = 0.06_f32.mul_add(
            (t * 1300.0 * std::f32::consts::TAU).sin(),
            0.25_f32.mul_add(
                (t * 130.0 * std::f32::consts::TAU).sin(),
                0.12 * (t * 390.0 * std::f32::consts::TAU).sin(),
            ),
        ) + 0.04 * hiss;
    }
}

struct Args {
    so: String,
    label: String,
    rate: u32,
    block: usize,
    seconds: f64,
    warmup_ms: u64,
    controls: HashMap<String, f32>,
    json: Option<String>,
}

fn parse_args() -> Args {
    let mut a = std::env::args().skip(1);
    let so = a.next().unwrap_or_else(|| usage("missing plugin .so"));
    let label = a.next().unwrap_or_else(|| usage("missing plugin label"));
    let mut args = Args {
        so,
        label,
        rate: 48_000,
        block: 480,
        seconds: 4.0,
        warmup_ms: 1500,
        controls: HashMap::new(),
        json: None,
    };
    while let Some(flag) = a.next() {
        let mut val = || a.next().unwrap_or_else(|| usage("flag needs a value"));
        match flag.as_str() {
            "--rate" => args.rate = val().parse().unwrap_or_else(|_| usage("bad --rate")),
            "--block" => args.block = val().parse().unwrap_or_else(|_| usage("bad --block")),
            "--seconds" => args.seconds = val().parse().unwrap_or_else(|_| usage("bad --seconds")),
            "--warmup-ms" => args.warmup_ms = val().parse().unwrap_or_else(|_| usage("bad --warmup-ms")),
            "--json" => args.json = Some(val()),
            "--control" => {
                let kv = val();
                let (k, v) = kv.rsplit_once('=').unwrap_or_else(|| usage("--control needs Name=value"));
                args.controls.insert(k.to_string(), v.parse().unwrap_or_else(|_| usage("bad control value")));
            }
            other => usage(&format!("unknown flag {other}")),
        }
    }
    args
}

fn usage(msg: &str) -> ! {
    eprintln!("ladspa_bench: {msg}");
    eprintln!("usage: ladspa_bench <plugin.so> <label> [--rate R] [--block N] [--seconds S] [--warmup-ms M] [--control \"Name=value\"]... [--json out]");
    std::process::exit(2);
}

/// Result of a run: per-block audio-thread times, plus the whole process's CPU
/// time over the timed window. The two differ for a plugin that offloads to a
/// worker thread — its `run()` returns fast (cheap audio thread) while a worker
/// core burns the real cost, which only the process-wide CPU time captures.
struct Measured {
    per_block: Vec<Duration>,
    cpu_seconds: f64,
    audio_seconds: f64,
}

/// Total CPU seconds this process has used across all its threads.
fn process_cpu_seconds() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: ts is a valid timespec; the clock id is a documented constant.
    unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9
}

fn main() {
    let args = parse_args();
    let measured = run_plugin(&args).unwrap_or_else(|e| {
        eprintln!("ladspa_bench: {e}");
        std::process::exit(1);
    });
    report(&args, &measured);
}

fn run_plugin(args: &Args) -> Result<Measured, String> {
    let cpath = CString::new(args.so.as_str()).map_err(|_| "bad path")?;
    let want = CString::new(args.label.as_str()).map_err(|_| "bad label")?;
    // SAFETY: cpath is NUL-terminated; the handle is closed after every pointer
    // taken from it has been dropped at the end of this function.
    let handle = unsafe { libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        return Err(format!("dlopen failed for {}", args.so));
    }
    let result = drive(handle, &want, args);
    // SAFETY: closing the handle opened above; nothing borrowed from it is live.
    unsafe { libc::dlclose(handle) };
    result
}

fn drive(handle: *mut c_void, want: &CStr, args: &Args) -> Result<Measured, String> {
    let symbol = CString::new("ladspa_descriptor").unwrap();
    // SAFETY: handle is live, symbol NUL-terminated.
    let entry = unsafe { libc::dlsym(handle, symbol.as_ptr()) };
    if entry.is_null() {
        return Err("no ladspa_descriptor symbol (not a LADSPA plugin?)".into());
    }
    // SAFETY: every LADSPA object declares this symbol with this signature.
    let descriptor_at = unsafe {
        std::mem::transmute::<*mut c_void, unsafe extern "C" fn(c_ulong) -> *const Descriptor>(entry)
    };
    let mut index = 0;
    let descriptor = loop {
        // SAFETY: the enumerator returns null past the last index.
        let found = unsafe { descriptor_at(index) };
        if found.is_null() {
            return Err(format!("label {:?} not found in plugin", want));
        }
        // SAFETY: found is a live descriptor; label is a NUL-terminated literal owned by it.
        if unsafe { CStr::from_ptr((*found).label) } == want {
            break found;
        }
        index += 1;
    };

    // SAFETY: descriptor is live for as long as the handle is.
    let d = unsafe { &*descriptor };
    let instantiate = d.instantiate.ok_or("no instantiate")?;
    let connect = d.connect_port.ok_or("no connect_port")?;
    let run = d.run.ok_or("no run")?;
    let cleanup = d.cleanup.ok_or("no cleanup")?;

    // SAFETY: the rate is passed to instantiate as the LADSPA ABI requires; a
    // plugin that refuses returns null, checked below.
    let instance = unsafe { instantiate(descriptor, c_ulong::from(args.rate)) };
    if instance.is_null() {
        return Err(format!("instantiate returned null at {} Hz", args.rate));
    }

    let ports = d.port_count as usize;
    let mut input = vec![0.0_f32; args.block];
    let mut output = vec![0.0_f32; args.block];
    let mut controls = vec![0.0_f32; ports];

    for port in 0..ports {
        // SAFETY: port < port_count, so the descriptor arrays cover it.
        let kind = unsafe { *d.port_descriptors.add(port) };
        // SAFETY: buffers outlive every run call below and are unaliased.
        unsafe {
            if kind & PORT_AUDIO != 0 {
                let buffer = if kind & PORT_INPUT != 0 { input.as_mut_ptr() } else { output.as_mut_ptr() };
                connect(instance, port as c_ulong, buffer);
            } else {
                let name = CStr::from_ptr(*d.port_names.add(port)).to_string_lossy().into_owned();
                let default = default_for(&*d.port_range_hints.add(port), args.rate);
                controls[port] = args.controls.get(&name).copied().unwrap_or(default);
                connect(instance, port as c_ulong, controls.as_mut_ptr().add(port));
            }
        }
    }

    if let Some(activate) = d.activate {
        // SAFETY: instance came from instantiate and is not cleaned up.
        unsafe { activate(instance) };
    }

    let block = Duration::from_secs_f64(args.block as f64 / f64::from(args.rate));
    let warmup_blocks = (Duration::from_millis(args.warmup_ms).as_secs_f64() / block.as_secs_f64()) as usize;
    let timed_blocks = (args.seconds / block.as_secs_f64()) as usize;
    let mut taken = Vec::with_capacity(timed_blocks);
    let mut cpu_start = 0.0;

    // Pre-generate every block's stimulus before the timed region (review
    // BENCH-03): otherwise the trig-heavy `speech_like` runs inside the
    // process-CPU window and inflates the plugin's measured cost. The loop then
    // only does a cheap memcpy into the connected input buffer plus `run`.
    let stimulus: Vec<Vec<f32>> = (0..warmup_blocks + timed_blocks)
        .map(|b| {
            let mut buf = vec![0.0_f32; args.block];
            speech_like(&mut buf, b, args.block, args.rate);
            buf
        })
        .collect();

    for b in 0..warmup_blocks + timed_blocks {
        if b == warmup_blocks {
            cpu_start = process_cpu_seconds();
        }
        input.copy_from_slice(&stimulus[b]);
        let started = Instant::now();
        // SAFETY: every port is connected and the buffers are `block` long.
        unsafe { run(instance, args.block as c_ulong) };
        let elapsed = started.elapsed();
        // Pace like a real data loop so a worker-backed plugin gets its idle
        // time and we measure the model, not the warm-up. The pause is real
        // time, not CPU time, so a worker's cost still lands in the process CPU
        // clock read below.
        if let Some(idle) = block.checked_sub(elapsed) {
            std::thread::sleep(idle);
        }
        if b >= warmup_blocks {
            taken.push(elapsed);
        }
    }
    let cpu_seconds = process_cpu_seconds() - cpu_start;
    let audio_seconds = taken.len() as f64 * block.as_secs_f64();

    if let Some(deactivate) = d.deactivate {
        // SAFETY: as for activate.
        unsafe { deactivate(instance) };
    }
    // SAFETY: last use of instance.
    unsafe { cleanup(instance) };

    if taken.is_empty() {
        return Err("no timed blocks (raise --seconds)".into());
    }
    Ok(Measured { per_block: taken, cpu_seconds, audio_seconds })
}

fn percentile(sorted: &[Duration], q: f64) -> Duration {
    let at = ((sorted.len() as f64 * q) as usize).min(sorted.len() - 1);
    sorted[at]
}

fn report(args: &Args, m: &Measured) {
    let durations = &m.per_block;
    let mut sorted = durations.to_vec();
    sorted.sort_unstable();
    let block = args.block as f64 / f64::from(args.rate);
    let us = |d: Duration| d.as_secs_f64() * 1e6;
    let rtf = |d: Duration| d.as_secs_f64() / block;
    let (p50, p95, p99, max) = (
        percentile(&sorted, 0.50),
        percentile(&sorted, 0.95),
        percentile(&sorted, 0.99),
        *sorted.last().unwrap(),
    );
    let mean: f64 = durations.iter().map(|d| d.as_secs_f64()).sum::<f64>() / durations.len() as f64;
    // All-thread CPU per second of audio: for a synchronous plugin this equals
    // the audio-thread RTF; for a worker-backed one it is higher, and is the
    // honest compute cost that the cheap `run()` hides.
    let cpu_rtf = m.cpu_seconds / m.audio_seconds;

    eprintln!(
        "{} @ {} Hz, block {} ({:.1} ms), {} blocks",
        args.label, args.rate, args.block, block * 1000.0, durations.len()
    );
    eprintln!(
        "  audio-thread us/block:  p50 {:.1}  p95 {:.1}  p99 {:.1}  max {:.1}  mean {:.1}",
        us(p50), us(p95), us(p99), us(max), mean * 1e6
    );
    eprintln!(
        "  audio-thread RTF:       p50 {:.4}  p99 {:.4}  (deadline {:.1} ms)",
        rtf(p50), rtf(p99), block * 1000.0
    );
    eprintln!("  total CPU RTF (all threads): {cpu_rtf:.4}");

    if let Some(path) = &args.json {
        let json = format!(
            "{{\"label\":{:?},\"rate\":{},\"block\":{},\"blocks\":{},\
\"deadline_ms\":{:.4},\"us_p50\":{:.3},\"us_p95\":{:.3},\"us_p99\":{:.3},\"us_max\":{:.3},\
\"us_mean\":{:.3},\"rtf_p50\":{:.5},\"rtf_p99\":{:.5},\"cpu_rtf\":{:.5}}}\n",
            args.label, args.rate, args.block, durations.len(), block * 1000.0,
            us(p50), us(p95), us(p99), us(max), mean * 1e6, rtf(p50), rtf(p99), cpu_rtf,
        );
        if std::fs::write(path, json).is_err() {
            eprintln!("ladspa_bench: could not write {path}");
        }
    }
}
