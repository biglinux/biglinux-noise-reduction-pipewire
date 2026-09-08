// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Boot + `/proc` readers for headless leak censuses.
//!
//! The `/proc` readers ([`vmrss_kib`], [`anon_rss_kib`]) are **pure** and unit
//! tested; the loop pump and app boot ([`pump`], [`disable_animations`],
//! [`run_census_app`]) need a GTK display and only run under weston headless.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk::glib;

/// Resident set size (KiB) of this process from `/proc/self/status` `VmRSS`.
///
/// Advisory: a growing-but-bounded GTK cache can inflate it, so pair it with
/// the exact [`leak::WidgetFinalizeCensus`](crate::leak::WidgetFinalizeCensus)
/// for teardown proofs. **pure** — no display needed.
#[must_use]
pub fn vmrss_kib() -> u64 {
    proc_kib_field("/proc/self/status", "VmRSS:")
}

/// Anonymous (heap/mmap/renderer) RSS in KiB for `pid` from
/// `/proc/<pid>/smaps_rollup`.
///
/// This is the number the RSS-slope leak probe samples after each close: it
/// excludes file-backed pages, so it tracks real heap/renderer growth. Returns
/// `0` when the file is unreadable (e.g. the process already exited).
/// **pure** — no display needed.
#[must_use]
pub fn anon_rss_kib(pid: u32) -> u64 {
    proc_kib_field(&format!("/proc/{pid}/smaps_rollup"), "Anonymous:")
}

/// Read the KiB value of a `Key:` field from a `/proc` status-style file.
fn proc_kib_field(path: &str, field: &str) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| {
            contents
                .lines()
                .find_map(|line| line.strip_prefix(field))
                .and_then(|rest| rest.split_whitespace().next().map(str::to_owned))
        })
        .and_then(|kib| kib.parse().ok())
        .unwrap_or(0)
}

/// Drain every pending default-`MainContext` task (iteration cap 10_000).
///
/// A synchronous census loop blocks the main loop, so realize / map / close /
/// finalize would otherwise never run. Call it between census steps. **weston**
/// — meaningful only with a GTK app running.
pub fn pump() {
    let main_context = glib::MainContext::default();
    let mut guard = 0;
    while main_context.pending() && guard < 10_000 {
        main_context.iteration(false);
        guard += 1;
    }
}

/// Turn off GTK animations so window close is synchronous under a headless
/// display (otherwise the close animation defers finalization and false-flags a
/// leak). No-op when GTK settings are not yet available. **weston**.
pub fn disable_animations() {
    if let Some(settings) = relm4::gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }
}

thread_local! {
    /// Exit code the currently running [`run_census_app`] will return. A census
    /// body flags a leak with [`fail_census`]; promotes the leak examples'
    /// `static EXIT_CODE`.
    static CENSUS_EXIT_CODE: Cell<i32> = const { Cell::new(0) };
}

/// Flag the running census as failed so [`run_census_app`] returns `exit_code`
/// (`3` = leak, by the leak-example convention) once the app quits.
pub fn fail_census(exit_code: i32) {
    CENSUS_EXIT_CODE.with(|code| code.set(exit_code));
}

/// Boot a minimal libadwaita census app, present one window, run `body` against
/// it on the first idle, then quit — returning the census exit code (`0` clean,
/// `3` when `body` called [`fail_census`]). **weston**.
///
/// Mirrors the `leak_soak` / `leak_cycle` boot dance (register the relm4 global
/// application, drive the underlying `gtk::Application` directly) so app-level
/// census `#[ignore]` tests and the `mem-leak-smoke-template.sh` runner share
/// one entry point instead of re-rolling `RelmApp` boilerplate.
pub fn run_census_app<F>(app_id: &str, body: F) -> i32
where
    F: FnOnce(&adw::ApplicationWindow) + 'static,
{
    CENSUS_EXIT_CODE.with(|code| code.set(0));

    // Registers relm4's global application (relm4/adw widgets need it); we then
    // drive the underlying gtk::Application ourselves, exactly like leak_cycle.
    let _relm_app = relm4::RelmApp::<()>::new(app_id);
    let application = relm4::main_application();

    let body_slot = Rc::new(RefCell::new(Some(body)));
    let application_for_activate = application.clone();
    application.connect_activate(move |_| {
        disable_animations();
        let window = adw::ApplicationWindow::builder()
            .application(&application_for_activate)
            .default_width(480)
            .default_height(320)
            .title("big-testkit census")
            .build();
        window.present();
        pump();

        let hold = application_for_activate.hold();
        let body = body_slot.borrow_mut().take();
        let application_for_idle = application_for_activate.clone();
        glib::idle_add_local_once(move || {
            let _hold = hold; // released as this idle returns, after quit()
            if let Some(body) = body {
                body(&window);
            }
            application_for_idle.quit();
        });
    });

    application.run_with_args::<&str>(&[]);
    CENSUS_EXIT_CODE.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vmrss_of_self_is_nonzero() {
        // Our own process always has resident pages.
        assert!(vmrss_kib() > 0, "VmRSS of the test process should be > 0");
    }

    #[test]
    fn anon_rss_of_self_is_nonzero() {
        // smaps_rollup Anonymous covers our heap/stack — always > 0 for self.
        assert!(
            anon_rss_kib(std::process::id()) > 0,
            "Anonymous RSS of the test process should be > 0"
        );
    }

    #[test]
    fn anon_rss_of_dead_pid_is_zero() {
        // No such pid → unreadable file → 0 (never panics).
        assert_eq!(anon_rss_kib(u32::MAX), 0);
    }
}
