//! GTK4 / libadwaita user interface.
//!
//! Structure:
//!
//! * [`MicrophoneApplication`] — top-level `adw::Application`. Owns the
//!   background services for the app's lifetime.
//! * [`window::build`] — composes the main [`adw::ApplicationWindow`]
//!   out of three views (Mic, Output, Spectrum).
//! * [`state::AppState`] — shared settings snapshot + debounced
//!   persistence / reload.

mod app;
mod i18n;
mod state;
mod views;
mod widgets;
mod window;

pub use app::MicrophoneApplication;
pub use i18n::init_gettext;
