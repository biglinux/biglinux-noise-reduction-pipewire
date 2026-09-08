// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared split chooser and move-target chooser chrome.

use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::feedback::tooltip;
use crate::layout::split_panes::BigSplitPlacement;

const DEFAULT_OUTER_MARGIN: i32 = 10;
const DEFAULT_LIST_MARGIN: i32 = 8;
const DEFAULT_GRID_SPACING: i32 = 12;
const DEFAULT_GROUP_SPACING: i32 = 6;
const DEFAULT_BUTTON_WIDTH: i32 = 48;
const DEFAULT_BUTTON_HEIGHT: i32 = 42;

/// One content action offered for each split direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSplitChooserContentSpec {
    /// Stable app-owned action id passed back to the activation callback.
    pub id: String,
    /// User-visible content label.
    pub label: String,
    /// Symbolic icon name.
    pub icon_name: String,
    /// Extra CSS class for this action button.
    pub css_class: String,
}

impl BigSplitChooserContentSpec {
    /// Create a split content action.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        icon_name: impl Into<String>,
        css_class: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon_name: icon_name.into(),
            css_class: css_class.into(),
        }
    }
}

/// Optional saved-session action shown beside normal content actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSplitChooserSessionSpec {
    /// User-visible session label.
    pub label: String,
    /// Symbolic icon name.
    pub icon_name: String,
    /// Extra CSS class for the button.
    pub css_class: String,
}

impl BigSplitChooserSessionSpec {
    /// Create a session split action.
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        icon_name: impl Into<String>,
        css_class: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            icon_name: icon_name.into(),
            css_class: css_class.into(),
        }
    }
}

/// Labels and styling for one split direction group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSplitChooserPlacementSpec {
    /// Placement represented by this group.
    pub placement: BigSplitPlacement,
    /// Visible group label.
    pub label: String,
    /// Prefix used in each action button tooltip/accessibility label.
    pub action_label: String,
    /// Extra CSS class for this direction group.
    pub css_class: String,
}

impl BigSplitChooserPlacementSpec {
    /// Create a direction group.
    #[must_use]
    pub fn new(
        placement: BigSplitPlacement,
        label: impl Into<String>,
        action_label: impl Into<String>,
    ) -> Self {
        Self {
            placement,
            label: label.into(),
            action_label: action_label.into(),
            css_class: default_placement_css_class(placement).to_owned(),
        }
    }

    /// Replace the direction group CSS class.
    #[must_use]
    pub fn css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_class = css_class.into();
        self
    }
}

/// Display-free split chooser spec.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSplitChooserSpec {
    /// Content actions offered for each placement.
    pub content_actions: Vec<BigSplitChooserContentSpec>,
    /// Optional session action.
    pub session_action: Option<BigSplitChooserSessionSpec>,
    /// Direction groups.
    pub placements: Vec<BigSplitChooserPlacementSpec>,
    /// Outer margin.
    pub outer_margin: i32,
    /// Grid spacing.
    pub grid_spacing: i32,
    /// Direction group spacing.
    pub group_spacing: i32,
    /// Button width.
    pub button_width: i32,
    /// Button height.
    pub button_height: i32,
}

impl BigSplitChooserSpec {
    /// Create a split chooser with standard placement order.
    #[must_use]
    pub fn new(
        content_actions: impl IntoIterator<Item = BigSplitChooserContentSpec>,
        placements: impl IntoIterator<Item = BigSplitChooserPlacementSpec>,
    ) -> Self {
        Self {
            content_actions: content_actions.into_iter().collect(),
            session_action: None,
            placements: placements.into_iter().collect(),
            outer_margin: DEFAULT_OUTER_MARGIN,
            grid_spacing: DEFAULT_GRID_SPACING,
            group_spacing: DEFAULT_GROUP_SPACING,
            button_width: DEFAULT_BUTTON_WIDTH,
            button_height: DEFAULT_BUTTON_HEIGHT,
        }
    }

    /// Set the optional session action.
    #[must_use]
    pub fn session_action(mut self, session_action: BigSplitChooserSessionSpec) -> Self {
        self.session_action = Some(session_action);
        self
    }

    /// Clamp layout values into a safe range.
    #[must_use]
    pub fn resolved(&self) -> Self {
        let mut resolved = self.clone();
        resolved.outer_margin = resolved.outer_margin.max(0);
        resolved.grid_spacing = resolved.grid_spacing.max(0);
        resolved.group_spacing = resolved.group_spacing.max(0);
        resolved.button_width = resolved.button_width.max(1);
        resolved.button_height = resolved.button_height.max(1);
        resolved
    }
}

