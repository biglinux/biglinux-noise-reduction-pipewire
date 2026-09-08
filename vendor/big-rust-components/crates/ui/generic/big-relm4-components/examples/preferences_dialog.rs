// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Minimal demo of [`BigPreferencesDialog`].
//!
//! Run with `cargo run -p big-relm4-components --example preferences_dialog`.

use adw::prelude::*;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::{
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
    preferences_dialog::{BigPreferencesDialog, BigPreferencesDialogSpec},
};
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent, gtk};

fn main() {
    let app = RelmApp::new("br.com.biglinux.big_relm4_components.examples.preferences_dialog");
    app.run::<PreferencesDialogDemo>(());
}

struct PreferencesDialogDemo;

impl SimpleComponent for PreferencesDialogDemo {
    type Input = ();
    type Output = ();
    type Init = ();
    type Root = adw::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .default_width(400)
            .default_height(200)
            .title("BigPreferencesDialog demo")
            .build()
    }

    fn init(
        (): Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        build_main_window(&root);
        ComponentParts {
            model: Self,
            widgets: (),
        }
    }
}

fn build_main_window(parent: &adw::ApplicationWindow) {
    let open_button = gtk::Button::with_label("Open preferences");
    let parent_clone = parent.clone();
    open_button.connect_clicked(move |_| open_dialog(&parent_clone));

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    body.append(&open_button);
    let page = BigDialogPage::plain();
    page.content().append(&body);
    parent.set_content(Some(page.root()));
}

fn open_dialog(parent: &adw::ApplicationWindow) {
    let dialog =
        BigPreferencesDialog::new(&BigPreferencesDialogSpec::new("Edit Folder").size(640, 480));

    let folder_information_group = BigPreferencesGroupSpec::new("Folder information")
        .description("Display name shown in the sessions panel.")
        .build();
    let name_row = adw::EntryRow::builder().title("Folder name").build();
    folder_information_group.add(&name_row);
    dialog.add_group(&folder_information_group);

    let org_group = BigPreferencesGroupSpec::new("Organization")
        .description("Place this folder inside another folder to build a tree.")
        .build();
    let parent_row = BigDropdownRow::new(BigDropdownRowSpec::new("Parent folder", ["None"]));
    org_group.add(parent_row.root());
    dialog.add_group(&org_group);

    let (cancel, save) = dialog.add_footer_cancel_save("Cancel", "Save");
    let window_for_cancel = dialog.window().clone();
    cancel.connect_clicked(move |_| window_for_cancel.close());
    let window_for_save = dialog.window().clone();
    save.connect_clicked(move |_| window_for_save.close());

    dialog.present(parent);
}
