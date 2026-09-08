//! Blocking HTTP client wrapper (feature `http-client`).
//!
//! The implementation now lives in `big_os_kit::http_client` (GTK-free leaf,
//! single HTTP-policy home shared with `big-media-data`). Re-exported here so
//! existing `big_app_kit::http_client::*` consumers keep working unchanged.

pub use big_os_kit::http_client::*;
