// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Runtime scheduling for the shared component story runner.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use relm4::gtk;
use relm4::gtk::prelude::*;

use super::component_story_cli::StoryArgs;

pub(super) fn schedule_story_ready(
    ready_widget: &gtk::Widget,
    transient_menu_button: Option<&gtk::MenuButton>,
    story_args: &StoryArgs,
) {
    let ready_widget = ready_widget.clone();
    let transient_menu_button = transient_menu_button.cloned();
    let story = story_args.story.as_str();
    let viewport = story_args.viewport;
    let opens_transient = story_args.opens_transient;
    let did_schedule = Rc::new(Cell::new(false));

    ready_widget.connect_map(move |_| {
        if did_schedule.replace(true) {
            return;
        }
        let transient_menu_button = transient_menu_button.clone();
        gtk::glib::timeout_add_local_once(Duration::from_millis(500), move || {
            if opens_transient && let Some(menu_button) = transient_menu_button {
                menu_button.set_active(true);
                menu_button.popup();
            }
            println!(
                "BIG_UI_STORY_READY story={story} viewport={}x{} transient_open={opens_transient}",
                viewport.width, viewport.height
            );
        });
    });
}

pub(super) fn schedule_story_quit(hold_ms: u64) {
    if hold_ms == 0 {
        return;
    }
    gtk::glib::timeout_add_local_once(Duration::from_millis(hold_ms), || {
        if let Some(app) = gtk::gio::Application::default() {
            app.quit();
        }
    });
}
