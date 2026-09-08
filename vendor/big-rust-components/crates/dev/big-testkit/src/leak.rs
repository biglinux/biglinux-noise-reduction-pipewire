// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Teardown / leak assertions.
//!
//! Two families:
//!
//! - **exact, weston** — [`WidgetFinalizeCensus`] and [`assert_finalizes`]
//!   prove a `GObject` actually finalized after its window closed (a strong
//!   widget⇄handler cycle keeps it alive). Needs a running GTK app.
//! - **pure** — [`RssSlope`] ports the advisory RSS-slope verdict from
//!   `leak_cycle::report` and the `mem-leak-check.sh` awk; [`Subscription`],
//!   [`assert_provider_slot_empty`], and [`assert_listeners_pruned`] are plain
//!   drop/state checks. All unit tested here, no display.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use big_app_kit::settings_store::BigSettingsStore;
use glib::prelude::*;
use relm4::gtk;

/// Least-squares slope is only computed over samples past this warm-up count,
/// so the first-run allocations (font/renderer/icon caches filling) do not
/// masquerade as a per-cycle leak.
const WARMUP: usize = 5;

// ---------------------------------------------------------------------------
// Exact finalization census (weston)
// ---------------------------------------------------------------------------

/// One or more tracked widgets outlived their close — a teardown leak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leak {
    /// How many cycles left a window alive after close.
    pub survivors: usize,
    /// Total cycles run.
    pub cycles: usize,
}

impl std::fmt::Display for Leak {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}/{} window(s) not finalized after close — incomplete widget teardown (INVARIANTS H9)",
            self.survivors, self.cycles
        )
    }
}

impl std::error::Error for Leak {}

/// Finalization census: log a `new` line when a widget is tracked and a `fin`
/// line when it is finalized, and remember which labels never finalized.
///
/// The decisive GTK leak check — a `GObject` only finalizes when no Rust
/// closure still holds it, so a label seen `new` but never `fin` names the exact
/// widget a reference cycle pinned. Output matches the `[mem_audit] object
/// new/fin <label>` format the `mem-leak-*.sh` probes grep for. **weston** —
/// needs live `GObject`s and a main loop to drive finalization.
#[derive(Default, Clone)]
pub struct WidgetFinalizeCensus {
    live: Rc<RefCell<BTreeMap<String, i64>>>,
}

impl WidgetFinalizeCensus {
    /// New empty census.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Track `object`: emit `new` now and `fin` when it is finalized.
    pub fn track(&self, label: impl Into<String>, object: &impl IsA<glib::Object>) {
        let label = label.into();
        eprintln!("[mem_audit] object new    {label}");
        *self.live.borrow_mut().entry(label.clone()).or_insert(0) += 1;
        let live = self.live.clone();
        object.add_weak_ref_notify_local(move || {
            eprintln!("[mem_audit] object fin    {label}");
            if let Some(count) = live.borrow_mut().get_mut(&label) {
                *count -= 1;
            }
        });
    }

    /// Labels tracked `new` more often than they were finalized — the leaked
    /// widgets. Empty slice ⇒ clean teardown.
    #[must_use]
    pub fn survivors(&self) -> Vec<String> {
        self.live
            .borrow()
            .iter()
            .filter(|&(_, &count)| count > 0)
            .map(|(label, _)| label.clone())
            .collect()
    }
}

