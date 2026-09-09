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

/// Optional frequency-based preference, restricted to the existing affinity
/// mask. Frequency is only a heuristic, not proof of a P/E-core topology.
fn prefer_fast_allowed_cpus() {
    let Ok(online_cpus) = read_to_string(std::path::Path::new("/sys/devices/system/cpu/online"))
    else {
        return;
    };

    // SAFETY: cpu_set_t is an initialized bitset of the size passed to libc.
    let mut allowed: libc::cpu_set_t = unsafe { mem::zeroed() };
    if unsafe { libc::sched_getaffinity(0, mem::size_of::<libc::cpu_set_t>(), &raw mut allowed) }
        != 0
    {
        return;
    }
    let mut cpus: Vec<(usize, u64)> = Vec::new();
    for cpu in parse_online_cpus(&online_cpus) {
        if cpu >= libc::CPU_SETSIZE as usize || !unsafe { libc::CPU_ISSET(cpu, &allowed) } {
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
                "biglinux-microphone-pwloader: opted in to allowed high-frequency CPUs {p_cores:?} ({max_freq} kHz)"
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

/// Opt-in reservation of mappings already loaded. Never use MCL_FUTURE:
/// later runtime allocations must not fail merely because a lock budget is
/// exhausted. PipeWire retains ownership of its own real-time buffers.
fn lock_pages_in_ram() {
    // SAFETY: `mlockall` is a libc function with no preconditions on
    // process state. The flag bits are documented constants.
    let rc = unsafe { libc::mlockall(libc::MCL_CURRENT) };
    if rc == 0 {
        eprintln!("biglinux-microphone-pwloader: mlockall() OK — pages pinned");
        return;
    }
    let error = std::io::Error::last_os_error();
    eprintln!(
        "biglinux-microphone-pwloader: mlockall() failed ({error}) — \
         memory reservation was not enabled; automatic paging remains in use"
    );
}

#[path = "../config/runtime.rs"]
mod runtime_preferences;

fn loader_preferences() -> runtime_preferences::RuntimeConfig {
    let Some(root) = dirs::config_dir() else {
        return runtime_preferences::RuntimeConfig::default();
    };
    let Some(value) = std::fs::read(root.join("biglinux-microphone/settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    else {
        return runtime_preferences::RuntimeConfig::default();
    };
    serde_json::from_value(value["runtime"].clone()).unwrap_or_default()
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
    for pair in raw_args.as_chunks::<2>().0 {
        let module_name = &pair[0];
        let args_path = resolve_args_path(&pair[1])?;
        let module_args =
            read_to_string(&args_path).map_err(|e| format!("read {}: {e}", args_path.display()))?;
        let module_name_c =
            CString::new(module_name.clone()).map_err(|e| format!("module name: {e}"))?;
        let module_args_c = CString::new(module_args).map_err(|e| format!("module args: {e}"))?;
        modules.push((module_name_c, module_args_c, module_name.clone()));
    }

    let preferences = loader_preferences();
    if preferences.prefer_fast_cpus {
        prefer_fast_allowed_cpus();
    }

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

    if preferences.reserve_memory {
        lock_pages_in_ram();
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

fn resolve_args_path(argument: &str) -> Result<std::path::PathBuf, String> {
    if let Some(name) = argument.strip_prefix("@config/") {
        if !matches!(name, "mic.args" | "aec.args" | "output.args") {
            return Err("unknown application configuration name".into());
        }
        let root = dirs::config_dir().ok_or("XDG configuration directory is unavailable")?;
        Ok(root.join("biglinux-microphone").join(name))
    } else {
        Ok(std::path::PathBuf::from(argument))
    }
}

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() == Some("--check-config") {
        return match arguments.next().map(|name| resolve_args_path(&name)) {
            Some(Ok(path)) if path.is_file() => ExitCode::SUCCESS,
            _ => ExitCode::FAILURE,
        };
    }

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
