// SPDX-License-Identifier: GPL-3.0-or-later
//! **What a noise model costs on this machine**, measured rather than assumed.
//!
//! The automatic quality policy has to answer one question — will the heavier model keep
//! up here — and every input it had was a proxy. Core count says nothing about how fast
//! those cores are. Graph load says how busy the machine is, not how expensive the model
//! is. The honest answer is to run the model and time it.
//!
//! So this drives the shipped LADSPA object the way an audio thread does: instantiate at
//! the graph's rate, activate, then `run()` with exactly one quantum of audio, over and
//! over, pacing each call so the plugin sees the idle time a real data loop gives it.
//! What comes back is the share of the callback deadline the **audio thread** spends,
//! which is the quantity a dropout is made of.
//!
//! It costs about a second, once, at login. Compared against being wrong about which
//! model a machine can run — which the person hears as their voice cutting out — that is
//! cheap.

use std::ffi::{CString, c_char, c_int, c_ulong, c_void};
use std::time::{Duration, Instant};

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

use crate::config::noise_model::NoiseModel;

/// Audio timed, after the warm-up. Long enough for a p99 to mean something and short
/// enough that nobody notices it at login.
const MEASURED: Duration = Duration::from_secs(2);
/// Audio discarded first, so the model is loaded and its worker is running.
const WARMUP: Duration = Duration::from_millis(2500);
/// The block the microphone chain runs at, and therefore the deadline that matters.
const QUANTUM: usize = 960;
const RATE: u32 = 48_000;

// The LADSPA 1.1 ABI. Stable since 2002 and the reason this is safe to bind by hand:
// the layout below is the one every host on this machine already relies on.
const PORT_INPUT: c_int = 0x1;
const PORT_AUDIO: c_int = 0x8;

// The hint bits that say what a host should use for a control nobody set.
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