/// Run `build_present_close` `cycles` times; each call must build, present, and
/// close one window and return a `WeakRef` to it, dropping every strong handle
/// before it returns. After draining the loop, any window still upgradeable
/// leaked. Generalizes `leak_soak::cycle_finalizes`. **weston**.
///
/// # Errors
///
/// Returns [`Leak`] when one or more windows survived their close.
pub fn assert_finalizes(
    cycles: usize,
    build_present_close: impl Fn() -> glib::WeakRef<gtk::Window>,
) -> Result<(), Leak> {
    let mut survivors = 0;
    for _ in 0..cycles {
        let weak = build_present_close();
        // Drain deferred finalize/idle work, then census the weak ref.
        crate::harness::pump();
        crate::harness::pump();
        if weak.upgrade().is_some() {
            survivors += 1;
        }
    }
    if survivors > 0 {
        Err(Leak { survivors, cycles })
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// RSS-slope verdict (pure)
// ---------------------------------------------------------------------------

/// Advisory verdict for a run of post-close RSS samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Fewer than three post-warm-up samples — cannot judge.
    Inconclusive,
    /// Slope within threshold: a flat plateau, clean.
    Pass,
    /// Growing but the late per-cycle delta is far below the early one —
    /// cache warming, not a steady leak.
    Decelerating,
    /// Steady growth above threshold — a leak is suspected.
    Leak,
}

impl Verdict {
    /// Only [`Verdict::Leak`] fails a gate; `Decelerating` / `Inconclusive` are
    /// advisory non-failures, matching the shell probe's `exit 0`.
    #[must_use]
    pub fn is_leak(self) -> bool {
        matches!(self, Verdict::Leak)
    }
}

/// Post-close RSS samples plus the leak verdict math.
///
/// Ports `leak_cycle::report` (warm-up drop, plateau threshold) and the
/// `mem-leak-check.sh` awk (least-squares slope + deceleration heuristic) into
/// one unit-tested place. **pure** — feed it sampled KiB, no display.
#[derive(Debug, Default, Clone)]
pub struct RssSlope {
    samples: Vec<u64>,
}

impl RssSlope {
    /// Empty sampler.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sampler seeded from an existing series (one KiB reading per cycle).
    #[must_use]
    pub fn from_samples(samples: &[u64]) -> Self {
        Self {
            samples: samples.to_vec(),
        }
    }

    /// Append one post-close RSS reading (KiB).
    pub fn push(&mut self, rss_kib: u64) {
        self.samples.push(rss_kib);
    }

    /// Every recorded sample, in cycle order.
    #[must_use]
    pub fn samples(&self) -> &[u64] {
        &self.samples
    }

    /// Samples past the warm-up window; falls back to all samples when the
    /// series is too short to drop a warm-up.
    fn post_warmup(&self) -> &[u64] {
        if self.samples.len() > WARMUP + 2 {
            &self.samples[WARMUP..]
        } else {
            &self.samples
        }
    }

    /// Least-squares slope in KiB/cycle over the post-warm-up samples (`0.0`
    /// with fewer than two).
    #[must_use]
    pub fn slope_kib_per_cycle(&self) -> f64 {
        least_squares_slope(self.post_warmup())
    }

    /// Classify the series: slope within `threshold_kib` ⇒ [`Verdict::Pass`];
    /// else a late/early delta ratio below `decel_ratio` ⇒
    /// [`Verdict::Decelerating`] (cache warming); else [`Verdict::Leak`].
    /// Fewer than three post-warm-up samples ⇒ [`Verdict::Inconclusive`].
    ///
    /// `threshold_kib` mirrors `--threshold-kb` (512 in the probe, 50 in the
    /// per-cycle example); `decel_ratio` is the awk's `0.4`.
    #[must_use]
    pub fn verdict(&self, threshold_kib: f64, decel_ratio: f64) -> Verdict {
        let series = self.post_warmup();
        let n = series.len();
        if n < 3 {
            return Verdict::Inconclusive;
        }
        let slope = least_squares_slope(series);
        if slope <= threshold_kib {
            return Verdict::Pass;
        }
        let (early_delta, late_delta) = split_half_deltas(series);
        if late_delta < early_delta * decel_ratio {
            Verdict::Decelerating
        } else {
            Verdict::Leak
        }
    }
}

/// Least-squares slope of `series` against `x = 1..=n` (`0.0` if `n < 2`).
fn least_squares_slope(series: &[u64]) -> f64 {
    let n = series.len();
    if n < 2 {
        return 0.0;
    }
    let n_f = n as f64;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xx = 0.0;
    let mut sum_xy = 0.0;
    for (index, &sample) in series.iter().enumerate() {
        let x = (index + 1) as f64;
        let y = sample as f64;
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }
    let denominator = n_f * sum_xx - sum_x * sum_x;
    if denominator == 0.0 {
        0.0
    } else {
        (n_f * sum_xy - sum_x * sum_y) / denominator
    }
}

/// Average per-cycle delta over the first vs the second half of `series`,
/// exactly as the `mem-leak-check.sh` awk computes `fa` / `la`.
fn split_half_deltas(series: &[u64]) -> (f64, f64) {
    let n = series.len();
    let half = n / 2;
    let delta = |k: usize| series[k] as f64 - series[k - 1] as f64;

    // Early: deltas at 1-based i = 2..=half → 0-based k = 1..=half-1.
    let mut early = 0.0;
    for k in 1..half {
        early += delta(k);
    }
    if half > 1 {
        early /= (half - 1) as f64;
    }

    // Late: deltas at 1-based i = half+1..=n → 0-based k = half..=n-1.
    let mut late = 0.0;
    for k in half..n {
        late += delta(k);
    }
    if n - half > 0 {
        late /= (n - half) as f64;
    }

    (early, late)
}

// ---------------------------------------------------------------------------
// Plain state / drop assertions (pure)
// ---------------------------------------------------------------------------

/// Assert the swap-not-stack CSS provider slot is cleared after teardown.
///
/// `big_relm4_components::theme::inject_palette_css` caches its provider in an
/// `Option<CssProvider>` slot and reuses it; a clean teardown must leave the
/// slot `None`, otherwise a provider stays registered on the `Display` and pins
/// state. **pure** — a plain `Option::is_none` check.
///
/// # Panics
///
/// Panics when `slot` still holds a provider.
pub fn assert_provider_slot_empty(slot: &Option<gtk::CssProvider>) {
    assert!(
        slot.is_none(),
        "CSS provider slot not cleared — the swap-not-stack teardown left a provider on the display (INVARIANTS H9)"
    );
}

/// A `#[must_use]` RAII unsubscribe guard: dropping it runs the stored
/// unsubscribe closure exactly once.
///
/// Generalizes the listener-list-unsubscribe token — a long-lived registry
/// (`Vec<Box<dyn Fn>>`) plus a dialog-scoped subscription accumulates leaked
/// closures unless the subscriber holds a token that unregisters on drop.
/// **pure**.
#[must_use = "dropping the Subscription runs its unsubscribe; bind it to keep the subscription alive"]
pub struct Subscription {
    unsubscribe: Option<Box<dyn FnOnce()>>,
}

impl Subscription {
    /// Wrap an unsubscribe action; it runs when the guard drops.
    pub fn new(unsubscribe: impl FnOnce() + 'static) -> Self {
        Self {
            unsubscribe: Some(Box::new(unsubscribe)),
        }
    }

    /// Unsubscribe now instead of waiting for drop (idempotent).
    pub fn unsubscribe(mut self) {
        self.run();
    }

    fn run(&mut self) {
        if let Some(unsubscribe) = self.unsubscribe.take() {
            unsubscribe();
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.run();
    }
}

/// Assert `store` retains exactly `expected` live change listeners.
///
/// Uses the `#[doc(hidden)]` [`BigSettingsStore::listener_count`] seam (the
/// minimal accessor into the private listener `Vec`). Drop the dialog-scoped
/// subscriptions and fire one notify (stale listeners return `false` and are
/// pruned) before asserting. **pure-ish** — no display, but exercises real
/// store state.
///
/// # Panics
///
/// Panics when the live listener count differs from `expected`.
pub fn assert_listeners_pruned(store: &BigSettingsStore, expected: usize) {
    let live = store.listener_count();
    assert_eq!(
        live, expected,
        "settings store retained {live} listener(s), expected {expected} — a scoped subscription leaked"
    );
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;

    use super::*;

    #[test]
    fn slope_of_clean_line_is_its_gradient() {
        // 5 warm-up samples (dropped) + a perfect +100/cycle line.
        let mut slope = RssSlope::from_samples(&[0, 0, 0, 0, 0]);
        for step in 0..8 {
            slope.push(step * 100);
        }
        assert!(
            (slope.slope_kib_per_cycle() - 100.0).abs() < 1e-6,
            "expected slope 100, got {}",
            slope.slope_kib_per_cycle()
        );
    }

    #[test]
    fn steady_growth_is_a_leak() {
        // Warm-up + steady +100/cycle: over threshold, not decelerating.
        let mut slope = RssSlope::from_samples(&[500; 5]);
        for step in 0..8 {
            slope.push(1000 + step * 100);
        }
        let verdict = slope.verdict(50.0, 0.4);
        assert_eq!(verdict, Verdict::Leak);
        assert!(verdict.is_leak());
    }

    #[test]
    fn decelerating_warmup_is_not_a_leak() {
        // Deltas 500,200,100,50,25,13,6 → late delta ≪ early delta * 0.4.
        let series = [
            1000, 1000, 1000, 1000, 1000, // warm-up
            1000, 1500, 1700, 1800, 1850, 1875, 1888, 1894,
        ];
        let slope = RssSlope::from_samples(&series);
        let verdict = slope.verdict(50.0, 0.4);
        assert_eq!(verdict, Verdict::Decelerating);
        assert!(!verdict.is_leak());
    }

    #[test]
    fn flat_plateau_passes() {
        let slope = RssSlope::from_samples(&[2000; 13]);
        let verdict = slope.verdict(50.0, 0.4);
        assert_eq!(verdict, Verdict::Pass);
        assert!(!verdict.is_leak());
    }

    #[test]
    fn too_few_samples_is_inconclusive() {
        let slope = RssSlope::from_samples(&[100, 200]);
        assert_eq!(slope.verdict(50.0, 0.4), Verdict::Inconclusive);
        assert!(!slope.verdict(50.0, 0.4).is_leak());
    }

    #[test]
    fn subscription_runs_unsub_on_drop() {
        let calls = Rc::new(Cell::new(0));
        let calls_for_unsub = calls.clone();
        let subscription =
            Subscription::new(move || calls_for_unsub.set(calls_for_unsub.get() + 1));
        assert_eq!(calls.get(), 0, "unsub must not run before drop");
        drop(subscription);
        assert_eq!(calls.get(), 1, "unsub runs exactly once on drop");
    }

    #[test]
    fn subscription_explicit_unsubscribe_does_not_double_run() {
        let calls = Rc::new(Cell::new(0));
        let calls_for_unsub = calls.clone();
        let subscription =
            Subscription::new(move || calls_for_unsub.set(calls_for_unsub.get() + 1));
        subscription.unsubscribe(); // consumes + runs, then drops (no second run)
        assert_eq!(
            calls.get(),
            1,
            "explicit unsubscribe runs once, drop does not repeat"
        );
    }

    #[test]
    fn provider_slot_empty_passes_when_none() {
        // Pure None path (constructing a real CssProvider needs a display).
        assert_provider_slot_empty(&None);
    }

    #[test]
    fn listeners_pruned_after_stale_subscriber_dropped() {
        use serde_json::{Map, Value};
        let store = BigSettingsStore::blank(
            Path::new("big-testkit-listener-test.json"),
            "0",
            &Map::<String, Value>::new(),
        );
        // A live listener stays; a stale one (returns false) is pruned on notify.
        store.subscribe(Rc::new(|_| true));
        store.subscribe(Rc::new(|_| false));
        assert_eq!(store.listener_count(), 2);
        store.notify_external_change("*");
        assert_listeners_pruned(&store, 1);
    }

    #[test]
    fn leak_display_reports_survivors_over_cycles() {
        let leak = Leak {
            survivors: 2,
            cycles: 5,
        };
        let message = leak.to_string();
        assert!(message.contains("2/5"), "{message}");
        assert!(message.contains("not finalized"), "{message}");
    }

    #[test]
    fn samples_accessor_returns_the_recorded_series() {
        let slope = RssSlope::from_samples(&[3, 1, 4, 1, 5]);
        assert_eq!(slope.samples(), &[3, 1, 4, 1, 5]);
        let mut pushed = RssSlope::new();
        pushed.push(9);
        pushed.push(7);
        assert_eq!(pushed.samples(), &[9, 7]);
    }

    #[test]
    fn slope_is_taken_over_the_post_warmup_window() {
        // 7 samples: `len > WARMUP + 2` (7 > 7) is FALSE, so the whole series is
        // used — a plunge from the warm-up plateau makes the slope negative. Any
        // off-by-one that drops the warm-up here would see only the rising tail.
        let seven = RssSlope::from_samples(&[1000, 1000, 1000, 1000, 1000, 0, 100]);
        assert!(
            seven.slope_kib_per_cycle() < 0.0,
            "7 samples keep the plateau: {}",
            seven.slope_kib_per_cycle()
        );
        // 8 samples: `8 > 7` is TRUE, so the 5 warm-up samples ARE dropped and the
        // slope is measured over the clean +10/cycle tail only.
        let eight = RssSlope::from_samples(&[1000, 1000, 1000, 1000, 1000, 0, 10, 20]);
        assert_eq!(eight.slope_kib_per_cycle(), 10.0);
    }

    #[test]
    fn least_squares_slope_of_exactly_two_samples() {
        // Exactly two post-warm-up samples still yields a real gradient (the
        // `n < 2` guard must not swallow the two-point case).
        assert_eq!(
            RssSlope::from_samples(&[100, 200]).slope_kib_per_cycle(),
            100.0
        );
    }

    #[test]
    fn verdict_needs_at_least_three_samples_but_no_more() {
        // Exactly three post-warm-up samples is judgeable (not Inconclusive): a
        // flat trio is a clean Pass. Guards `n < 3` against `n <= 3`.
        assert_eq!(
            RssSlope::from_samples(&[2000, 2000, 2000]).verdict(50.0, 0.4),
            Verdict::Pass
        );
    }

    #[test]
    fn verdict_at_the_decel_ratio_boundary_is_a_leak() {
        // early avg delta = 100, late avg delta = 50, decel_ratio = 0.5 ⇒
        // late == early * ratio EXACTLY. The threshold is strict `<`, so equality
        // is NOT decelerating — it is a Leak (guards `<` against `<=`).
        let series = RssSlope::from_samples(&[0, 100, 200, 250, 300, 350]);
        assert_eq!(series.verdict(50.0, 0.5), Verdict::Leak);
    }

    #[test]
    fn verdict_decelerating_series_averages_each_half() {
        // early avg delta = 500 (raw 1000 over half-1 = 2), late avg delta = 150
        // (raw 450 over n-half = 3). late (150) < early*0.4 (200) ⇒ Decelerating.
        // The per-half averaging divisors must be exact: dropping either
        // normalization inflates a delta and flips the verdict to Leak.
        let series = RssSlope::from_samples(&[0, 500, 1000, 1150, 1300, 1450]);
        assert_eq!(series.verdict(50.0, 0.4), Verdict::Decelerating);
    }
}
