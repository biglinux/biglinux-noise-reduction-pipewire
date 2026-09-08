// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Debounced GTK input signal helpers.

use relm4::gtk;
use relm4::gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Cancellable trailing-edge debouncer scheduled on the GLib main
/// loop. Each [`Self::run`] call replaces any pending action so only
/// the last one fires after `delay`.
#[derive(Clone)]
pub struct BigDebouncedAction {
    delay: Duration,
    pending: Rc<RefCell<Option<gtk::glib::SourceId>>>,
}

impl BigDebouncedAction {
    /// Creates a new instance.
    #[must_use]
    pub fn new(delay: Duration) -> Self {
        Self {
            delay,
            pending: Rc::new(RefCell::new(None)),
        }
    }

    /// Execute the configured operation and return its outcome.
    pub fn run(&self, action: impl FnOnce() + 'static) {
        if let Some(id) = self.pending.borrow_mut().take() {
            id.remove();
        }
        let pending = self.pending.clone();
        let id = gtk::glib::timeout_add_local_once(self.delay, move || {
            pending.borrow_mut().take();
            action();
        });
        self.pending.borrow_mut().replace(id);
    }
}

/// Connect `adjustment.value-changed` so `on_value` only fires after
/// the user stops moving the control for `delay`. Avoids storms of
/// updates from sliders and spin buttons.
pub fn connect_debounced_adjustment(
    adjustment: &gtk::Adjustment,
    delay: Duration,
    on_value: impl Fn(f64) + 'static,
) {
    let debouncer = BigDebouncedAction::new(delay);
    let on_value = Rc::new(on_value);
    adjustment.connect_value_changed(move |adjustment| {
        let value = adjustment.value();
        let on_value = on_value.clone();
        debouncer.run(move || on_value(value));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounced_action_keeps_requested_delay() {
        let action = BigDebouncedAction::new(Duration::from_millis(200));
        assert_eq!(action.delay, Duration::from_millis(200));
    }
}
