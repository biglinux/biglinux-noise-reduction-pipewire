//! GUI entry point (`biglinux-microphone`).
//!
//! Delegates the whole lifecycle to `MicrophoneApplication` which owns
//! the GTK app, the PipeWire service and the audio monitor.

use biglinux_microphone::ui::{init_gettext, MicrophoneApplication};

fn main() -> glib::ExitCode {
    env_logger::init_from_env(env_logger::Env::new().filter("BIGLINUX_MICROPHONE_LOG"));
    init_gettext();

    let app = MicrophoneApplication::new();
    app.run()
}
