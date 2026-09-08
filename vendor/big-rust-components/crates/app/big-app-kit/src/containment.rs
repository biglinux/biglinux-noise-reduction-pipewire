// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Component-fault containment for multi-app host processes (big-host).
//!
//! big-host runs the shell plus every `process_policy = "shared"` module in a
//! SINGLE process. An unguarded Rust panic in any of them kills the whole
//! desktop (shell + every open app), and the BigModule ABI's fault containment
//! only wraps module ENTRY calls — not the glib callbacks where most UI logic
//! runs. These helpers close that gap at the callback boundary.
//!
//! Empirics (big-host `panic-probe`, 2026-06; re-confirmed big-shell raw-idle
//! probe, 2026-06-24 on the test VM):
//! - A panic inside a Relm4 `update()` unwinds into the glib executor task and
//!   dies there — the PROCESS survives, but the component runtime is dead and
//!   its window stays orphaned on screen. Wrap with
//!   [`contain_component_update`](crate::containment::contain_component_update)
//!   to log it and reap the window cleanly.
//! - A panic that ESCAPES a raw glib callback (timeout / signal trampoline)
//!   unwinds across the C boundary → immediate process ABORT.
//! - BUT a panic CAUGHT inside the callback body — before it reaches the C
//!   trampoline — is fully contained: the process survives and the main loop
//!   keeps running. [`guard`](crate::containment::guard) is that catch. (Verified: a deliberate
//!   `panic!` inside a guarded `idle_add_local` body left big-host's PID
//!   alive, the offending action skipped + logged.) This supersedes the older
//!   "no containment is possible on the raw path" note, which only held for
//!   UNGUARDED closures.
//!
//! Contract for host-embeddable components: wrap every fallible callback body —
//! Relm4 `update()` with
//! [`contain_component_update`](crate::containment::contain_component_update), and
//! every raw glib signal / `idle_add` / `timeout_add` closure body with
//! [`guard`](crate::containment::guard).

use std::panic::{AssertUnwindSafe, catch_unwind};

use gtk::glib;
use gtk::prelude::{Cast, CastNone, GtkWindowExt, IsA, WidgetExt};

/// Run one `update()` body, converting a panic into a clean component
/// teardown: the error is logged and the component's window is destroyed on
/// the next idle (never during the unwind), which lets the embedding host
/// reap the component (big-host: registry unrealize hook → drop + cleanup).
/// The process survives; sibling components keep running.
pub fn contain_component_update(
    window: &impl IsA<gtk::Window>,
    component_label: &str,
    update: impl FnOnce(),
) {
    if catch_unwind(AssertUnwindSafe(update)).is_ok() {
        return;
    }
    log::error!(
        "component '{component_label}' panicked in update(); \
         destroying its window, host process survives"
    );
    let window = window.clone().upcast::<gtk::Window>();
    glib::idle_add_local_once(move || window.destroy());
}

/// Like [`contain_component_update`] but for a host-embedded component whose
/// `Root` is not itself a [`gtk::Window`] (e.g. a `ToastOverlay` or `Box`): it
/// derives the toplevel window from `root_widget` so the module's window is
/// reaped on panic. If the widget is not yet rooted, it falls back to logging
/// only (still containing the panic — the process always survives).
pub fn contain_embedded_update(
    root_widget: &impl IsA<gtk::Widget>,
    component_label: &str,
    update: impl FnOnce(),
) {
    if let Some(window) = root_widget.root().and_downcast::<gtk::Window>() {
        contain_component_update(&window, component_label, update);
    } else if catch_unwind(AssertUnwindSafe(update)).is_err() {
        log::error!(
            "component '{component_label}' panicked in update() before it was rooted; \
             host process survives"
        );
    }
}

/// Run a raw glib callback body (signal / `idle_add` / `timeout_add` closure),
/// containing any panic: it is logged and `None` is returned instead of
/// unwinding across the C trampoline (which would abort the whole host
/// process). `context` names the call site for the log line.
///
/// Place this INSIDE the closure, around the fallible body, and map `None` to a
/// safe default for the signal's return type (e.g. `glib::Propagation::Proceed`,
/// `glib::ControlFlow::Break`, `true` for a filter). The default panic hook
/// still runs first, so the message + location reach the journal.
///
/// `AssertUnwindSafe`: GTK widgets and `RefCell`s are not `UnwindSafe`, but a
/// contained panic only skips one callback — every borrow guard releases as the
/// stack unwinds, so the shared state stays usable afterwards (at worst with a
/// half-applied update, still strictly better than a dead process).
#[inline]
pub fn guard<R>(context: &str, body: impl FnOnce() -> R) -> Option<R> {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => Some(value),
        Err(_) => {
            log::error!(
                "contained a panic in `{context}`; action skipped, host process kept alive"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::guard;

    #[test]
    fn returns_value_when_no_panic() {
        assert_eq!(guard("ok", || 7), Some(7));
    }

    #[test]
    fn returns_none_and_releases_borrow_when_body_panics() {
        let cell = RefCell::new(0);
        let outcome = guard("boom", || {
            let mut value = cell.borrow_mut();
            *value += 1;
            panic!("widget bug");
            #[allow(unreachable_code)]
            *value
        });
        assert_eq!(outcome, None);
        // The borrow released as the stack unwound — the cell is usable again.
        assert_eq!(*cell.borrow(), 1);
    }
}
