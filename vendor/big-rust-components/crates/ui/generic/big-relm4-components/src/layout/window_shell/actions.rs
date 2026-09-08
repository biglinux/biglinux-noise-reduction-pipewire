// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Action widgets for shared window shells.

use adw::prelude::*;
use big_app_kit::window_shell::{BigWindowActionSpec, BigWindowActionSurface};
use relm4::gtk;

pub(super) fn create_footer_status_bar(actions: &[BigWindowActionSpec]) -> Option<gtk::Box> {
    let footer_actions = footer_status_actions(actions);
    if footer_actions.is_empty() {
        return None;
    }

    let footer_status_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .css_classes(["big-application-window-footer-status"])
        .build();
    footer_status_bar.set_accessible_role(gtk::AccessibleRole::Group);
    footer_status_bar.update_property(&[gtk::accessible::Property::Label("Status actions")]);

    for action in footer_actions {
        footer_status_bar.append(&create_shell_action_button(action));
    }

    Some(footer_status_bar)
}

fn footer_status_actions(actions: &[BigWindowActionSpec]) -> Vec<&BigWindowActionSpec> {
    actions
        .iter()
        .filter(|action| action.surface == BigWindowActionSurface::FooterStatus)
        .collect()
}

pub(super) fn create_shell_action_button(action: &BigWindowActionSpec) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(&action.label)
        .css_classes(["big-application-window-action"])
        .build();
    button.set_action_name(Some(&action.action_id));
    button.update_property(&[gtk::accessible::Property::Label(&action.accessible_name)]);
    button
}

#[cfg(test)]
mod tests {
    use super::footer_status_actions;
    use big_app_kit::window_shell::{BigWindowActionSpec, BigWindowActionSurface};

    #[test]
    fn footer_status_bar_requires_footer_actions() {
        assert!(footer_status_actions(&[]).is_empty());
        let action = BigWindowActionSpec::new(
            "win.toggle-status",
            "Status",
            "Toggle status",
            BigWindowActionSurface::FooterStatus,
        );
        let actions = [action];

        assert_eq!(footer_status_actions(&actions).len(), 1);
    }
}