/// The value a host uses for a control nobody set.
///
/// Reading these is not optional politeness. Leaving every control at zero measured
/// DeepFilterNet3 at 26 % of the deadline where a host that honours its defaults sees
/// 67 %: three of its six controls are processing thresholds, and zero turns most of the
/// work off. A probe that silently benchmarks a bypass is worse than no probe.
fn default_for(hint: &PortRangeHint, rate: u32) -> f32 {
    let scale = if hint.hint_descriptor & HINT_SAMPLE_RATE != 0 {
        rate as f32
    } else {
        1.0
    };
    let low = hint.lower_bound * scale;
    let high = hint.upper_bound * scale;
    let interpolate = |weight: f32| {
        if hint.hint_descriptor & 0x10 != 0 && low > 0.0 && high > 0.0 {
            (low.ln() * (1.0 - weight) + high.ln() * weight).exp()
        } else {
            high.mul_add(weight, low * (1.0 - weight))
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
    if hint.hint_descriptor & 0x20 != 0 {
        value.round()
    } else {
        value
    }
}

/// Speech-like input, so a model that skips silence cheaply does not measure as free.
fn speech_like(buffer: &mut [f32], block: usize) {
    for (i, sample) in buffer.iter_mut().enumerate() {
        let n = (block * QUANTUM + i) as f32;
        let t = n / RATE as f32;
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

/// The share of one callback deadline this model's audio thread spends here, at p99.
///
/// `None` when the plugin is not installed or refuses to load — the caller must not read
/// that as "cheap", and every path here returns `None` rather than guessing.
///
/// Blocks for about four and a half seconds. Never call it from a UI thread or an audio
/// callback.
#[must_use]
pub fn audio_thread_share(model: NoiseModel) -> Option<f32> {
    // The live graph uses 48 kHz. Never instantiate a 16 kHz-only plugin
    // with the wrong host rate, even when a CLI caller provides its ID.
    if !model.is_realtime_lavfi_supported() {
        return None;
    }
    let (path, label) = model.plugin_and_label();
    let cpath = CString::new(path).ok()?;
    let want = CString::new(label).ok()?;

    // SAFETY: `cpath` is a valid NUL-terminated string. The handle is closed at the end
    // of this function, after every pointer taken from it has been dropped.
    let handle = unsafe { libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        return None;
    }
    let result = measure(handle, &want, model);
    // SAFETY: closing the handle opened above, with nothing borrowed from it still live.
    unsafe { libc::dlclose(handle) };
    result
}

fn measure(handle: *mut c_void, want: &CString, model: NoiseModel) -> Option<f32> {
    let asynchronous = matches!(model, NoiseModel::DpdfnetV2Hr | NoiseModel::DpdfnetV8Hr);
    let hops = if asynchronous {
        // These shipped plugins publish process-global atomic hop counters.
        // Without this evidence a cheap callback is not proof of denoising.
        let symbol = unsafe { libc::dlsym(handle, c"dpdfnet_hops".as_ptr()) };
        if symbol.is_null() {
            return None;
        }
        // SAFETY: the shipped accessor ABI is fn(*mut u64, *mut u64).
        Some(unsafe { std::mem::transmute::<*mut c_void, HopCounter>(symbol) })
    } else {
        None
    };
    let symbol = CString::new("ladspa_descriptor").ok()?;
    // SAFETY: `handle` is live and `symbol` is NUL-terminated.
    let entry = unsafe { libc::dlsym(handle, symbol.as_ptr()) };
    if entry.is_null() {
        return None;
    }
    // SAFETY: every LADSPA object declares this symbol with this signature; a file that
    // does not is not a LADSPA plugin and `dlsym` above would not have found it.
    let descriptor_at = unsafe {
        std::mem::transmute::<*mut c_void, unsafe extern "C" fn(c_ulong) -> *const Descriptor>(
            entry,
        )
    };

    let mut index = 0;
    let descriptor = loop {
        // SAFETY: the enumerator returns null past the last index, which ends the loop.
        let found = unsafe { descriptor_at(index) };
        if found.is_null() {
            return None;
        }
        // SAFETY: `found` is a live descriptor and `label` is a NUL-terminated literal
        // owned by the plugin.
        let label = unsafe { std::ffi::CStr::from_ptr((*found).label) };
        if label == want.as_c_str() {
            break found;
        }
        index += 1;
    };

    // SAFETY: `descriptor` is live for as long as the library handle is.
    let d = unsafe { &*descriptor };
    let instantiate = d.instantiate?;
    let connect = d.connect_port?;
    let run = d.run?;
    let cleanup = d.cleanup?;

    // SAFETY: the rate is one the plugin advertises support for; a plugin that refuses
    // returns null, which is checked.
    let instance = unsafe { instantiate(descriptor, c_ulong::from(RATE)) };
    if instance.is_null() {
        return None;
    }

    let ports = d.port_count as usize;
    let mut input = vec![0.0_f32; QUANTUM];
    let mut output = vec![0.0_f32; QUANTUM];
    let mut controls = vec![0.0_f32; ports];

    for port in 0..ports {
        // SAFETY: `port` is below `port_count`, so the descriptor array covers it.
        let kind = unsafe { *d.port_descriptors.add(port) };
        // SAFETY: the buffers outlive every `run` call below, and are unaliased.
        unsafe {
            if kind & PORT_AUDIO != 0 {
                let buffer = if kind & PORT_INPUT != 0 {
                    input.as_mut_ptr()
                } else {
                    output.as_mut_ptr()
                };
                connect(instance, port as c_ulong, buffer);
            } else {
                let name = std::ffi::CStr::from_ptr(*d.port_names.add(port)).to_string_lossy();
                let default = default_for(&*d.port_range_hints.add(port), RATE);
                controls[port] = benchmark_control(model, &name).unwrap_or(default);
                connect(instance, port as c_ulong, controls.as_mut_ptr().add(port));
            }
        }
    }

    if let Some(activate) = d.activate {
        // SAFETY: `instance` came from `instantiate` and has not been cleaned up.
        unsafe { activate(instance) };
    }

    let block = Duration::from_secs_f64(QUANTUM as f64 / f64::from(RATE));
    let warmup_blocks = (WARMUP.as_secs_f64() / block.as_secs_f64()) as usize;
    let timed_blocks = (MEASURED.as_secs_f64() / block.as_secs_f64()) as usize;
    let mut taken: Vec<Duration> = Vec::with_capacity(timed_blocks);

    let mut initial_hops = None;
    for b in 0..warmup_blocks + timed_blocks {
        if b == warmup_blocks {
            initial_hops = hops.map(read_hops);
        }
        speech_like(&mut input, b);
        let started = Instant::now();
        // SAFETY: every port is connected and the buffers are `QUANTUM` long.
        unsafe { run(instance, QUANTUM as c_ulong) };
        let elapsed = started.elapsed();
        // A real data loop idles between callbacks, and a plugin that builds its model on
        // a worker needs that idle time to ever finish. Running flat out measures the
        // warm-up instead of the model.
        if let Some(idle) = block.checked_sub(elapsed) {
            std::thread::sleep(idle);
        }
        if b >= warmup_blocks {
            taken.push(elapsed);
        }
    }

    let final_hops = hops.map(read_hops);
    if let Some(deactivate) = d.deactivate {
        // SAFETY: as for `activate`.
        unsafe { deactivate(instance) };
    }
    // SAFETY: last use of `instance`; nothing below touches it.
    unsafe { cleanup(instance) };

    if asynchronous && !processing_verified(initial_hops, final_hops) {
        return None;
    }
    taken.sort_unstable();
    let at = ((taken.len() as f64 * 0.99) as usize).min(taken.len().checked_sub(1)?);
    Some((taken[at].as_secs_f64() / block.as_secs_f64()) as f32)
}

/// The same reading, taken once per build of the plugin.
///
/// Timing the model costs four and a half seconds. Doing that at every login, for a
/// number that only changes when the plugin or the machine does, is four and a half
/// seconds of somebody's login. The cache is keyed on the plugin's size and modification
/// time, so a package update re-measures and nothing else does.
#[must_use]
pub fn audio_thread_share_cached(model: NoiseModel) -> Option<f32> {
    let (path, label) = model.plugin_and_label();
    let stamp = std::fs::metadata(path)
        .ok()
        .and_then(|meta| Some((meta.len(), meta.modified().ok()?)))?;
    // A different model/control surface or boot must not reuse this reading.
    // Binding to the boot also avoids copying a timing result between hosts.
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let key = cache_key(
        model,
        label,
        path,
        &format!("{}:{:?}:{}", stamp.0, stamp.1, boot.trim()),
    );
    let cache = cache_file();

    // Read once and keep it: the same file was read again below to rebuild
    // the retained entries.
    let stored_entries = std::fs::read_to_string(&cache).unwrap_or_default();
    for line in stored_entries.lines() {
        if let Some((stored, value)) = line.rsplit_once(' ')
            && stored == key
            && let Ok(share) = value.parse::<f32>()
            && share.is_finite()
            && share > 0.0
        {
            return Some(share);
        }
    }

    // Native inference runtimes may load allocators with private symbol binding.
    // Keep their benchmark out of the jemalloc GUI process.
    //
    // Through the shared subprocess boundary rather than `Command::output()`:
    // the measurement takes seconds by design, and a child that never exits
    // would otherwise hold the apply worker forever.
    let cli = std::env::current_exe()
        .ok()?
        .with_file_name("biglinux-microphone-cli");
    let output = BigSubprocessSpec::builder()
        .program(cli.to_str()?)
        .args(["measure-model", &(model as u8).to_string()])
        .stderr(BigSubprocessOutputMode::Null)
        .allow_list([cli.to_str()?])
        .build()
        .run()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let share = output.stdout_lossy().trim().parse::<f32>().ok()?;
    if !share.is_finite() || share <= 0.0 {
        return None;
    }
    // Keep only the entries that still describe a file on this machine, so a cache that
    // has seen a year of updates does not grow without bound.
    let kept: String = stored_entries
        .lines()
        .filter(|line| line.rsplit_once(' ').is_some_and(|(k, _)| k != key))
        .take(16)
        .fold(String::new(), |mut acc, line| {
            acc.push_str(line);
            acc.push('\n');
            acc
        });
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, format!("{kept}{key} {share}\n"));
    Some(share)
}

fn cache_file() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME").map_or_else(
        || std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"),
        std::path::PathBuf::from,
    );
    base.join("biglinux-microphone").join("plugin-cost")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_with_no_plugin_installed_says_so() {
        // Whatever is on the machine running this, an answer of `Some(0.0)` would be the
        // dangerous one: it reads as "free" and would promote a model that is not there.
        for model in [NoiseModel::DpdfnetV2Hr, NoiseModel::GtcrnDns3] {
            if let Some(share) = audio_thread_share(model) {
                assert!(
                    share > 0.0,
                    "{model:?} measured at exactly zero, which no real plugin does"
                );
            }
        }
    }

    /// Prints what this machine measures, for the comment in `quality.rs` that
    /// carries the ladder's numbers. Ignored: it takes five seconds per model and
    /// its output is a reading, not an assertion.
    #[test]
    #[ignore = "reports rather than asserts; run with --ignored when the ladder moves"]
    fn what_each_model_costs_here() {
        for model in [
            NoiseModel::GtcrnDns3,
            NoiseModel::DeepFilterNet3,
            NoiseModel::DpdfnetV2Hr,
            NoiseModel::DpdfnetV8Hr,
        ] {
            match audio_thread_share(model) {
                Some(share) => eprintln!("{model:?}: {:.1}% of the deadline", share * 100.0),
                None => eprintln!("{model:?}: not installed"),
            }
        }
    }

    #[test]
    fn the_speech_signal_is_not_silence() {
        let mut buffer = vec![0.0_f32; QUANTUM];
        speech_like(&mut buffer, 0);
        let rms = (buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len() as f32).sqrt();
        assert!(rms > 0.05, "a silent probe would measure the skip path");
    }
}

type HopCounter = unsafe extern "C" fn(*mut u64, *mut u64);

fn read_hops(counter: HopCounter) -> (u64, u64) {
    let (mut asked, mut enhanced) = (0, 0);
    // SAFETY: both arguments point to live u64 values; the accessor was
    // resolved from a library handle retained for the whole measurement.
    unsafe {
        counter(&raw mut asked, &raw mut enhanced);
    }
    (asked, enhanced)
}

fn processing_verified(start: Option<(u64, u64)>, end: Option<(u64, u64)>) -> bool {
    let (Some((before, completed_before)), Some((after, completed_after))) = (start, end) else {
        return false;
    };
    let (Some(asked), Some(enhanced)) = (
        after.checked_sub(before),
        completed_after.checked_sub(completed_before),
    ) else {
        return false;
    };
    asked > 0 && enhanced <= asked && enhanced as f64 / asked as f64 >= 0.98
}

fn benchmark_control(model: NoiseModel, name: &str) -> Option<f32> {
    if model.is_attenuation_only() {
        return (name == "Attenuation Limit (dB)").then_some(100.0);
    }
    match name {
        "Enable" | "Strength" | "VoiceRecovery" => Some(1.0),
        "Model" => Some(model.ladspa_control()),
        "SpeechStrength" => Some(crate::config::gtcrn_speech_strength(1.0) as f32),
        "LookaheadMs" | "ModelBlend" => Some(0.0),
        "Threshold (dB)" => Some(-80.0),
        _ => None,
    }
}

fn cache_key(model: NoiseModel, label: &str, path: &str, stamp: &str) -> String {
    format!(
        "verified-v2:{}:{label}:{RATE}:{QUANTUM}:{path}:{stamp}",
        model as u8
    )
}

#[cfg(test)]
mod measurement_contracts {
    use super::*;
    #[test]
    fn fast_callbacks_without_completed_work_are_not_a_valid_measurement() {
        assert!(!processing_verified(Some((0, 0)), Some((100, 20))));
        assert!(!processing_verified(None, None));
        assert!(!processing_verified(Some((100, 100)), Some((0, 0))));
        assert!(processing_verified(Some((0, 0)), Some((100, 99))));
    }
    #[test]
    fn variants_sharing_a_library_have_distinct_controls_and_cache_keys() {
        let a = NoiseModel::GtcrnDns3;
        let b = NoiseModel::GtcrnVctk;
        assert_ne!(
            cache_key(a, "same", "same", "same"),
            cache_key(b, "same", "same", "same")
        );
        assert_eq!(benchmark_control(a, "Model"), Some(0.0));
        assert_eq!(benchmark_control(b, "Model"), Some(1.0));
    }
    #[test]
    fn logarithmic_ladspa_defaults_use_geometric_interpolation() {
        let hint = PortRangeHint {
            hint_descriptor: 0xC0 | 0x10,
            lower_bound: 1.0,
            upper_bound: 100.0,
        };
        assert!((default_for(&hint, RATE) - 10.0).abs() < 0.001);
    }
}
