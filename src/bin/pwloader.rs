//! `biglinux-microphone-pwloader` — minimal PipeWire client that loads a
//! module into its own local context and stays alive until SIGTERM/SIGINT.
//!
//! # Why this exists
//!
//! Both the mic chain and the per-app output filter need to share the
//! main PipeWire daemon's clock. Spawning a second `pipewire -c file.conf`
//! daemon (the original architecture) gave each filter graph its own
//! quantum/rate negotiation and produced cross-process drift — surfaced
//! as `spa.alsa: front:1p ... resync` events and audible micro-cuts.
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
use std::fs;
use std::mem;
use std::process::ExitCode;

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
    let Ok(entries) = fs::read_dir("/sys/devices/system/cpu") else {
        return;
    };

    let mut cpus: Vec<(usize, u64)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(s) = name.to_str() else { continue };
        let Some(num) = s.strip_prefix("cpu").and_then(|n| n.parse::<usize>().ok()) else {
            continue;
        };
        let path = entry.path().join("cpufreq/cpuinfo_max_freq");
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(freq) = raw.trim().parse::<u64>() else {
            continue;
        };
        cpus.push((num, freq));
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
    // is the documented way to start an empty set before
    // `CPU_SET`. `sched_setaffinity` reads `set` for `size` bytes.
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
            let errno = *libc::__errno_location();
            eprintln!("biglinux-microphone-pwloader: sched_setaffinity failed (errno {errno})");
        }
    }
}

/// Promote the calling (main) thread to `SCHED_FIFO` at priority 88
/// **before** `pw::init()` so the data-loop thread spawned inside
/// libpipewire inherits the policy. Linux pthread default is
/// `PTHREAD_INHERIT_SCHED`, so child threads pick up the parent's
/// scheduling policy at creation time.
///
/// Why duplicate what `libpipewire-module-rt` already does:
/// `module-rt` is loaded asynchronously inside the PipeWire context, by
/// which time the data-loop thread already exists. If `module-rt` then
/// hits `EPERM` on `pthread_setschedparam` (insufficient
/// `RLIMIT_RTPRIO`), the data-loop silently stays `SCHED_OTHER` and
/// the audio path runs without RT — the exact failure mode that
/// produces 5 ms `pw-top` spikes when GTCRN warms ORT pages. Setting
/// `SCHED_FIFO` up-front turns this into a hard failure (logged),
/// avoiding the silent regression.
///
/// Priority 88 matches what BigLinux's `module-rt` variant 2 ships
/// (heterogeneous CPU, stock kernel) — high enough to outrank every
/// userspace thread, low enough to stay below the kernel's RT softirqs.
///
/// Failure is non-fatal: when rlimits forbid RT (process launched
/// outside `biglinux-microphone-*.service`, no PAM session, user
/// missing from the `realtime` group), we log a clear diagnostic and
/// let the loader continue. `module-rt` will still try later and may
/// or may not succeed; either way, the operator sees in the journal
/// why audio is glitchy.
fn promote_to_realtime() {
    // SAFETY: `sched_param` is a plain POD; libc's `sched_setscheduler`
    // takes a pointer-to-const for it. PID 0 means "current thread".
    let param = libc::sched_param { sched_priority: 88 };
    let rc = unsafe { libc::sched_setscheduler(0, libc::SCHED_FIFO, &raw const param) };
    if rc == 0 {
        eprintln!(
            "biglinux-microphone-pwloader: sched_setscheduler(SCHED_FIFO, 88) OK — \
             data-loop will inherit RT policy"
        );
        return;
    }
    let errno = unsafe { *libc::__errno_location() };
    eprintln!(
        "biglinux-microphone-pwloader: sched_setscheduler(SCHED_FIFO, 88) failed (errno {errno}) — \
         module-rt may retry; if RLIMIT_RTPRIO=0 (loader launched outside the systemd unit), \
         data-loop stays SCHED_OTHER and pw-top spikes are expected"
    );
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
    let errno = unsafe { *libc::__errno_location() };
    eprintln!(
        "biglinux-microphone-pwloader: mlockall() failed (errno {errno}) — \
         expect occasional pw-top spikes from page faults; check RLIMIT_MEMLOCK"
    );
}

fn run() -> Result<(), String> {
    const USAGE: &str =
        "usage: biglinux-microphone-pwloader <module> <args-file> [<module> <args-file> ...]";

    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    if raw_args.is_empty() || raw_args.len() % 2 != 0 {
        return Err(USAGE.to_owned());
    }

    // Parse + read every (module, args-file) pair up front so a bad path
    // exits before we touch PipeWire.
    let mut modules: Vec<(CString, CString, String)> = Vec::with_capacity(raw_args.len() / 2);
    for pair in raw_args.chunks_exact(2) {
        let module_name = &pair[0];
        let args_path = &pair[1];
        let module_args =
            fs::read_to_string(args_path).map_err(|e| format!("read {args_path}: {e}"))?;
        let module_name_c =
            CString::new(module_name.clone()).map_err(|e| format!("module name: {e}"))?;
        let module_args_c = CString::new(module_args).map_err(|e| format!("module args: {e}"))?;
        modules.push((module_name_c, module_args_c, module_name.clone()));
    }

    // Pin BEFORE pw::init() so the data-loop thread (spawned inside
    // libpipewire) inherits the affinity mask.
    pin_to_p_cores();

    // Promote main thread to SCHED_FIFO BEFORE pw::init() so the
    // data-loop thread inherits the RT policy at creation time
    // (PTHREAD_INHERIT_SCHED is the Linux pthread default). Avoids the
    // race where module-rt loads asynchronously and silently fails on
    // EPERM, leaving the data-loop on SCHED_OTHER.
    promote_to_realtime();

    // Lock all current and future pages into RAM. The PipeWire data-loop
    // runs SCHED_FIFO at prio 83, but a major page fault inside ORT (cold
    // weight pages, lazy mmap-backed regions, freshly allocated arena
    // pages) still stalls the audio thread for milliseconds — long enough
    // to spike pw-top from ~0.8 ms to >5 ms after a few seconds of run.
    // MCL_FUTURE pre-faults every subsequent allocation, so ORT's first
    // post-silence inference can't trip a fault. Best-effort: when
    // RLIMIT_MEMLOCK is too low (no rtkit / non-audio group), we log and
    // continue — the system stays functional, just with the original
    // spike profile.
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

    let weak = main_loop.downgrade();
    let _sig_int = main_loop.loop_().add_signal_local(Signal::SIGINT, move || {
        if let Some(m) = weak.upgrade() {
            m.quit();
        }
    });
    let weak = main_loop.downgrade();
    let _sig_term = main_loop
        .loop_()
        .add_signal_local(Signal::SIGTERM, move || {
            if let Some(m) = weak.upgrade() {
                m.quit();
            }
        });

    main_loop.run();
    Ok(())
}

fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("biglinux-microphone-pwloader: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
