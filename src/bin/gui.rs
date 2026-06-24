//! GUI entry point (`biglinux-microphone`).
//!
//! Delegates the whole lifecycle to `MicrophoneApplication` which owns
//! the GTK app, the PipeWire service and the audio monitor.

use biglinux_microphone::ui::{init_gettext, MicrophoneApplication};

fn main() -> glib::ExitCode {
    // Bind jemalloc as the process allocator (links libjemalloc; see big-jemalloc).
    // GTK4 only returns freed memory to the OS under jemalloc. The cli/pwloader
    // helpers are separate binaries that do not link it.
    big_jemalloc::anchor();
    env_logger::init_from_env(env_logger::Env::new().filter("BIGLINUX_MICROPHONE_LOG"));
    init_gettext();

    let app = MicrophoneApplication::new();
    app.run()
}
