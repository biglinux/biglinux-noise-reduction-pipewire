#![expect(
    clippy::expect_used,
    reason = "tests require explicit setup invariants"
)]

use super::*;
use crate::config::AppSettings;

#[test]
fn advanced_flag_maps_to_mode() {
    assert_eq!(Mode::from_advanced_flag(true), Mode::Advanced);
    assert_eq!(Mode::from_advanced_flag(false), Mode::Simple);
}

#[test]
fn settings_load_tracker_rejects_stale_completion() {
    let mut tracker = SettingsLoadTracker::default();
    let stale = tracker.begin().expect("first settings load starts");
    let _current = tracker.begin().expect("second settings load starts");

    assert!(!tracker.accept(stale));
}

#[test]
fn settings_load_tracker_accepts_current_completion_only_once() {
    let mut tracker = SettingsLoadTracker::default();
    let current = tracker.begin().expect("settings load starts");
    assert!(tracker.accept(current));

    assert!(!tracker.accept(current));
}

#[test]
fn settings_load_tracker_stops_at_generation_overflow() {
    let mut tracker = SettingsLoadTracker {
        generation: u64::MAX,
        pending: None,
    };

    assert!(tracker.begin().is_none());
}

#[test]
fn a_failed_apply_says_what_happened_without_the_diagnostic_chain() {
    let title = apply_failure_title();

    assert!(
        title.starts_with("Could not apply the audio settings"),
        "{title}"
    );
    assert!(title.contains("choices are saved"), "{title}");
    // Units, argv and exit codes belong in the journal, never in the banner.
    for internal in ["systemctl", "subprocess", "Some(", "[\""] {
        assert!(!title.contains(internal), "{internal} leaked into {title}");
    }
}

#[test]
fn startup_apply_must_complete_before_health_can_start() {
    let mut tracker = ApplyTracker::default();
    let startup = tracker
        .mark_current_ready()
        .expect("startup apply starts immediately");

    assert!(tracker.begin_health().is_none());
    let completion = tracker
        .complete_apply(startup)
        .expect("startup completion matches the active worker");
    assert!(completion.is_settled);
    tracker.record_applied(startup);

    let health = tracker
        .begin_health()
        .expect("health starts only after the startup apply");
    assert_eq!(health.revision, startup.revision());
    assert_ne!(health.generation, startup.generation());
}

#[test]
fn edits_during_apply_coalesce_to_the_latest_revision() {
    let mut tracker = ApplyTracker::default();
    let startup = tracker
        .mark_current_ready()
        .expect("startup apply starts immediately");
    let stale = tracker.settings_changed();
    let latest = tracker.settings_changed();

    assert!(tracker.mark_ready(stale).is_none());
    assert!(tracker.mark_ready(latest).is_none());
    let completion = tracker
        .complete_apply(startup)
        .expect("startup completion is current worker");
    let next = completion.next.expect("latest edit starts next");

    assert!(!completion.is_settled);
    assert_eq!(next.revision(), latest);
}

#[test]
fn stale_and_duplicate_apply_completions_do_not_release_the_active_worker() {
    let mut tracker = ApplyTracker::default();
    let active = tracker
        .mark_current_ready()
        .expect("startup apply starts immediately");
    let stale = ApplyRequest::new(ApplyRevision::default(), active.generation().next());

    assert!(tracker.complete_apply(stale).is_none());
    assert!(tracker.has_active_apply());
    assert!(tracker.complete_apply(active).is_some());
    assert!(tracker.complete_apply(active).is_none());
}

#[test]
fn settings_change_invalidates_health_and_runs_apply_after_it_finishes() {
    let mut tracker = ApplyTracker::default();
    let startup = tracker
        .mark_current_ready()
        .expect("startup apply starts immediately");
    tracker
        .complete_apply(startup)
        .expect("startup apply completes");
    tracker.record_applied(startup);
    let health = tracker.begin_health().expect("health starts");
    let latest = tracker.settings_changed();

    assert!(tracker.mark_ready(latest).is_none());
    let completion = tracker
        .complete_health(health)
        .expect("health completion releases the single worker lane");

    assert!(!completion.is_current);
    assert_eq!(completion.next.map(ApplyRequest::revision), Some(latest));
    assert!(tracker.complete_health(health).is_none());
}

#[test]
fn dirty_revision_blocks_health_until_its_apply_succeeds() {
    let mut tracker = ApplyTracker::default();
    let startup = tracker
        .mark_current_ready()
        .expect("startup apply starts immediately");
    tracker
        .complete_apply(startup)
        .expect("startup apply completes");
    tracker.record_applied(startup);
    let dirty = tracker.settings_changed();

    assert!(tracker.begin_health().is_none());
    let apply = tracker
        .mark_ready(dirty)
        .expect("dirty revision starts after debounce");
    tracker
        .complete_apply(apply)
        .expect("dirty apply completion is accepted");
    assert!(tracker.begin_health().is_none());

    tracker.record_applied(apply);
    assert!(tracker.begin_health().is_some());
}

#[test]
fn close_revision_waits_for_the_older_apply() {
    let mut tracker = ApplyTracker::default();
    let active = tracker
        .mark_current_ready()
        .expect("startup apply starts immediately");
    let close_revision = tracker.settings_changed();
    assert!(tracker.mark_current_ready().is_none());

    let older = tracker
        .complete_apply(active)
        .expect("older apply completion is accepted");
    let close = older.next.expect("close apply starts next");
    assert_eq!(close.revision(), close_revision);
    assert!(!older.is_settled);

    let final_completion = tracker
        .complete_apply(close)
        .expect("close completion matches latest revision");
    assert!(final_completion.is_settled);
}

#[test]
fn actual_window_dimension_wins_and_invalid_values_use_persisted_size() {
    assert_eq!(current_window_dimension(1180, 720), 1180);
    assert_eq!(current_window_dimension(0, 720), 720);
    assert_eq!(current_window_dimension(-1, 700), 700);
}

#[test]
fn advanced_toggle_persists_into_settings() {
    let state = AppState::new(AppSettings::default());
    state.mutate(|s| s.ui.show_advanced = true);
    assert!(state.settings().ui.show_advanced);
    state.mutate(|s| s.ui.show_advanced = false);
    assert!(!state.settings().ui.show_advanced);
}
