#[test]
fn user_units_resolve_configuration_in_the_process_environment() {
    for (unit, name) in [
        (
            include_str!("../usr/lib/systemd/user/biglinux-microphone-mic.service"),
            "mic",
        ),
        (
            include_str!("../usr/lib/systemd/user/biglinux-microphone-aec.service"),
            "aec",
        ),
        (
            include_str!("../usr/lib/systemd/user/biglinux-microphone-output.service"),
            "output",
        ),
    ] {
        assert!(!unit.contains("%h/.config"));
        assert!(unit.contains(&format!("--check-config @config/{name}.args")));
    }
}

/// With every filter off, no unit of ours may be left running. The loaders were
/// already demand-started; these two were the ones that autostarted regardless,
/// and an `[Install]` section is all it takes to put them back in every session
/// of every user — including the ones who never turned the feature on.
#[test]
fn feature_daemons_are_started_by_the_reconciler_not_by_login() {
    for (unit, name) in [
        (
            include_str!("../usr/lib/systemd/user/biglinux-microphone-agc.service"),
            "agc",
        ),
        (
            include_str!("../usr/lib/systemd/user/biglinux-microphone-route.service"),
            "route",
        ),
    ] {
        // Directives only, so the comment explaining the absence does not read
        // as the thing it is explaining.
        let installs = unit
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#'))
            .any(|line| line == "[Install]" || line.starts_with("WantedBy="));
        assert!(
            !installs,
            "{name}.service must not install itself into a target"
        );
    }
}

/// The reconcile unit reacts to settings changes through systemd's own inotify
/// instead of a thread of ours. Both halves are load-bearing: a `Type=simple`
/// service would stay resident, and without the path unit an edit would only be
/// applied at the next login.
#[test]
fn the_reconciler_runs_once_and_is_woken_by_the_settings_file() {
    let service = include_str!("../usr/lib/systemd/user/biglinux-microphone.service");
    assert!(service.contains("Type=oneshot"));
    assert!(service.contains("biglinux-microphone-cli autostart"));
    assert!(!service.contains("RemainAfterExit"));

    let path = include_str!("../usr/lib/systemd/user/biglinux-microphone.path");
    // Watching the directory instead would also fire for the temporary file the
    // atomic save writes beside the target, and for every other file kept there.
    assert!(path.contains("PathChanged=%h/.config/biglinux-microphone/settings.json"));
    assert!(path.contains("Unit=biglinux-microphone.service"));
    assert!(path.contains("WantedBy=default.target"));
}

/// A start limit on a path-triggered unit counts the user, not the fault.
///
/// systemd counts every start against the limit, and the path unit starts this
/// one on each settings change — so a burst sized for a crash loop is spent by
/// somebody changing a handful of settings, and from then on systemd refuses to
/// start it and their edits stop being applied. Observed exactly that: six
/// changes in a row ended in `start-limit-hit`, with the filter chain left
/// running for a filter the user had switched off.
#[test]
fn settings_changes_are_never_counted_against_a_start_limit() {
    let service = include_str!("../usr/lib/systemd/user/biglinux-microphone.service");
    let limit = service
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.strip_prefix("StartLimitIntervalSec="));
    assert_eq!(
        limit,
        Some("0"),
        "the reconcile unit must disable the start limit, not size it"
    );
    assert!(
        !service
            .lines()
            .map(str::trim)
            .any(|l| l.starts_with("StartLimitBurst=")),
        "a burst is meaningless once the interval is 0, and invites re-sizing it"
    );

    // Runaway triggers are bounded on the trigger instead, where a change the
    // user made is never the thing being refused.
    let path = include_str!("../usr/lib/systemd/user/biglinux-microphone.path");
    assert!(path.contains("TriggerLimitIntervalSec="));
    assert!(path.contains("TriggerLimitBurst="));
}
