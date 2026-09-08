// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Hover-to-open popover helpers with mutual exclusion.

use adw::prelude::*;
use relm4::gtk;
use relm4::gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;

/// DEFAULT HOVER CLOSE DELAY MS constant.
pub const DEFAULT_HOVER_CLOSE_DELAY_MS: u64 = 800;

thread_local! {
    static POPOVER_REGISTRY: RefCell<Vec<glib::WeakRef<gtk::Popover>>> = const { RefCell::new(Vec::new()) };
}

type PendingClose = Rc<RefCell<Option<glib::SourceId>>>;

/// Build a non-autohiding popover with a vertical content box and a
/// title label. Returns both the popover and the inner box so the
/// caller can append rows.
pub fn build_vertical_popover_shell(
    parent: &gtk::Box,
    title: &str,
    spacing: i32,
) -> (gtk::Popover, gtk::Box) {
    let popover = gtk::Popover::new();
    popover.set_parent(parent);
    crate::feedback::popover_lifecycle::unparent_popover_on_parent_teardown(
        parent.upcast_ref(),
        &popover,
    );
    popover.set_position(gtk::PositionType::Top);
    popover.set_autohide(false);

    let content = gtk::Box::new(gtk::Orientation::Vertical, spacing);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(4);
    content.set_margin_end(4);
    content.append(&gtk::Label::new(Some(title)));
    popover.set_child(Some(&content));

    (popover, content)
}

/// Wire `hover_box` so its motion/click controllers pop `popover` up
/// and pop down every other popover in `all_popovers`. Implements the
/// mutually-exclusive hover-popover group behaviour.
pub fn setup_hover_controller_exclusive(
    hover_box: &gtk::Box,
    popover: &gtk::Popover,
    all_popovers: &[gtk::Popover],
) {
    register_popover(popover);

    let close_id: PendingClose = Rc::new(RefCell::new(None));
    // Weak: the group members' controllers cross-reference every popover —
    // strong `others` lists form a ring that pins all of them (plus the
    // widgets their scale handlers capture) after window close.
    let others: Vec<glib::WeakRef<gtk::Popover>> = all_popovers
        .iter()
        .filter(|candidate| candidate != &popover)
        .map(glib::object::ObjectExt::downgrade)
        .collect();

    let motion = gtk::EventControllerMotion::new();
    {
        let popover = popover.clone();
        let others = others.clone();
        let close_id = close_id.clone();
        motion.connect_enter(move |_, _, _| {
            cancel_pending_close(&close_id);
            popdown_all_others(&popover, &others);
            if !popover.is_visible() {
                popover.popup();
            }
        });
    }
    {
        let close_id = close_id.clone();
        let popover = popover.clone();
        motion.connect_leave(move |_| {
            schedule_popover_close(&popover, &close_id);
        });
    }
    hover_box.add_controller(motion);

    let popover_motion = gtk::EventControllerMotion::new();
    {
        let close_id = close_id.clone();
        popover_motion.connect_enter(move |_, _, _| {
            cancel_pending_close(&close_id);
        });
    }
    {
        let close_id = close_id.clone();
        // Weak: this controller lives ON the popover — a strong clone here
        // is a popover⇄controller self-cycle that leaks the popover (and
        // everything its child closures capture) on window close.
        let popover = popover.downgrade();
        popover_motion.connect_leave(move |_| {
            if let Some(popover) = popover.upgrade() {
                schedule_popover_close(&popover, &close_id);
            }
        });
    }
    popover.add_controller(popover_motion);

    let click = gtk::GestureClick::new();
    {
        let popover = popover.clone();
        click.connect_released(move |_, _, _, _| {
            popdown_all_others(&popover, &others);
            if !popover.is_visible() {
                popover.popup();
            }
        });
    }
    hover_box.add_controller(click);
}

/// Convenience: register every `(hover_box, popover)` pair as a
/// member of one exclusive hover-popover group, then return the
/// popover list so the caller can reference them later.
pub fn setup_exclusive_hover_popover_group<'a, I>(items: I) -> Vec<gtk::Popover>
where
    I: IntoIterator<Item = (&'a gtk::Box, &'a gtk::Popover)>,
{
    let entries: Vec<(&gtk::Box, gtk::Popover)> = items
        .into_iter()
        .map(|(hover_box, popover)| (hover_box, popover.clone()))
        .collect();
    let all_popovers: Vec<gtk::Popover> =
        entries.iter().map(|(_, popover)| popover.clone()).collect();

    for (hover_box, popover) in &entries {
        setup_hover_controller_exclusive(hover_box, popover, &all_popovers);
    }

    all_popovers
}

/// Return the first preset greater than `current` (by `epsilon`),
/// wrapping around to the first preset when none is greater. `None`
/// when the slice is empty.
#[must_use]
pub fn next_cyclic_preset(current: f64, presets: &[f64], epsilon: f64) -> Option<f64> {
    presets
        .iter()
        .find(|&&preset| preset > current + epsilon)
        .copied()
        .or_else(|| presets.first().copied())
}

fn register_popover(popover: &gtk::Popover) {
    POPOVER_REGISTRY.with(|registry| {
        let mut refs = registry.borrow_mut();
        refs.retain(|weak| weak.upgrade().is_some());
        if refs
            .iter()
            .filter_map(glib::WeakRef::upgrade)
            .any(|registered| registered == *popover)
        {
            return;
        }
        refs.push(popover.downgrade());
    });
}

fn popdown_all_others(current: &gtk::Popover, explicit_others: &[glib::WeakRef<gtk::Popover>]) {
    for other in explicit_others.iter().filter_map(glib::WeakRef::upgrade) {
        if &other != current {
            other.popdown();
        }
    }

    POPOVER_REGISTRY.with(|registry| {
        let mut refs = registry.borrow_mut();
        refs.retain(|weak| weak.upgrade().is_some());
        for popover in refs.iter().filter_map(glib::WeakRef::upgrade) {
            if popover != *current {
                popover.popdown();
            }
        }
    });
}

fn cancel_pending_close(close_id: &PendingClose) {
    if let Some(id) = close_id.borrow_mut().take() {
        id.remove();
    }
}

fn schedule_popover_close(popover: &gtk::Popover, close_id: &PendingClose) {
    let popover = popover.clone();
    let close_id_for_timer = close_id.clone();
    let id = glib::timeout_add_local_once(
        std::time::Duration::from_millis(DEFAULT_HOVER_CLOSE_DELAY_MS),
        move || {
            popover.popdown();
            close_id_for_timer.borrow_mut().take();
        },
    );
    close_id.borrow_mut().replace(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyclic_preset_moves_forward_then_wraps() {
        let presets = [1.0, 1.25, 1.5];
        assert_eq!(next_cyclic_preset(1.0, &presets, 0.01), Some(1.25));
        assert_eq!(next_cyclic_preset(1.5, &presets, 0.01), Some(1.0));
        assert_eq!(next_cyclic_preset(2.0, &presets, 0.01), Some(1.0));
    }

    #[test]
    fn cyclic_preset_handles_empty_list() {
        assert_eq!(next_cyclic_preset(1.0, &[], 0.01), None);
    }
}
