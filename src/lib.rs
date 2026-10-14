//! `biglinux-microphone` — AI-powered microphone noise reduction for PipeWire.
//!
//! This crate exposes the domain logic (settings model, filter-chain generator,
//! PipeWire graph manipulation, audio monitor) used by both the GTK4 GUI
//! (`biglinux-microphone`) and the CLI (`biglinux-microphone-cli`).
//!
//! Stage 1 delivers the persistent configuration layer. Remaining stages —
//! pipeline generator, PipeWire service, audio monitor, UI — will be added in
//! subsequent modules.

/// Persistent application settings: typed model + JSON load/save.
pub mod config;

/// PipeWire / WirePlumber filter-chain and routing configuration generator.
pub mod pipeline;

/// Runtime services: PipeWire client, filter-chain lifecycle, audio monitor.
pub mod services;

/// End-to-end diagnostic probe (mirrors `biglinux-microphone-cli doctor`).
pub mod diagnostics;

/// Microphone auto-gain **policy** (no PCM, no hardware I/O; off by default).
pub mod gain_policy;

/// Automatic mic-level control loop (off-RT; drives the policy + source volume).
pub mod agc;
