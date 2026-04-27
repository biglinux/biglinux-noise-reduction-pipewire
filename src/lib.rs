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

/// GTK4 / libadwaita GUI.
pub mod ui;

/// End-to-end diagnostic probe (mirrors `biglinux-microphone-cli doctor`).
pub mod diagnostics;
