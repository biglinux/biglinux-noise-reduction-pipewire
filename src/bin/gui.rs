//! GUI entry point (`biglinux-microphone`).
//!
//! Delegates the whole lifecycle to [`MicrophoneApplication`] which owns
//! the GTK app, the PipeWire service and the audio monitor.

use biglinux_microphone::ui::{init_gettext, MicrophoneApplication};

fn main() -> glib::ExitCode {
    pretty_env_logger::init_custom_env("BIGLINUX_MICROPHONE_LOG");
    init_gettext();

    let app = MicrophoneApplication::new();
    app.run()
}
