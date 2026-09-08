// SPDX-License-Identifier: MIT

//! Guided empty-state widget builder.

use adw::prelude::*;
use relm4::gtk;

use crate::state::empty::{BigEmptyStateAction, BigGuidedEmptyStateSpec};

const ROOT_CLASS: &str = "big-guided-empty-state";
const ICON_CLASS: &str = "big-guided-empty-state-icon";
const STEPS_CLASS: &str = "big-guided-empty-state-steps";
const STEP_CLASS: &str = "big-guided-empty-state-step";
const STEP_NUMBER_CLASS: &str = "big-guided-empty-state-step-number";
const DEFAULT_ROOT_SPACING: i32 = 8;
const DEFAULT_STEP_SPACING: i32 = 8;
const DEFAULT_ACTION_SPACING: i32 = 8;
const DEFAULT_ROOT_MARGIN: i32 = 18;
const DEFAULT_SIDE_MARGIN: i32 = 8;
const DEFAULT_ICON_PIXEL_SIZE: i32 = 38;

/// One action button built from a [`BigGuidedEmptyStateSpec`].
#[derive(Debug, Clone)]
pub struct BigGuidedEmptyStateActionButton {
    action_id: String,
    button: gtk::Button,
}

impl BigGuidedEmptyStateActionButton {
    /// Return the action identifier associated with this button.
    #[must_use]
    pub fn action_id(&self) -> &str {
        &self.action_id
    }

    /// Return the GTK button.
    #[must_use]
    pub fn button(&self) -> &gtk::Button {
        &self.button
    }
}

/// Built guided empty-state widget plus its unbound action buttons.
#[derive(Debug, Clone)]
pub struct BigGuidedEmptyState {
    root: gtk::Box,
    action_buttons: Vec<BigGuidedEmptyStateActionButton>,
}

impl BigGuidedEmptyState {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigGuidedEmptyStateSpec) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, DEFAULT_ROOT_SPACING);
        root.add_css_class(ROOT_CLASS);
        add_classes(&root, &spec.root_css_classes);
        root.set_valign(gtk::Align::Start);
        root.set_halign(gtk::Align::Fill);
        root.set_margin_top(DEFAULT_ROOT_MARGIN);
        root.set_margin_bottom(DEFAULT_ROOT_MARGIN);
        root.set_margin_start(DEFAULT_SIDE_MARGIN);
        root.set_margin_end(DEFAULT_SIDE_MARGIN);

        root.append(&build_icon(&spec));
        root.append(&build_title(&spec.title));

        if let Some(body) = spec.body.as_deref() {
            root.append(&build_body(body));
        }

        if !spec.steps.is_empty() {
            root.append(&build_steps(&spec));
        }

        let (actions, buttons) = build_actions(&spec.actions);
        if !spec.actions.is_empty() {
            root.append(&actions);
        }

        Self {
            root,
            action_buttons: buttons,
        }
    }

    /// Return the root container.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return action buttons.
    #[must_use]
    pub fn action_buttons(&self) -> &[BigGuidedEmptyStateActionButton] {
        &self.action_buttons
    }

    /// Consume `self` and yield the root container plus action buttons.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, Vec<BigGuidedEmptyStateActionButton>) {
        (self.root, self.action_buttons)
    }
}

fn build_icon(spec: &BigGuidedEmptyStateSpec) -> gtk::Image {
    let icon = gtk::Image::from_icon_name(&spec.icon_name);
    icon.set_pixel_size(DEFAULT_ICON_PIXEL_SIZE);
    icon.add_css_class(ICON_CLASS);
    add_classes(&icon, &spec.icon_css_classes);
    icon.set_halign(gtk::Align::Start);
    icon
}

fn build_title(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("title-3");
    label.set_xalign(0.0);
    label
}

fn build_body(body: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(body));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.add_css_class("dim-label");
    label
}

fn build_steps(spec: &BigGuidedEmptyStateSpec) -> gtk::Box {
    let steps = gtk::Box::new(gtk::Orientation::Vertical, 4);
    steps.add_css_class(STEPS_CLASS);
    add_classes(&steps, &spec.steps_css_classes);
    for (index, step) in spec.steps.iter().enumerate() {
        steps.append(&build_step_row(index + 1, &step.label, spec));
    }
    steps
}

fn build_step_row(number: usize, label_text: &str, spec: &BigGuidedEmptyStateSpec) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, DEFAULT_STEP_SPACING);
    row.add_css_class(STEP_CLASS);
    add_classes(&row, &spec.step_css_classes);

    let badge = gtk::Label::new(Some(&number.to_string()));
    badge.add_css_class(STEP_NUMBER_CLASS);
    add_classes(&badge, &spec.step_number_css_classes);
    row.append(&badge);

    let label = gtk::Label::builder()
        .label(label_text)
        .xalign(0.0)
        .hexpand(true)
        .build();
    row.append(&label);
    row
}

fn build_actions(
    actions: &[BigEmptyStateAction],
) -> (gtk::Box, Vec<BigGuidedEmptyStateActionButton>) {
    let action_box = gtk::Box::new(gtk::Orientation::Horizontal, DEFAULT_ACTION_SPACING);
    action_box.set_halign(gtk::Align::Start);
    action_box.set_margin_top(6);

    let mut buttons = Vec::with_capacity(actions.len());
    for action in actions {
        let button = gtk::Button::builder()
            .label(&action.label)
            .halign(gtk::Align::Center)
            .build();
        button.add_css_class("pill");
        if action.suggested {
            button.add_css_class("suggested-action");
        }
        action_box.append(&button);
        buttons.push(BigGuidedEmptyStateActionButton {
            action_id: action.action_id.clone(),
            button,
        });
    }

    (action_box, buttons)
}

fn add_classes(widget: &impl IsA<gtk::Widget>, classes: &[String]) {
    for class_name in classes {
        widget.add_css_class(class_name);
    }
}
