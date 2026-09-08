//! `biglinux-microphone-pwloader` — minimal PipeWire client that loads a
//! module into its own local context and stays alive until SIGTERM/SIGINT.
//!
//! # Why this exists
//!
//! Both the mic chain and the per-app output filter need to share the
//! main PipeWire daemon's clock. A separate `pipewire -c file.conf`
//! daemon would give each filter graph independent quantum/rate negotiation,
//! causing cross-process drift, `spa.alsa: front:1p ... resync` events, and
//! audible micro-cuts.
//!
//! This loader connects as a regular client to the running PipeWire daemon
//! (so it inherits the daemon's clock), then calls
//! `pw_context_load_module()` to instantiate a filter-chain (or any other)
//! module inside this client process. The module's audio nodes are
//! exported to the daemon's graph and driven by the daemon's data-loop —
//! one clock, no drift. When the loader receives SIGTERM, it drops its
//! Core proxy, the module is unloaded, and the nodes disappear from the
//! daemon's graph. The audio system itself is never restarted.
//!
//! # Usage
//!
//! ```text
//! biglinux-microphone-pwloader <module-name> <args-file> [<module-name> <args-file> ...]
//! ```
//!
//! Each `args-file` is read verbatim and passed as that module's `args`
//! string. Multiple module/args pairs load all of them inside the same
//! Context, so they share one data-loop thread (no cross-process IPC,
//! no SMT contention between sibling NN inferences). Failure to open
//! any file or load any module exits non-zero and is visible to
//! systemd's `Restart=on-failure`.

use std::ffi::CString;
use std::mem;
use std::process::ExitCode;
use std::time::Duration;

use std::fs::read_to_string;
#[path = "../pwloader/denoise_watch.rs"]
mod denoise_watch;
use denoise_watch::DenoiseWatch;

use pipewire as pw;
use pw::loop_::Signal;

/// On Intel/ARM hybrid CPUs (e.g. i5-13400 with 6 P-cores @ 4.6 GHz +
/// 4 E-cores @ 3.3 GHz), the kernel can land an FIFO-priority audio
/// thread on an E-core. PipeWire's `module-rt` sets `uclamp.min=768`
/// to bias toward fast cores, but `intel_pstate=active` ignores that
/// hint for *core selection* — it only affects per-core frequency
/// scaling. The data-loop thread then runs ~30 % slower and the
/// extra latency surfaces as audible micro-stutters in heavy filter
/// chains (DPDFNet, DeepFilterNet3).
///
/// Pin the loader process to the set of CPUs whose `cpuinfo_max_freq`
/// matches the system maximum. Threads spawned later (PipeWire's
/// data-loop) inherit this mask, so the entire filter graph stays on
/// P-cores. On homogeneous CPUs (every core at the same max freq),
/// this is a no-op.
fn pin_to_p_cores() {
    let Ok(online_cpus) = read_to_string(std::path::Path::new("/sys/devices/system/cpu/online"))
    else {
        return;
    };

    let mut cpus: Vec<(usize, u64)> = Vec::new();
    for cpu in parse_online_cpus(&online_cpus) {
        if cpu >= libc::CPU_SETSIZE as usize {
            continue;
        }
        let path = std::path::Path::new("/sys/devices/system/cpu")
            .join(format!("cpu{cpu}"))
            .join("cpufreq/cpuinfo_max_freq");
        let Ok(raw) = read_to_string(&path) else {
            continue;
        };
        let Ok(freq) = raw.trim().parse::<u64>() else {
            continue;
        };
        cpus.push((cpu, freq));
    }

    if cpus.is_empty() {
        return;
    }

    let max_freq = cpus.iter().map(|(_, f)| *f).max().unwrap_or(0);
    let p_cores: Vec<usize> = cpus
        .iter()
        .filter(|(_, f)| *f == max_freq)
        .map(|(n, _)| *n)
        .collect();

    if p_cores.len() == cpus.len() {
        // Homogeneous CPU — nothing to gain from pinning.
        return;
    }

    // SAFETY: `cpu_set_t` is a plain bitset; zero-init via mem::zeroed
    // is the documented way to start an empty set before `CPU_SET`.
    // Every CPU index was checked against `CPU_SETSIZE`, and
    // `sched_setaffinity` reads `set` for exactly `size` bytes.
    unsafe {
        let mut set: libc::cpu_set_t = mem::zeroed();
        for cpu in &p_cores {
            libc::CPU_SET(*cpu, &mut set);
        }
        let rc = libc::sched_setaffinity(0, mem::size_of::<libc::cpu_set_t>(), &raw const set);
        if rc == 0 {
            eprintln!(
                "biglinux-microphone-pwloader: pinned to P-cores {p_cores:?} ({max_freq} kHz)"
            );
        } else {
            let error = std::io::Error::last_os_error();
            eprintln!("biglinux-microphone-pwloader: sched_setaffinity failed ({error})");
        }
    }
}

