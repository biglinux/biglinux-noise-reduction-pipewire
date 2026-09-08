//! URL parsing and validation helpers.
//!
//! The implementation now lives in `big_os_kit::url` (GTK-free leaf, shared
//! with `big-media-data`). Re-exported here so existing `big_app_kit::url::*`
//! consumers keep working unchanged.

pub use big_os_kit::url::*;
