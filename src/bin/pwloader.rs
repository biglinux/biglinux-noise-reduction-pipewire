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
//! biglinux-microphone-pwloader <module-name> <args-file>
//! ```
//!
//! `args-file` is read verbatim and passed as the module's `args` string.
//! Failure to open the file or load the module exits non-zero and is
//! visible to systemd's `Restart=on-failure`.

use std::ffi::CString;
use std::fs;
use std::process::ExitCode;

use pipewire as pw;
use pw::loop_::Signal;

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let module_name = args
        .next()
        .ok_or_else(|| "usage: biglinux-microphone-pwloader <module> <args-file>".to_owned())?;
    let args_path = args
        .next()
        .ok_or_else(|| "usage: biglinux-microphone-pwloader <module> <args-file>".to_owned())?;

    let module_args =
        fs::read_to_string(&args_path).map_err(|e| format!("read {args_path}: {e}"))?;

    let module_name_c =
        CString::new(module_name.clone()).map_err(|e| format!("module name: {e}"))?;
    let module_args_c = CString::new(module_args).map_err(|e| format!("module args: {e}"))?;

    pw::init();

    let main_loop =
        pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("MainLoop::new: {e}"))?;
    let context =
        pw::context::ContextRc::new(&main_loop, None).map_err(|e| format!("Context::new: {e}"))?;
    // Hold the Core proxy: dropping it disconnects from the daemon, which
    // tears down every node the loaded module exported.
    let _core = context
        .connect_rc(None)
        .map_err(|e| format!("connect: {e}"))?;

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
