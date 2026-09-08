// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Teardown discipline for manually parented popovers.
//!
//! `gtk::Popover::set_parent` never gets an automatic counterpart: GTK4
//! expects the owner to `unparent()` during the parent's dispose, otherwise
//! the parent finalizes with the popover still attached ("Finalizing
//! GtkWidget, but it still has children left") and the popover leaks.
//!
//! Two hooks, both needed (signal-level equivalent of the dispose-vfunc
//! pattern for code that does not subclass):
//! - `unrealize` — fires early (widget leaves the tree / toplevel closes),
//!   but ONLY for widgets that were realized; a control on a never-shown
//!   stack page never realizes, so unrealize alone misses it (measured:
//!   the converter's zoom button on the hidden audio-edit page).
//! - `destroy` — emitted at the start of the widget's dispose, in time to
//!   unparent before the finalize check. Sufficient alone ONLY when the
//!   parent actually disposes; an external strong ref (module registries,
//!   controllers) delays dispose indefinitely, which is why unrealize is
//!   kept as the early path.

use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

/// Unparent (and pop down) `popover` when `parent` unrealizes or is
/// destroyed, whichever comes first; idempotent. Accepts any popover type
/// (`Popover`, `PopoverMenu`, …). Holds only a weak ref to the popover, so
/// callers keep full ownership.
pub fn unparent_popover_on_parent_teardown(parent: &gtk::Widget, popover: &impl IsA<gtk::Popover>) {
    let popover: &gtk::Popover = popover.upcast_ref();
    let popover_weak = popover.downgrade();
    parent.connect_unrealize(move |_| release_popover(&popover_weak));
    let popover_weak = popover.downgrade();
    parent.connect_destroy(move |_| release_popover(&popover_weak));
}

fn release_popover(popover_weak: &glib::WeakRef<gtk::Popover>) {
    if let Some(popover) = popover_weak.upgrade()
        && popover.parent().is_some()
    {
        popover.popdown();
        popover.unparent();
    }
}