/// One target row in a move-pane chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigMovePaneTargetSpec {
    /// Stable app-owned target id passed back to the activation callback.
    pub id: String,
    /// User-visible target title.
    pub title: String,
}

impl BigMovePaneTargetSpec {
    /// Create a target row.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

/// Display-free move-pane target chooser spec.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigMovePaneTargetChooserSpec {
    /// Chooser title.
    pub title: String,
    /// Prefix used in each target tooltip/accessibility label.
    pub target_accessible_prefix: String,
    /// Target rows.
    pub targets: Vec<BigMovePaneTargetSpec>,
    /// Outer margin.
    pub outer_margin: i32,
    /// Row spacing.
    pub spacing: i32,
}

impl BigMovePaneTargetChooserSpec {
    /// Create a move-target chooser.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        target_accessible_prefix: impl Into<String>,
        targets: impl IntoIterator<Item = BigMovePaneTargetSpec>,
    ) -> Self {
        Self {
            title: title.into(),
            target_accessible_prefix: target_accessible_prefix.into(),
            targets: targets.into_iter().collect(),
            outer_margin: DEFAULT_LIST_MARGIN,
            spacing: 4,
        }
    }

    /// Clamp layout values into a safe range.
    #[must_use]
    pub fn resolved(&self) -> Self {
        let mut resolved = self.clone();
        resolved.outer_margin = resolved.outer_margin.max(0);
        resolved.spacing = resolved.spacing.max(0);
        resolved
    }
}

/// Build a split chooser.
#[must_use]
pub fn build_split_chooser(
    spec: BigSplitChooserSpec,
    on_content_action: Rc<dyn Fn(String, BigSplitPlacement) + 'static>,
    on_session_action: Option<Rc<dyn Fn(BigSplitPlacement) + 'static>>,
) -> gtk::Box {
    let resolved = spec.resolved();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(resolved.outer_margin)
        .margin_bottom(resolved.outer_margin)
        .margin_start(resolved.outer_margin)
        .margin_end(resolved.outer_margin)
        .build();
    root.add_css_class("split-chooser-map");

    let grid = gtk::Grid::builder()
        .column_spacing(resolved.grid_spacing)
        .row_spacing(resolved.grid_spacing)
        .halign(gtk::Align::Center)
        .build();
    grid.add_css_class("split-chooser-map-grid");

    for placement in &resolved.placements {
        let group = split_placement_group(
            &resolved,
            placement,
            on_content_action.clone(),
            on_session_action.clone(),
        );
        let (column, row, width, height) = placement_grid_slot(placement.placement);
        grid.attach(&group, column, row, width, height);
    }

    grid.attach(&split_center_spacer(), 1, 1, 1, 1);
    root.append(&grid);
    root
}

/// Build a move-pane target chooser.
#[must_use]
pub fn build_move_pane_target_chooser(
    spec: BigMovePaneTargetChooserSpec,
    on_target: Rc<dyn Fn(String) + 'static>,
) -> gtk::Box {
    let resolved = spec.resolved();
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(resolved.spacing)
        .margin_top(resolved.outer_margin)
        .margin_bottom(resolved.outer_margin)
        .margin_start(resolved.outer_margin)
        .margin_end(resolved.outer_margin)
        .build();
    container.add_css_class("move-pane-tab-list");

    let title = gtk::Label::new(Some(&resolved.title));
    title.add_css_class("heading");
    title.set_halign(gtk::Align::Start);
    container.append(&title);

    for target in resolved.targets {
        container.append(&move_target_button(
            &resolved.target_accessible_prefix,
            target,
            on_target.clone(),
        ));
    }

    container
}

fn split_placement_group(
    spec: &BigSplitChooserSpec,
    placement: &BigSplitChooserPlacementSpec,
    on_content_action: Rc<dyn Fn(String, BigSplitPlacement) + 'static>,
    on_session_action: Option<Rc<dyn Fn(BigSplitPlacement) + 'static>>,
) -> gtk::Box {
    let group = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(spec.group_spacing)
        .halign(gtk::Align::Center)
        .build();
    group.add_css_class("split-direction-group");
    group.add_css_class(&placement.css_class);

    let label = gtk::Label::new(Some(&placement.label));
    label.add_css_class("caption");
    label.add_css_class("split-direction-label");
    label.set_halign(gtk::Align::Center);
    group.append(&label);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(spec.group_spacing)
        .halign(gtk::Align::Center)
        .build();
    actions.add_css_class("split-direction-actions");

    for action in &spec.content_actions {
        actions.append(&split_choice_button(
            spec,
            placement,
            action,
            on_content_action.clone(),
        ));
    }

    if let (Some(session_action), Some(on_session_action)) =
        (&spec.session_action, on_session_action)
    {
        actions.append(&split_session_button(
            spec,
            placement,
            session_action,
            on_session_action,
        ));
    }

    group.append(&actions);
    group
}

