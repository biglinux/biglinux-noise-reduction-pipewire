// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Workspace header widgets for shared window shells.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::window_shell::BigWorkspaceWindowShellResolved;
use relm4::gtk;

use crate::pane::{BigWorkspaceShell, SplitPaneSurface};

pub(super) fn create_workspace_tab_bar_host(
    resolved_spec: &BigWorkspaceWindowShellResolved,
) -> gtk::Box {
    let workspace_tab_bar_host = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .css_classes(["big-workspace-window-tab-bar"])
        .build();
    workspace_tab_bar_host.set_accessible_role(gtk::AccessibleRole::TabList);
    workspace_tab_bar_host.update_property(&[gtk::accessible::Property::Label(
        &resolved_spec
            .workspace_accessibility_spec
            .tab_list_accessible_name,
    )]);
    workspace_tab_bar_host
}

pub(super) fn create_workspace_title_stack<P: SplitPaneSurface + 'static, M: 'static>(
    resolved_spec: &BigWorkspaceWindowShellResolved,
    workspace_shell: &Rc<BigWorkspaceShell<P, M>>,
    workspace_tab_bar_host: &gtk::Box,
    tab_strip_in_header: &Rc<Cell<bool>>,
) -> gtk::Stack {
    let single_title = gtk::Label::builder()
        .css_classes(["single-tab-title"])
        .ellipsize(gtk::pango::EllipsizeMode::Start)
        .halign(gtk::Align::Center)
        .hexpand(true)
        .xalign(0.5)
        .build();
    let fallback_title = resolved_spec.application_shell.window_title.clone();
    refresh_workspace_single_title(&single_title, workspace_shell, &fallback_title);

    let title_stack = gtk::Stack::new();
    title_stack.add_named(workspace_tab_bar_host, Some("tabs-view"));
    title_stack.add_named(&single_title, Some("title-view"));
    title_stack.set_hexpand(true);
    refresh_workspace_title_stack_policy(
        &title_stack,
        workspace_shell.strip().n_pages(),
        tab_strip_in_header,
    );

    let stack_weak = title_stack.downgrade();
    let in_header = tab_strip_in_header.clone();
    workspace_shell
        .strip()
        .connect_n_pages_notify(move |strip| {
            let Some(stack) = stack_weak.upgrade() else {
                return;
            };
            refresh_workspace_title_stack_policy(&stack, strip.n_pages(), &in_header);
        });

    let title_weak = single_title.downgrade();
    let shell_for_select = Rc::downgrade(workspace_shell);
    let fallback_for_select = fallback_title.clone();
    workspace_shell
        .strip()
        .connect_selected_page_notify(move |_| {
            let Some(title) = title_weak.upgrade() else {
                return;
            };
            let Some(shell) = shell_for_select.upgrade() else {
                return;
            };
            refresh_workspace_single_title(&title, &shell, &fallback_for_select);
        });

    let title_weak = single_title.downgrade();
    let shell_for_attach = Rc::downgrade(workspace_shell);
    let fallback_for_attach = fallback_title;
    workspace_shell
        .strip()
        .connect_page_attached(move |_, page, _| {
            let title_weak = title_weak.clone();
            let shell = shell_for_attach.clone();
            let fallback = fallback_for_attach.clone();
            page.connect_title_notify(move |_| {
                let Some(title) = title_weak.upgrade() else {
                    return;
                };
                let Some(shell) = shell.upgrade() else {
                    return;
                };
                refresh_workspace_single_title(&title, &shell, &fallback);
            });
        });

    title_stack
}

fn refresh_workspace_single_title<P: SplitPaneSurface + 'static, M: 'static>(
    single_title: &gtk::Label,
    workspace_shell: &Rc<BigWorkspaceShell<P, M>>,
    fallback_title: &str,
) {
    let title = workspace_shell
        .strip()
        .selected_page()
        .map(|page| page.title())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| fallback_title.to_owned());
    single_title.set_text(&title);
    crate::feedback::tooltip::set(single_title, &title);
}

/// Header title policy: show the tab bar only while the strip is mounted in
/// the header AND more than one tab is open; otherwise show the single title.
pub(super) fn refresh_workspace_title_stack_policy(
    stack: &gtk::Stack,
    page_count: i32,
    tab_strip_in_header: &Rc<Cell<bool>>,
) {
    stack.set_visible_child_name(if tab_strip_in_header.get() && page_count > 1 {
        "tabs-view"
    } else {
        "title-view"
    });
}
