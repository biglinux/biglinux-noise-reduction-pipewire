//! UI views, each exposing a `build(&state, …) -> gtk::Widget` entry.
//!
//! Layout selection lives in [`super::window::build`]:
//!
//! * [`Mode::Simple`] → [`simple::build`] renders a single page that
//!   contains both the microphone and the system-sound switches with
//!   one intensity slider each.
//! * [`Mode::Advanced`] → an `adw::ViewStack` with [`mic::build`] and
//!   [`output::build`] as titled children.

pub mod mic;
pub mod output;
pub mod simple;

/// UI complexity selected by the header toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Simple,
    Advanced,
}

impl Mode {
    #[must_use]
    pub fn from_advanced_flag(advanced: bool) -> Self {
        if advanced {
            Self::Advanced
        } else {
            Self::Simple
        }
    }
}