fn split_choice_button(
    spec: &BigSplitChooserSpec,
    placement: &BigSplitChooserPlacementSpec,
    action: &BigSplitChooserContentSpec,
    on_content_action: Rc<dyn Fn(String, BigSplitPlacement) + 'static>,
) -> gtk::Button {
    let button = gtk::Button::builder().icon_name(&action.icon_name).build();
    button.add_css_class("flat");
    button.add_css_class("split-choice-button");
    button.add_css_class(&action.css_class);
    button.set_size_request(spec.button_width, spec.button_height);

    let accessible_label = format!("{}: {}", placement.action_label, action.label);
    tooltip::set(&button, accessible_label.clone());
    button.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);

    let action_id = action.id.clone();
    let placement = placement.placement;
    button.connect_clicked(move |_| {
        on_content_action(action_id.clone(), placement);
    });

    button
}

fn split_session_button(
    spec: &BigSplitChooserSpec,
    placement: &BigSplitChooserPlacementSpec,
    action: &BigSplitChooserSessionSpec,
    on_session_action: Rc<dyn Fn(BigSplitPlacement) + 'static>,
) -> gtk::Button {
    let button = gtk::Button::builder().icon_name(&action.icon_name).build();
    button.add_css_class("flat");
    button.add_css_class("split-choice-button");
    button.add_css_class(&action.css_class);
    button.set_size_request(spec.button_width, spec.button_height);

    let accessible_label = format!("{}: {}", placement.action_label, action.label);
    tooltip::set(&button, accessible_label.clone());
    button.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);

    let placement = placement.placement;
    button.connect_clicked(move |_| {
        on_session_action(placement);
    });

    button
}

fn move_target_button(
    target_accessible_prefix: &str,
    target: BigMovePaneTargetSpec,
    on_target: Rc<dyn Fn(String) + 'static>,
) -> gtk::Button {
    let button = gtk::Button::with_label(&target.title);
    button.add_css_class("flat");
    button.add_css_class("move-pane-tab-button");
    button.set_halign(gtk::Align::Fill);
    button.set_hexpand(true);
    let accessible_label = format!("{target_accessible_prefix}: {}", target.title);
    tooltip::set(&button, accessible_label.clone());
    button.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);

    button.connect_clicked(move |_| {
        on_target(target.id.clone());
    });

    button
}

fn split_center_spacer() -> gtk::Box {
    let spacer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    spacer.add_css_class("split-center-spacer");
    spacer
}

fn placement_grid_slot(placement: BigSplitPlacement) -> (i32, i32, i32, i32) {
    match placement {
        BigSplitPlacement::Top => (0, 0, 3, 1),
        BigSplitPlacement::Left => (0, 1, 1, 1),
        BigSplitPlacement::Right => (2, 1, 1, 1),
        BigSplitPlacement::Bottom => (0, 2, 3, 1),
    }
}

fn default_placement_css_class(placement: BigSplitPlacement) -> &'static str {
    match placement {
        BigSplitPlacement::Left => "split-direction-left",
        BigSplitPlacement::Right => "split-direction-right",
        BigSplitPlacement::Top => "split-direction-top",
        BigSplitPlacement::Bottom => "split-direction-bottom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_spec_uses_standard_css_classes() {
        assert_eq!(
            BigSplitChooserPlacementSpec::new(BigSplitPlacement::Left, "Left", "Split Left")
                .css_class,
            "split-direction-left"
        );
        assert_eq!(
            BigSplitChooserPlacementSpec::new(BigSplitPlacement::Bottom, "Bottom", "Split Bottom")
                .css_class,
            "split-direction-bottom"
        );
    }

    #[test]
    fn split_chooser_spec_clamps_layout_values() {
        let mut spec = BigSplitChooserSpec::new([], []);
        spec.outer_margin = -8;
        spec.group_spacing = -6;
        spec.button_width = 0;
        spec.button_height = -1;

        let resolved = spec.resolved();

        assert_eq!(resolved.outer_margin, 0);
        assert_eq!(resolved.group_spacing, 0);
        assert_eq!(resolved.button_width, 1);
        assert_eq!(resolved.button_height, 1);
    }

    #[test]
    fn move_target_spec_keeps_ids_and_clamps_layout_values() {
        let mut spec = BigMovePaneTargetChooserSpec::new(
            "Move",
            "Move pane to tab",
            [BigMovePaneTargetSpec::new("1", "Tab")],
        );
        spec.outer_margin = -1;
        spec.spacing = -2;

        let resolved = spec.resolved();

        assert_eq!(resolved.outer_margin, 0);
        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.targets[0].id, "1");
    }
}