fn parse_online_cpus(raw: &str) -> Vec<usize> {
    raw.trim()
        .split(',')
        .flat_map(|part| {
            if let Some((start, end)) = part.split_once('-') {
                let start = start.parse::<usize>().ok();
                let end = end.parse::<usize>().ok();
                match (start, end) {
                    (Some(start), Some(end)) if start <= end => (start..=end).collect::<Vec<_>>(),
                    _ => Vec::new(),
                }
            } else {
                part.parse::<usize>()
                    .map_or_else(|_| Vec::new(), |cpu| vec![cpu])
            }
        })
        .collect()
}

/// `mlockall(MCL_CURRENT | MCL_FUTURE)` — see the call site in `run()` for
/// the rationale. Failure is non-fatal: the log line lets the operator
/// know the spike-mitigation guard didn't engage but the loader keeps
/// going so audio still works.
fn lock_pages_in_ram() {
    // SAFETY: `mlockall` is a libc function with no preconditions on
    // process state. The flag bits are documented constants.
    let rc = unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) };
    if rc == 0 {
        eprintln!("biglinux-microphone-pwloader: mlockall() OK — pages pinned");
        return;
    }
    let error = std::io::Error::last_os_error();
    eprintln!(
        "biglinux-microphone-pwloader: mlockall() failed ({error}) — \
         expect occasional pw-top spikes from page faults; check RLIMIT_MEMLOCK"
    );
}

/// How often to sample the denoiser's counters. Long enough that the timer
/// costs nothing, short enough that a degraded window is noticed while the
/// call is still happening.
const WATCH_PERIOD: Duration = Duration::from_secs(10);

