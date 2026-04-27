//! Runtime services that bridge the domain model to the PipeWire graph.
//!
//! The domain layer (`crate::config`, `crate::pipeline`) is pure data: it
//! computes what *should* be on disk and describes the filter graph we
//! *want*. This module hosts the moving parts that make those intentions
//! real: talking to the PipeWire daemon, loading / unloading modules,
//! watching the graph for new application streams, and forwarding live
//! parameter updates.

pub mod audio_monitor;
pub mod loopback;
pub mod pipewire;
