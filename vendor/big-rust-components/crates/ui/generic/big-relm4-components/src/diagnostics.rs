// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Runtime diagnostics for leak-checking GTK apps.
//!
//! The finalization census is the decisive GTK leak check: a `GObject` only
//! finalizes when no Rust closure still holds it, so tracking which widgets are
//! finalized after a dialog/page closes tells you exactly what leaked (a strong
//! widget⇄handler reference cycle keeps the object alive). heaptrack shows bytes
//! by call site; this shows which *widget* survived.

use relm4::gtk;

/// Log a `new` line now and a `fin` line when `tracked_object` is finalized (freed).
///
/// Census recipe: track the window plus several deep child widgets when a dialog
/// is built, drive N open/close cycles, then
/// `grep -E "object (new|fin)" <log> | sort | uniq -c` — a label with `new` but
/// no matching `fin` leaked (its widget never finalized). Output format matches
/// `[mem_audit] object new <label>` / `... object fin <label>`.
///
/// Cheap (one weak-ref notify per tracked object) but the CALLER decides when to
/// enable it (e.g. behind an app env flag) so production never logs.
pub fn track_object_finalize(
    label: impl Into<String>,
    tracked_object: &impl gtk::glib::object::ObjectExt,
) {
    let label = label.into();
    eprintln!("[mem_audit] object new    {label}");
    tracked_object.add_weak_ref_notify_local(move || {
        eprintln!("[mem_audit] object fin    {label}");
    });
}