fn run() -> Result<(), String> {
    const USAGE: &str =
        "usage: biglinux-microphone-pwloader <module> <args-file> [<module> <args-file> ...]";

    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    if raw_args.is_empty() || !raw_args.len().is_multiple_of(2) {
        return Err(USAGE.to_owned());
    }

    // Parse + read every (module, args-file) pair up front so a bad path
    // exits before we touch PipeWire.
    let mut modules: Vec<(CString, CString, String)> = Vec::with_capacity(raw_args.len() / 2);
    for pair in raw_args.chunks_exact(2) {
        let module_name = &pair[0];
        let args_path = &pair[1];
        let module_args = read_to_string(std::path::Path::new(args_path))
            .map_err(|e| format!("read {args_path}: {e}"))?;
        let module_name_c =
            CString::new(module_name.clone()).map_err(|e| format!("module name: {e}"))?;
        let module_args_c = CString::new(module_args).map_err(|e| format!("module args: {e}"))?;
        modules.push((module_name_c, module_args_c, module_name.clone()));
    }

    // Pin BEFORE pw::init() so the data-loop thread (spawned inside
    // libpipewire) inherits the affinity mask.
    pin_to_p_cores();

    // Lock all current and future pages into RAM. The PipeWire data-loop
    // runs SCHED_FIFO at prio 83, but a major page fault inside ORT (cold
    // weight pages, lazy mmap-backed regions, freshly allocated arena
    // pages) still stalls the audio thread for milliseconds — long enough
    // to spike pw-top from ~0.8 ms to >5 ms after a few seconds of run.
    // MCL_FUTURE pre-faults every subsequent allocation, so ORT's first
    // post-silence inference can't trip a fault. Best-effort: when
    // RLIMIT_MEMLOCK is too low (no rtkit / non-audio group), we log and
    // continue — the system stays functional but may still exhibit
    // fault-related latency spikes.
    lock_pages_in_ram();

    pw::init();

    let main_loop =
        pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("MainLoop::new: {e}"))?;
    let context =
        pw::context::ContextRc::new(&main_loop, None).map_err(|e| format!("Context::new: {e}"))?;
    // Hold the Core proxy: dropping it disconnects from the daemon, which
    // tears down every node every loaded module exported.
    let _core = context
        .connect_rc(None)
        .map_err(|e| format!("connect: {e}"))?;

    for (module_name_c, module_args_c, module_name) in &modules {
        // SAFETY: `pw::init()` has run; `context.as_raw_ptr()` is non-null
        // for a successfully-constructed Context. The C function returns
        // either NULL (failure) or a borrowed `pw_impl_module` whose lifetime
        // is tied to `context` — we never destroy it manually, the Context's
        // Drop will.
        let module = unsafe {
            pw::sys::pw_context_load_module(
                context.as_raw_ptr(),
                module_name_c.as_ptr(),
                module_args_c.as_ptr(),
                std::ptr::null_mut(),
            )
        };
        if module.is_null() {
            return Err(format!("pw_context_load_module failed for {module_name}"));
        }
    }

    // Watch whether the denoiser keeps up. The plugin counts its own hops;
    // reading them from here rather than from the worker is the point — a
    // starved thread cannot report that it is starved.
    let watches: Vec<DenoiseWatch> = modules
        .iter()
        .flat_map(|(_, args, _)| denoise_watch::plugin_paths(&args.to_string_lossy()))
        .filter_map(|path| DenoiseWatch::open(std::path::Path::new(&path)))
        .collect();
    // `add_timer` takes an `Fn`, so the state it mutates lives behind a
    // `RefCell`. Only the main loop ever touches it, and never reentrantly.
    let watching = !watches.is_empty();
    let watches = std::cell::RefCell::new(watches);
    let watch_timer = watching.then(|| {
        let timer = main_loop.loop_().add_timer(move |_| {
            for watch in watches.borrow_mut().iter_mut() {
                watch.tick();
            }
        });
        let _ = timer
            .update_timer(Some(WATCH_PERIOD), Some(WATCH_PERIOD))
            .into_result();
        timer
    });

    let weak = main_loop.downgrade();
    let _sig_int = main_loop.loop_().add_signal_local(Signal::INT, move || {
        if let Some(m) = weak.upgrade() {
            m.quit();
        }
    });
    let weak = main_loop.downgrade();
    let _sig_term = main_loop.loop_().add_signal_local(Signal::TERM, move || {
        if let Some(m) = weak.upgrade() {
            m.quit();
        }
    });

    main_loop.run();
    drop(watch_timer);
    Ok(())
}

fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("biglinux-microphone-pwloader: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::parse_online_cpus;

    #[test]
    fn parses_sysfs_online_cpu_ranges() {
        assert_eq!(
            parse_online_cpus("0-3,8,10-11\n"),
            vec![0, 1, 2, 3, 8, 10, 11]
        );
    }

    #[test]
    fn ignores_malformed_online_cpu_ranges() {
        assert_eq!(parse_online_cpus("0-1,bad,5-3,7\n"), vec![0, 1, 7]);
    }
}
