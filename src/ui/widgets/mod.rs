//! Re-usable GTK widgets (as opposed to views, which are page-level
//! assemblies). Each widget is self-contained and may carry its own
//! animation / state so the main window can drop it in without plumbing
//! lifecycles by hand.

pub mod didactic;
pub mod eq_card;
pub mod model_picker;
pub mod source_picker;
pub mod spectrum;
pub mod wp_override_warning;
