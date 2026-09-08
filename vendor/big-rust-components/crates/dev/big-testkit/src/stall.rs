// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Main-loop stall assertion — turns "the interface never freezes" into a test.
//!
//! # Detection model
//!
//! A watchdog [`glib::timeout_add_local`] is armed to fire every `tick_ms` on
//! the default `MainContext`. Each fire records the monotonic elapsed time; the
//! gap between two successive fires is the wall time the main loop went
//! *undispatched*. A responsive loop dispatches the watchdog on schedule, so
//! every gap is ≈ `tick_ms`. A blocked loop cannot dispatch the watchdog at all
//! while it is blocked, so the first fire *after* the block records a gap equal
//! to the block duration — the observed stall.
//!
//! The measurement window has three phases so a blocked span is always framed by
//! a real prior tick and a real later tick:
//!
//! 1. **prime** — spin the loop for a few ticks so a baseline tick is recorded
//!    *before* `body` runs (the first tick only sets the baseline; a stall is
//!    the gap *from* it).
//! 2. **body** — run the operation. Offloaded work returns to the loop and lets
//!    the watchdog keep ticking (small gaps); a synchronous block on the main
//!    thread freezes the loop (no ticks until it returns).
//! 3. **settle** — spin the loop again so the first post-`body` tick records the
//!    stall gap and we confirm the loop recovered.
//!
//! # The offload pattern this proves
//!
//! `body` runs on the *same* thread as the main loop, so a fully general
//! "assert any body keeps the loop responsive" is impossible: a body that
//! blocks the thread blocks the watchdog too. That asymmetry is exactly the
//! product contract. The F6 task kit (`gio::spawn_blocking` /
//! `big_relm4_components::task::spawn_result_bound`) moves heavy work *off* the
//! main thread and delivers the result back *on* it, so the loop keeps
//! dispatching — [`assert_main_loop_responsive`] **passes**. The same helper
//! run against a deliberate `std::thread::sleep` on the main thread **fails**
//! with the observed gap. Offloaded ⇒ responsive; main-thread block ⇒ stall.
//!
//! ```no_run
//! use big_testkit::stall::{assert_main_loop_responsive, pump_until};
//! use relm4::gtk::{gio, glib};
//! use std::cell::Cell;
//! use std::rc::Rc;
//! use std::time::Duration;
//!
//! // Heavy work OFFLOADED via the F6 pattern keeps the loop responsive.
//! assert_main_loop_responsive(150, || {
//!     let done = Rc::new(Cell::new(false));
//!     let done_for_cb = Rc::clone(&done);
//!     glib::spawn_future_local(async move {
//!         let _ = gio::spawn_blocking(|| std::thread::sleep(Duration::from_millis(500))).await;
//!         done_for_cb.set(true);
//!     });
//!     pump_until(Duration::from_secs(3), || done.get());
//! })
//! .expect("offloaded work must keep the loop responsive");
//! ```
//!
//! **Confidence:** the primitive reliably distinguishes offloaded work from a
//! main-thread block (the two differ by an order of magnitude — ≈ `tick_ms` vs
//! the whole block). It is a wall-clock timing probe, so it is *not* a formal
//! guarantee against sub-`tick_ms` micro-stalls, and it is deliberately kept off
//! the deterministic nextest floor (`#[ignore]`, run under weston) so scheduler
//! jitter never flakes the gate.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use relm4::gtk::glib;

/// The main loop stalled longer than the allowed budget during the measured
/// operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stall {
    /// Largest observed gap between watchdog ticks (ms) — the stall duration.
    pub observed_gap_ms: u64,
    /// The budget that was exceeded (ms).
    pub max_stall_ms: u64,
}

impl std::fmt::Display for Stall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "main loop stalled {}ms (budget {}ms) — a blocking operation froze the GLib main context; offload it (gio::spawn_blocking / spawn_result_bound) so the loop stays responsive",
            self.observed_gap_ms, self.max_stall_ms
        )
    }
}

impl std::error::Error for Stall {}

