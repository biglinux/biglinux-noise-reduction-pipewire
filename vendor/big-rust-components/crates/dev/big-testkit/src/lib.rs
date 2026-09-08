// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Dev-only test harness for the BigLinux Rust suite.
//!
//! Consolidates the leak-checking machinery that was copy-pasted across the
//! standalone `leak_soak` / `leak_cycle` examples and the `mem-leak-*.sh`
//! shell probes into one dev-dependency crate (`publish = false`).
//!
//! Helpers are split by what they need to run:
//!
//! - **pure** — plain functions and math (`/proc` readers, [`leak::RssSlope`],
//!   [`leak::Subscription`], [`pixel`] region math). They run under an ordinary
//!   `cargo nextest run`, no display, and carry the crate's unit tests.
//! - **weston** — anything that boots GTK or drives a widget lifecycle
//!   ([`harness::pump`], [`harness::run_census_app`], [`leak::WidgetFinalizeCensus`],
//!   [`leak::assert_finalizes`]) or the real main loop
//!   ([`stall::measure_max_main_loop_gap_ms`], [`stall::assert_main_loop_responsive`]).
//!   Live-GTK tests that use these are `#[ignore]` and run via
//!   `cargo nextest run --run-ignored` under
//!   `weston --backend=headless --renderer=pixman` (host-safe software GTK).
//! - **VM** — screenshot capture (spectacle / weston-screenshooter), AT-SPI
//!   driving, and GL/kwin paths are *not* in this crate; they belong to a
//!   separate follow-up wave on a disposable VM. This crate only does the pure
//!   pixel math on a PNG a VM step already captured.

pub mod harness;
pub mod leak;
pub mod pixel;
pub mod stall;
