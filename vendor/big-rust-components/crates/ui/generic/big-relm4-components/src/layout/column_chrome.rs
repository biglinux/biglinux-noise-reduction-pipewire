// SPDX-License-Identifier: MIT

//! Shared "Configure Columns…" chrome for `gtk::ColumnView` tables (RESTRUCTURE
//! P5). Promoted from the duplicated audio-player chrome; generic over any
//! `ColumnView`, so the queue, library detail, and future tables share one
//! column-visibility menu + dialog. Width persistence lives in
//! [`super::column_persist`]; the per-app cell factories and sorters stay in the
//! app (they are content-specific, not chrome).

use adw::prelude::*;
use gtk::gio;

use big_app_kit::actions::install_action;

/// Attach a "Configure Columns…" entry to every column's header menu and install
/// the `cv.configure-columns` action that opens a non-modal per-column visibility
/// dialog. `configure_label` (menu entry) and `dialog_title` are caller-provided,
/// already-translated strings (the catalog never embeds msgids). Column
/// visibility is session-only (callers persist it separately if desired).
pub fn wire_configure_columns_menu(
    column_view: &gtk::ColumnView,
    configure_label: &str,
    dialog_title: &str,
) {
    let header_menu = gio::Menu::new();
    header_menu.append(Some(configure_label), Some("cv.configure-columns"));
    let columns = column_view.columns();
    for index in 0..columns.n_items() {
        if let Some(column) = columns.item(index).and_downcast::<gtk::ColumnViewColumn>() {
            column.set_header_menu(Some(&header_menu));
        }
    }

    let view = column_view.downgrade();
    let title = dialog_title.to_owned();
    let group = gio::SimpleActionGroup::new();
    install_action(&group, "configure-columns", move || {
        if let Some(view) = view.upgrade() {
            let parent = view.root().and_downcast::<gtk::Window>();
            show_column_config_dialog(&view, parent.as_ref(), &title);
        }
    });
    column_view.insert_action_group("cv", Some(&group));
}

/// Non-modal dialog: one switch per titled column toggling its visibility live.
fn show_column_config_dialog(
    column_view: &gtk::ColumnView,
    parent: Option<&gtk::Window>,
    title: &str,
) {
    let dialog = adw::Window::builder()
        .title(title)
        .default_width(300)
        .modal(false)
        .resizable(false)
        .build();
    dialog.set_transient_for(parent);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&adw::HeaderBar::new());

    let prefs_group = adw::PreferencesGroup::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(12)
        .build();

    let columns = column_view.columns();
    for index in 0..columns.n_items() {
        let Some(column) = columns.item(index).and_downcast::<gtk::ColumnViewColumn>() else {
            continue;
        };
        let column_title = column
            .title()
            .map(|text| text.to_string())
            .unwrap_or_default();
        if column_title.is_empty() {
            continue;
        }
        let switch_row = adw::SwitchRow::builder()
            .title(&column_title)
            .active(column.is_visible())
            .build();
        // Weak column: the row lives in the dialog, the column in the view — a
        // strong capture would be a row⇄column island outliving both.
        let column = column.downgrade();
        switch_row.connect_active_notify(move |row| {
            if let Some(column) = column.upgrade() {
                column.set_visible(row.is_active());
            }
        });
        prefs_group.add(&switch_row);
    }

    toolbar_view.set_content(Some(&prefs_group));
    dialog.set_content(Some(&toolbar_view));
    dialog.present();
}