/// Pure max-gap accumulator — the watchdog timing math without a clock.
///
/// The watchdog feeds it the monotonic elapsed time of each tick; it tracks the
/// largest gap between successive ticks. Keeping the clock at the call boundary
/// (the watchdog captures `Instant::now()`) leaves this **pure** and unit
/// tested with fixed samples, no display.
///
/// # Examples
///
/// ```
/// use big_testkit::stall::GapAccumulator;
/// use std::time::Duration;
///
/// let mut accumulator = GapAccumulator::new();
/// accumulator.record(Duration::from_millis(0));
/// accumulator.record(Duration::from_millis(16)); // one frame — responsive
/// accumulator.record(Duration::from_millis(516)); // a 500 ms stall
/// assert_eq!(accumulator.max_gap_ms(), 500);
/// ```
#[derive(Debug, Default, Clone)]
pub struct GapAccumulator {
    last: Option<Duration>,
    max_gap: Duration,
}

impl GapAccumulator {
    /// New accumulator with no recorded ticks.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a tick at `elapsed` since the measurement start. The gap from the
    /// previous tick updates the running max; the very first tick only sets the
    /// baseline (there is nothing to gap against yet).
    pub fn record(&mut self, elapsed: Duration) {
        if let Some(last) = self.last {
            let gap = elapsed.saturating_sub(last);
            if gap > self.max_gap {
                self.max_gap = gap;
            }
        }
        self.last = Some(elapsed);
    }

    /// Largest gap observed between two successive ticks.
    #[must_use]
    pub fn max_gap(&self) -> Duration {
        self.max_gap
    }

    /// [`Self::max_gap`] in whole milliseconds.
    #[must_use]
    pub fn max_gap_ms(&self) -> u64 {
        self.max_gap.as_millis() as u64
    }
}

/// Ticks spun before and after `body` so a stall is always framed by a real
/// prior tick (prime) and a real later tick (settle).
const FRAME_TICKS: u32 = 3;

/// Run `body` while a watchdog ticks the main loop every `tick_ms`; return the
/// maximum observed gap (ms) between successive ticks.
///
/// If `body` blocks the main context (e.g. sync I/O on the main thread), ticks
/// are delayed and the returned gap exceeds `tick_ms` by the block duration. If
/// `body` offloads its heavy work and returns to the loop, ticks stay on
/// schedule and the gap stays ≈ `tick_ms`. See the [module docs](self) for the
/// detection model. **weston** — needs a running GLib main loop.
#[must_use]
pub fn measure_max_main_loop_gap_ms(tick_ms: u64, body: impl FnOnce()) -> u64 {
    let tick = Duration::from_millis(tick_ms.max(1));
    let context = glib::MainContext::default();
    let start = Instant::now();
    let accumulator = Rc::new(RefCell::new(GapAccumulator::new()));

    let accumulator_for_tick = Rc::clone(&accumulator);
    let watchdog = glib::timeout_add_local(tick, move || {
        accumulator_for_tick.borrow_mut().record(start.elapsed());
        glib::ControlFlow::Continue
    });

    // prime: baseline tick(s) before body so a later stall is a gap from a real
    // prior tick, not just the accumulator's first (baseline-only) record.
    iterate_for(&context, tick * FRAME_TICKS);

    body();

    // settle: the first post-body tick records the stall gap; further ticks
    // confirm the loop recovered to a small cadence.
    iterate_for(&context, tick * FRAME_TICKS);

    watchdog.remove();
    accumulator.borrow().max_gap_ms()
}

/// Assert the main loop stayed responsive (max gap ≤ `max_stall_ms`) while
/// `body` ran; otherwise fail with the observed gap.
///
/// The watchdog tick is derived from the budget
/// (`tick_ms = (max_stall_ms / 4).clamp(5, 50)`) so the baseline cadence is well
/// under the budget while still resolving a real stall. Pair with the F6 offload
/// pattern: spawn heavy work off the main thread so this passes; a deliberate
/// main-thread block trips it. See the [module docs](self). **weston**.
///
/// # Errors
///
/// Returns [`Stall`] when the largest observed inter-tick gap exceeded
/// `max_stall_ms`.
pub fn assert_main_loop_responsive(max_stall_ms: u64, body: impl FnOnce()) -> Result<(), Stall> {
    let tick_ms = (max_stall_ms / 4).clamp(5, 50);
    let observed_gap_ms = measure_max_main_loop_gap_ms(tick_ms, body);
    if observed_gap_ms <= max_stall_ms {
        Ok(())
    } else {
        Err(Stall {
            observed_gap_ms,
            max_stall_ms,
        })
    }
}

