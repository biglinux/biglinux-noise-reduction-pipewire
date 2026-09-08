// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Dialog teardown helpers — actually free a dialog on close.
//!
//! Closing a dialog must deallocate: a `GObject` widget tree only finalizes when
//! no Rust closure still holds it, and even once freed glibc parks the pages in
//! its arenas unless they are trimmed. [`finalize_dialog_window`] performs the
//! detach + destroy + trim so every dialog releases memory on close instead of
//! parking it. [`anchor_dialog_owners`] keeps per-dialog callback owners alive
//! for the window's lifetime WITHOUT forming a widget⇄handler cycle.
//!
//! Rule for dialog authors:
//! - Never store a strong `gtk::Window`/`adw::Window` in a struct, nor capture a
//!   widget strong in a `connect_*` handler attached to that widget or its
//!   descendants — it forms a cycle that outlives the dialog. Use
//!   `glib::WeakRef` (`window.downgrade()` / `Rc::downgrade`) and upgrade in the
//!   handler, or use the signal's own argument.
//! - End the dialog's close-request handler with [`finalize_dialog_window`] and
//!   return `gtk::glib::Propagation::Stop`.

use std::any::Any;
use std::cell::RefCell;

use adw::prelude::*;
use relm4::gtk;

/// Return freed allocator arenas to the OS so resident memory drops on close.
/// Re-exported from the single canonical `big_os_kit::mem` implementation.
pub use big_os_kit::mem::trim_arenas;

/// Detach the window's content, drop its parent/application links, destroy it,
/// re-activate the parent, and hand freed allocator arenas back to the OS.
///
/// Call at the END of a dialog's `connect_close_request` handler — after any
/// app-specific state teardown — then return `gtk::glib::Propagation::Stop`.
pub fn finalize_dialog_window(window: &adw::Window) {
    // Release modality FIRST, while the transient-for link is still intact, so
    // GTK restores the parent window's sensitivity. Clearing transient-for (or
    // destroying) before this leaves a modal parent stuck inert/desaturated.
    let parent = window.transient_for();
    window.set_modal(false);
    window.set_content(None::<&gtk::Widget>);
    window.set_application(None::<&gtk::Application>);
    // Keep `transient_for` set across destroy so GTK hands focus back to the
    // parent; clearing it first leaves the parent stuck in the `:backdrop` state
    // (libadwaita desaturates inactive windows → washed-out colors).
    window.destroy();
    if let Some(parent) = &parent {
        // Re-activate the parent so it leaves `:backdrop` (restores colors).
        parent.present();
    }
    // The widget tree is freed; return the arenas so RSS drops on close.
    trim_arenas();
}

/// Tie per-dialog callback owners to the window's lifetime WITHOUT forming a
/// widget⇄handler cycle.
///
/// An aggregator callback (e.g. a save-sensitivity or visibility evaluator)
/// usually holds the form widgets strong so it can read/update them. If a handler
/// attached to one of those widgets also held the aggregator strong, the pair
/// would cycle and the whole form would survive `destroy()` — the dominant dialog
/// memory leak. The fix: form-widget handlers hold the aggregator with
/// [`std::rc::Weak`] (upgrade-or-return), and the single strong reference is
/// parked here. The window owns the `destroy` handler, so the owners are dropped
/// when the window is destroyed, releasing the form.
pub fn anchor_dialog_owners(window: &impl IsA<gtk::Widget>, owners: Vec<Box<dyn Any>>) {
    let owners = RefCell::new(Some(owners));
    window.connect_destroy(move |_| {
        owners.borrow_mut().take();
    });
}
