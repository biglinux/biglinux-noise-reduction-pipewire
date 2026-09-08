//! GUI entry point (`biglinux-microphone`).
//!
//! Delegates the whole lifecycle to the Relm4 root, which owns the GTK
//! window, the PipeWire service and the audio monitor.

fn main() -> glib::ExitCode {
    #[cfg(feature = "gtk-jemalloc")]
    big_jemalloc::anchor();

    env_logger::init_from_env(env_logger::Env::new().filter("BIGLINUX_MICROPHONE_LOG"));

    biglinux_microphone::ui::run()
}