/// Iterate the default main context until `is_done` returns true or `timeout`
/// elapses; return whether `is_done` fired in time.
///
/// Blocking iteration, so an installed watchdog (or the awaited work waking the
/// loop) keeps it turning. This is how the offload pattern drives the loop to
/// the operation's completion *inside* a [`measure_max_main_loop_gap_ms`] body:
/// the loop keeps dispatching the watchdog (responsive) while the worker thread
/// does the heavy work. **weston** — needs a running GLib main loop.
pub fn pump_until(timeout: Duration, is_done: impl Fn() -> bool) -> bool {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + timeout;
    while !is_done() && Instant::now() < deadline {
        context.iteration(true);
    }
    is_done()
}

/// Spin `context` (blocking iteration, so the watchdog wakes it) until `window`
/// has elapsed.
fn iterate_for(context: &glib::MainContext, window: Duration) {
    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        context.iteration(true);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use relm4::gtk::{self, gio};

    use super::*;

    // ---- pure gap math (no display) --------------------------------------

    #[test]
    fn max_gap_is_the_largest_inter_tick_delta() {
        let mut accumulator = GapAccumulator::new();
        for ms in [0u64, 10, 20, 30, 530, 540] {
            accumulator.record(Duration::from_millis(ms));
        }
        // 30 -> 530 is the outlier gap; the steady 10 ms cadence is ignored.
        assert_eq!(accumulator.max_gap_ms(), 500);
    }

    #[test]
    fn first_tick_only_sets_a_baseline() {
        let mut accumulator = GapAccumulator::new();
        accumulator.record(Duration::from_millis(1000)); // large elapsed, but first
        assert_eq!(
            accumulator.max_gap_ms(),
            0,
            "first tick must not count as a gap"
        );
        accumulator.record(Duration::from_millis(1005));
        assert_eq!(accumulator.max_gap_ms(), 5);
    }

    #[test]
    fn stall_display_names_gap_and_budget() {
        let stall = Stall {
            observed_gap_ms: 600,
            max_stall_ms: 100,
        };
        let message = stall.to_string();
        assert!(message.contains("600"), "{message}");
        assert!(message.contains("100"), "{message}");
    }

    // ---- live main-loop timing (weston) ----------------------------------

    #[test]
    #[ignore = "weston: drives the real GLib main loop with wall-clock timing"]
    fn offloaded_heavy_op_keeps_loop_responsive() {
        // POSITIVE: the F6 offload pattern — heavy work on a worker thread,
        // result delivered back on the loop. The loop keeps dispatching, so the
        // observed gap stays ≈ tick and the stall assertion PASSES.
        if gtk::init().is_err() {
            eprintln!("skip: no display for gtk::init()");
            return;
        }
        let gap_ms = measure_max_main_loop_gap_ms(37, || {
            let done = Rc::new(Cell::new(false));
            let done_for_cb = Rc::clone(&done);
            glib::spawn_future_local(async move {
                let _ = gio::spawn_blocking(|| {
                    // Heavy, but OFF the main thread — the loop stays free.
                    std::thread::sleep(Duration::from_millis(500));
                    99u32
                })
                .await;
                done_for_cb.set(true);
            });
            // Drive the loop to the result; the watchdog keeps ticking meanwhile.
            pump_until(Duration::from_secs(5), || done.get());
        });
        eprintln!("[stall] offloaded 500 ms op: max main-loop gap = {gap_ms} ms (budget 150)");
        // Same condition assert_main_loop_responsive checks — offloaded ⇒ Ok.
        assert!(
            gap_ms <= 150,
            "offloaded heavy op should keep the loop responsive, gap {gap_ms} ms > 150 ms budget"
        );
    }

    #[test]
    #[ignore = "weston: drives the real GLib main loop with wall-clock timing"]
    fn main_thread_block_trips_the_stall_assertion() {
        // NEGATIVE: the same helper, but the heavy op runs ON the main thread.
        // The loop is frozen for the whole sleep, so the assertion FAILS with a
        // gap reflecting the block — "never freezes" is now a failing test.
        if gtk::init().is_err() {
            eprintln!("skip: no display for gtk::init()");
            return;
        }
        let result = assert_main_loop_responsive(150, || {
            std::thread::sleep(Duration::from_millis(500));
        });
        let stall = result.expect_err("a 500 ms main-thread block must trip the stall assertion");
        eprintln!(
            "[stall] main-thread 500 ms block: observed gap = {} ms (budget {})",
            stall.observed_gap_ms, stall.max_stall_ms
        );
        assert!(
            stall.observed_gap_ms >= 400,
            "observed gap {} should reflect the ~500 ms main-thread block",
            stall.observed_gap_ms
        );
    }
}
