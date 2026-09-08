// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Longevity finalization census (INVARIANTS H9 / DESIGN.md §1).
//!
//! Opens and closes a [`BigPreferencesDialog`] `TOTAL` times in ONE process,
//! holding a `glib::WeakRef` to each dialog window across its close: a window
//! still alive after close was NOT finalized — a teardown leak. The census is
//! EXACT (unlike an RSS slope, which a growing-but-bounded GTK cache can
//! false-flag); the printed RSS is advisory only.
//!
//! The boot dance, main-loop pump, and finalization check now come from
//! `big_testkit` (this example is the in-repo adopter that proves the harness).
//!
//! Run headless (host-safe software GTK):
//! `weston --backend=headless --renderer=pixman &` then
//! `WAYLAND_DISPLAY=… cargo run -p big-relm4-components --example leak_soak`.
//! Exits 3 on a leak.

use adw::prelude::*;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::{
    preferences::BigPreferencesGroupSpec,
    preferences_dialog::{BigPreferencesDialog, BigPreferencesDialogSpec},
};
use big_testkit::harness::{fail_census, pump, run_census_app, vmrss_kib};
use big_testkit::leak::assert_finalizes;
use relm4::gtk;
use relm4::gtk::glib;

/// Open/close cycles to census.
const TOTAL: usize = 40;

fn main() {
    let exit_code = run_census_app(
        "br.com.biglinux.big_relm4_components.examples.leak_soak",
        |parent| {
            let result = assert_finalizes(TOTAL, || build_present_close(parent));
            let rss = vmrss_kib();
            match result {
                Ok(()) => println!(
                    "leak-soak census: {TOTAL} open/close cycles, no dialog survived teardown \
                     (advisory RSS {rss} KiB)"
                ),
                Err(leak) => {
                    eprintln!("LEAK: {leak} (advisory RSS {rss} KiB)");
                    fail_census(3);
                }
            }
        },
    );
    std::process::exit(exit_code);
}

/// Build, present, and close one preferences dialog while holding only a
/// `WeakRef` to its window. The `dialog` (and the strong refs it owns) drop as
/// this returns, so [`assert_finalizes`] can census the weak ref.
fn build_present_close(parent: &adw::ApplicationWindow) -> glib::WeakRef<gtk::Window> {
    let dialog = BigPreferencesDialog::new(&BigPreferencesDialogSpec::new("Soak").size(640, 480));
    let group = BigPreferencesGroupSpec::new("Soak group").build();
    group.add(&adw::EntryRow::builder().title("Name").build());
    let parent_row = BigDropdownRow::new(BigDropdownRowSpec::new("Parent", ["None"]));
    group.add(parent_row.root());
    dialog.add_group(&group);
    let _footer = dialog.add_footer_cancel_save("Cancel", "Save");
    dialog.present(parent);
    pump();

    let window: gtk::Window = dialog.window().clone().upcast();
    let weak = window.downgrade();
    window.close();
    drop(window);
    pump();
    weak
}
