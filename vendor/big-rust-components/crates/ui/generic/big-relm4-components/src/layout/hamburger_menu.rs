// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Standard BigLinux hamburger menu builder.
//!
//! The framework contract is intentionally narrow: a hamburger menu is a native
//! `GtkMenuButton` with explicit action rows. Apps provide labels and action
//! IDs; the framework owns tooltip labels, popover behavior, and accessible
//! action rows.

use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::gdk;
use relm4::gtk::gio;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

use crate::layout::widget_container::clear_box_children;

const ACTION_POPOVER_QDATA_KEY: &str = "big-action-popover";

#[path = "hamburger_menu/action_row.rs"]
mod action_row;
#[path = "hamburger_menu/menu_model.rs"]
mod menu_model;

use action_row::{build_action_menu_row, build_action_panel_button};
#[cfg(test)]
use menu_model::menu_item_labels;
pub use menu_model::{
    action_groups_from_menu_model, flat_action_items_from_menu_model, menu_action_names,
};
use menu_model::{
    build_custom_action_menu_model_from_menu, build_custom_grouped_action_menu_model,
    custom_action_child_id, menu_model_action_item, menu_model_label_attribute,
};

/// One visible menu item bound to one detailed action name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigMenuActionItem {
    /// User-visible item label.
    pub label: String,
    /// Detailed action name, for example `win.preferences` or `app.about`.
    pub action: String,
    /// Whether the row can be activated.
    pub enabled: bool,
    /// Optional symbolic icon shown before the label.
    pub leading_icon_name: Option<String>,
    /// Optional keyboard shortcut or trailing hint shown after the label.
    pub shortcut_label: Option<String>,
}

impl BigMenuActionItem {
    /// Create a menu item from a localized label and detailed action name.
    #[must_use]
    pub fn new(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: action.into(),
            enabled: true,
            leading_icon_name: None,
            shortcut_label: None,
        }
    }

    /// Set whether the row can be activated.
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Return a menu item that is visible but cannot be activated.
    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Set the optional symbolic icon shown before the label.
    #[must_use]
    pub fn leading_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.leading_icon_name = Some(icon_name.into());
        self
    }

    /// Set the optional shortcut or trailing hint shown after the label.
    #[must_use]
    pub fn shortcut_label(mut self, shortcut_label: impl Into<String>) -> Self {
        self.shortcut_label = Some(shortcut_label.into());
        self
    }
}

/// One visual menu group with an optional heading and ordered action rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigMenuActionGroup {
    /// Optional visible heading for this group.
    pub heading: Option<String>,
    /// Action rows in visual order.
    pub items: Vec<BigMenuActionItem>,
}

impl BigMenuActionGroup {
    /// Create a menu group.
    #[must_use]
    pub fn new(heading: Option<String>, items: Vec<BigMenuActionItem>) -> Self {
        Self { heading, items }
    }
}

/// Specification for a native hamburger menu button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigHamburgerMenuSpec {
    /// Symbolic icon name shown inside the button.
    pub icon_name: String,
    /// Tooltip and accessible label for the icon-only button.
    pub label: String,
    /// CSS classes applied to the menu button.
    pub css_classes: Vec<String>,
    /// Flat ordered menu items.
    pub items: Vec<BigMenuActionItem>,
}

/// Accessible button that opens a flat action popover.
#[derive(Debug, Clone)]
pub struct BigActionPopoverButton {
    root: gtk::Box,
    button: gtk::Button,
    popover: gtk::Popover,
}

impl BigActionPopoverButton {
    /// The root container that must be inserted into the widget tree.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// The button exposed to the widget tree.
    #[must_use]
    pub fn button(&self) -> &gtk::Button {
        &self.button
    }

    /// The popover opened by the button.
    #[must_use]
    pub fn popover(&self) -> &gtk::Popover {
        &self.popover
    }
}

impl BigHamburgerMenuSpec {
    /// Create a standard hamburger spec with the `open-menu-symbolic` icon.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            icon_name: "open-menu-symbolic".to_owned(),
            label: label.into(),
            css_classes: Vec::new(),
            items: Vec::new(),
        }
    }

    /// Override the symbolic icon name.
    #[must_use]
    pub fn icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = icon_name.into();
        self
    }

    /// Replace the CSS class list.
    #[must_use]
    pub fn css_classes<I, S>(mut self, classes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Replace the menu item list.
    #[must_use]
    pub fn items<I>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = BigMenuActionItem>,
    {
        self.items = items.into_iter().collect();
        self
    }
}

/// Build a flat native `GMenuModel` from ordered action items.
#[must_use]
pub fn build_flat_menu_model(items: &[BigMenuActionItem]) -> gio::Menu {
    let menu = gio::Menu::new();
    for item in items {
        menu.append(Some(&item.label), Some(&item.action));
    }
    menu
}

/// Attach a flat action popover to `button` with named action rows.
pub fn set_flat_action_popover(button: &gtk::MenuButton, items: &[BigMenuActionItem]) {
    let popover = build_flat_action_popover(button.upcast_ref::<gtk::Widget>(), items);
    button.set_popover(Some(&popover));
}

/// Attach a grouped action popover to `button` with named action rows.
pub fn set_grouped_action_popover(button: &gtk::MenuButton, groups: &[BigMenuActionGroup]) {
    let popover = build_grouped_action_popover(button.upcast_ref::<gtk::Widget>(), groups);
    button.set_popover(Some(&popover));
}

/// Attach a structured action popover copied from `menu` with named action rows.
pub fn set_menu_model_action_popover(button: &gtk::MenuButton, menu: &impl IsA<gio::MenuModel>) {
    let popover = build_menu_model_action_popover(button.upcast_ref::<gtk::Widget>(), menu);
    button.set_popover(Some(&popover));
}

/// Build a native menu popover whose visible rows are custom named buttons.
#[must_use]
pub fn build_flat_action_popover_menu(
    action_context: &impl IsA<gtk::Widget>,
    items: &[BigMenuActionItem],
) -> gtk::PopoverMenu {
    build_grouped_action_popover_menu(
        action_context,
        &[BigMenuActionGroup::new(None, items.to_vec())],
    )
}

/// Build a native menu popover whose visible groups contain custom named buttons.
#[must_use]
pub fn build_grouped_action_popover_menu(
    action_context: &impl IsA<gtk::Widget>,
    groups: &[BigMenuActionGroup],
) -> gtk::PopoverMenu {
    let menu = build_custom_grouped_action_menu_model(groups);
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.add_css_class("menu");

    for (custom_index, item) in groups
        .iter()
        .flat_map(|group| group.items.iter())
        .enumerate()
    {
        let child_id = custom_action_child_id(custom_index);
        let row = build_action_menu_row(action_context.as_ref(), popover.upcast_ref(), item);
        let added = popover.add_child(&row, &child_id);
        debug_assert!(added, "custom popover menu child id must match menu model");
    }

    popover
}

/// Build a native menu popover that keeps sections/submenus from a `GMenuModel`.
#[must_use]
pub fn build_menu_model_action_popover_menu(
    action_context: &impl IsA<gtk::Widget>,
    menu: &impl IsA<gio::MenuModel>,
) -> gtk::PopoverMenu {
    let (custom_menu, action_items) = build_custom_action_menu_model_from_menu(menu.as_ref());
    let popover = gtk::PopoverMenu::from_model(Some(&custom_menu));
    popover.add_css_class("menu");

    for (custom_index, item) in action_items.iter().enumerate() {
        let child_id = custom_action_child_id(custom_index);
        let row = build_action_menu_row(action_context.as_ref(), popover.upcast_ref(), item);
        let added = popover.add_child(&row, &child_id);
        debug_assert!(added, "custom popover menu child id must match menu model");
    }

    popover
}

/// Build a compact structured action popover with named submenu/action rows.
#[must_use]
pub fn build_menu_model_action_popover(
    action_context: &impl IsA<gtk::Widget>,
    menu: &impl IsA<gio::MenuModel>,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.set_accessible_role(gtk::AccessibleRole::Group);
    popover.add_css_class("menu");
    popover.update_property(&[gtk::accessible::Property::Label("Action menu")]);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    let mut next_page_index = 0;
    append_menu_model_page(
        &stack,
        "root",
        None,
        menu.as_ref(),
        None,
        action_context.as_ref(),
        &popover,
        &mut next_page_index,
    );
    stack.set_visible_child_name("root");

    popover.set_child(Some(&stack));
    popover
}

/// Replace a popover's child with flat named action rows.
pub fn populate_flat_action_popover(
    popover: &gtk::Popover,
    action_context: &impl IsA<gtk::Widget>,
    items: &[BigMenuActionItem],
) {
    popover.set_accessible_role(gtk::AccessibleRole::Group);
    popover.add_css_class("menu");

    let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);

    for item in items {
        rows.append(&build_action_menu_row(
            action_context.as_ref(),
            popover,
            item,
        ));
    }

    popover.set_child(Some(&rows));
}

/// Build a flat action popover with named action rows.
#[must_use]
pub fn build_flat_action_popover(
    action_context: &impl IsA<gtk::Widget>,
    items: &[BigMenuActionItem],
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    populate_flat_action_popover(&popover, action_context, items);
    popover
}

/// Build a grouped action popover with named action rows.
#[must_use]
pub fn build_grouped_action_popover(
    action_context: &impl IsA<gtk::Widget>,
    groups: &[BigMenuActionGroup],
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.set_accessible_role(gtk::AccessibleRole::Group);
    popover.add_css_class("menu");
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);

    for group in groups {
        append_section_heading(&rows, group.heading.clone());
        for item in &group.items {
            rows.append(&build_action_menu_row(
                action_context.as_ref(),
                &popover,
                item,
            ));
        }
    }

    popover.set_child(Some(&rows));
    popover
}

/// Build the reusable overlay panel used when a native popover would expose
/// unnamed toolkit menu proxies through AT-SPI.
#[must_use]
pub fn build_action_overlay_panel(label: &str) -> gtk::Box {
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .width_request(260)
        .margin_top(6)
        .margin_end(6)
        .css_classes(["card", "big-action-overlay-panel"])
        .visible(false)
        .build();
    panel.set_accessible_role(gtk::AccessibleRole::Group);
    panel.update_property(&[gtk::accessible::Property::Label(label)]);
    panel
}

/// Replace a reusable overlay panel's children with flat named action buttons.
pub fn populate_flat_action_panel(
    panel: &gtk::Box,
    action_context: &impl IsA<gtk::Widget>,
    items: &[BigMenuActionItem],
    close_panel: Rc<dyn Fn()>,
) {
    populate_grouped_action_panel(
        panel,
        action_context,
        &[BigMenuActionGroup::new(None, items.to_vec())],
        close_panel,
    );
}

/// Replace a reusable overlay panel's children with grouped named buttons.
pub fn populate_grouped_action_panel(
    panel: &gtk::Box,
    action_context: &impl IsA<gtk::Widget>,
    groups: &[BigMenuActionGroup],
    close_panel: Rc<dyn Fn()>,
) {
    clear_box_children(panel);

    panel.set_margin_top(6);
    panel.set_margin_bottom(6);
    panel.set_margin_start(6);
    panel.set_margin_end(6);

    for group in groups {
        append_section_heading(panel, group.heading.clone());
        for item in &group.items {
            panel.append(&build_action_panel_button(
                action_context.as_ref(),
                item,
                close_panel.clone(),
            ));
        }
    }
}

/// Build a tooltip-aware native hamburger button from a flat menu spec.
#[must_use]
pub fn build_hamburger_menu_button(spec: &BigHamburgerMenuSpec) -> gtk::MenuButton {
    let class_refs = spec
        .css_classes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let button =
        crate::feedback::tooltip::icon_menu_button(&spec.icon_name, &spec.label, &class_refs);
    set_flat_action_popover(&button, &spec.items);
    button
}

/// Build an AT-SPI-actionable button that opens a flat action popover.
#[must_use]
pub fn build_flat_action_popover_button(spec: &BigHamburgerMenuSpec) -> BigActionPopoverButton {
    let class_refs = spec
        .css_classes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let button = crate::feedback::tooltip::icon_button(&spec.icon_name, &spec.label, &class_refs);
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.set_valign(gtk::Align::Center);
    root.append(&button);
    let popover = build_flat_action_popover(&button, &spec.items);
    attach_popover_to_button_with_parent(&root, &button, &popover);
    BigActionPopoverButton {
        root,
        button,
        popover,
    }
}

fn append_menu_model_page(
    stack: &gtk::Stack,
    page_name: &str,
    title: Option<&str>,
    menu: &gio::MenuModel,
    parent_page_name: Option<&str>,
    action_context: &gtk::Widget,
    popover: &gtk::Popover,
    next_page_index: &mut usize,
) {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.set_margin_top(6);
    page.set_margin_bottom(6);
    page.set_margin_start(6);
    page.set_margin_end(6);

    if let Some(title) = title {
        page.append(&build_submenu_header_row(
            stack,
            title,
            parent_page_name.unwrap_or("root"),
        ));
        page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }

    append_menu_model_rows(
        &page,
        stack,
        page_name,
        menu,
        action_context,
        popover,
        next_page_index,
    );
    stack.add_named(&page, Some(page_name));
}

fn append_menu_model_rows(
    page: &gtk::Box,
    stack: &gtk::Stack,
    page_name: &str,
    menu: &gio::MenuModel,
    action_context: &gtk::Widget,
    popover: &gtk::Popover,
    next_page_index: &mut usize,
) {
    for index in 0..menu.n_items() {
        if let Some(section) = menu.item_link(index, "section") {
            append_section_heading(page, menu_model_label_attribute(menu, index));
            append_menu_model_rows(
                page,
                stack,
                page_name,
                &section,
                action_context,
                popover,
                next_page_index,
            );
            continue;
        }

        if let Some(submenu) = menu.item_link(index, "submenu") {
            if let Some(label) = menu_model_label_attribute(menu, index) {
                let submenu_page_name = format!("submenu-{}", *next_page_index);
                *next_page_index += 1;
                append_menu_model_page(
                    stack,
                    &submenu_page_name,
                    Some(&label),
                    &submenu,
                    Some(page_name),
                    action_context,
                    popover,
                    next_page_index,
                );
                page.append(&build_submenu_open_row(stack, &label, &submenu_page_name));
            }
            continue;
        }

        if let Some(item) = menu_model_action_item(menu, index) {
            page.append(&build_action_menu_row(action_context, popover, &item));
        }
    }
}

fn append_section_heading(page: &gtk::Box, heading: Option<String>) {
    if let Some(heading) = heading {
        let label = gtk::Label::new(Some(&heading));
        label.set_xalign(0.0);
        label.add_css_class("dim-label");
        label.add_css_class("caption-heading");
        label.set_margin_top(8);
        label.set_margin_bottom(4);
        label.set_margin_start(12);
        label.set_margin_end(12);
        page.append(&label);
    }
}

fn build_submenu_open_row(stack: &gtk::Stack, label: &str, submenu_page_name: &str) -> gtk::Box {
    let row_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row_content.set_margin_top(6);
    row_content.set_margin_bottom(6);
    row_content.set_margin_start(12);
    row_content.set_margin_end(12);

    let title = gtk::Label::new(Some(label));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    row_content.append(&title);
    row_content.append(&gtk::Image::from_icon_name("go-next-symbolic"));

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["flat", "model", "button"])
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .build();
    row.append(&row_content);
    row.set_focusable(true);
    row.set_accessible_role(gtk::AccessibleRole::Button);
    row.update_property(&[gtk::accessible::Property::Label(label)]);
    row.update_relation(&[gtk::accessible::Relation::LabelledBy(&[title.upcast_ref()])]);

    let stack = stack.clone();
    let submenu_page_name = submenu_page_name.to_owned();
    let activate: Rc<dyn Fn()> = Rc::new(move || {
        stack.set_visible_child_name(&submenu_page_name);
    });
    let click = gtk::GestureClick::new();
    click.connect_released({
        let activate = activate.clone();
        move |_, _, _, _| activate()
    });
    row.add_controller(click);

    let key = gtk::EventControllerKey::new();
    key.connect_key_pressed(move |_, keyval, _, _| {
        if matches!(
            keyval,
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space
        ) {
            activate();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    row.add_controller(key);
    row
}

fn build_submenu_header_row(stack: &gtk::Stack, title: &str, parent_page_name: &str) -> gtk::Box {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.set_margin_top(2);
    header.set_margin_bottom(6);

    let back = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .css_classes(["flat", "circular"])
        .build();
    let back_label = format!("Back from {title}");
    back.update_property(&[gtk::accessible::Property::Label(&back_label)]);
    {
        let stack = stack.clone();
        let parent_page_name = parent_page_name.to_owned();
        back.connect_clicked(move |_| {
            stack.set_visible_child_name(&parent_page_name);
        });
    }
    header.append(&back);

    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    title_label.add_css_class("heading");
    header.append(&title_label);

    header
}

/// Parent `popover` to `button` and open it when the button is activated.
pub fn attach_popover_to_button(button: &gtk::Button, popover: &gtk::Popover) {
    attach_popover_to_button_with_parent(button, button, popover);
}

/// Parent `popover` to a layout-managed root and open it from `button`.
pub fn attach_popover_to_button_with_parent(
    popover_parent: &impl IsA<gtk::Widget>,
    button: &gtk::Button,
    popover: &gtk::Popover,
) {
    popover.set_parent(popover_parent.as_ref());
    unsafe {
        // SAFETY: this module is the only writer for ACTION_POPOVER_QDATA_KEY,
        // and the value is always a strong gtk::Popover reference tied to the
        // parent lifetime. GLib drops the stored value when the parent finalizes.
        popover_parent
            .as_ref()
            .set_data(ACTION_POPOVER_QDATA_KEY, popover.clone());
    }
    {
        let popover = popover.downgrade();
        popover_parent.as_ref().connect_destroy(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
                if popover.parent().is_some() {
                    popover.unparent();
                }
            }
        });
    }
    {
        let popover = popover.clone();
        button.connect_clicked(move |_| {
            if popover.is_mapped() {
                popover.popdown();
            } else {
                popover.popup();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relm4::gtk::glib::variant::ToVariant;

    // GMenu is a GLib object model; ordinary tests cover the contract while
    // Miri stays focused on Rust-owned invariants.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn flat_model_preserves_ordered_actions() {
        let menu = build_flat_menu_model(&[
            BigMenuActionItem::new("Preferences", "win.preferences"),
            BigMenuActionItem::new("About", "app.about"),
        ]);

        assert_eq!(menu_action_names(&menu), ["win.preferences", "app.about"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn flat_action_items_from_menu_model_preserves_named_sections() {
        let root = gio::Menu::new();
        let create = gio::Menu::new();
        create.append(Some("Create Folder"), Some("fm.create-folder"));
        create.append(Some("Create File"), Some("fm.create-file"));
        root.append_section(None, &create);
        let tools = gio::Menu::new();
        tools.append(Some("Rename"), Some("fm.rename"));
        root.append_submenu(Some("Tools"), &tools);

        let items = flat_action_items_from_menu_model(&root);

        assert_eq!(
            menu_item_labels(&items),
            ["Create Folder", "Create File", "Rename"]
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.action.as_str())
                .collect::<Vec<_>>(),
            ["fm.create-folder", "fm.create-file", "fm.rename"]
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn action_groups_from_menu_model_preserves_submenu_headings() {
        let root = gio::Menu::new();
        let view = gio::Menu::new();
        view.append(Some("Details"), Some("win.view::details"));
        view.append(Some("Icons"), Some("win.view::icons"));
        let sort = gio::Menu::new();
        sort.append(Some("Name"), Some("win.sort::name"));
        sort.append(Some("Size"), Some("win.sort::size"));
        root.append_submenu(Some("View"), &view);
        root.append_submenu(Some("Sort"), &sort);

        let groups = action_groups_from_menu_model(&root);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].heading.as_deref(), Some("View"));
        assert_eq!(menu_item_labels(&groups[0].items), ["Details", "Icons"]);
        assert_eq!(groups[1].heading.as_deref(), Some("Sort"));
        assert_eq!(menu_item_labels(&groups[1].items), ["Name", "Size"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn flat_action_items_from_menu_model_keeps_detailed_targets() {
        let root = gio::Menu::new();
        let open_workspace_menu_item = gio::MenuItem::new(Some("Open Workspace"), None);
        open_workspace_menu_item
            .set_action_and_target_value(Some("win.open-workspace"), Some(&"daily".to_variant()));
        root.append_item(&open_workspace_menu_item);

        let items = flat_action_items_from_menu_model(&root);

        assert_eq!(menu_item_labels(&items), ["Open Workspace"]);
        assert_eq!(items[0].action, "win.open-workspace::daily");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn flat_action_items_from_menu_model_ignores_invalid_actions() {
        let root = gio::Menu::new();
        let broken = gio::MenuItem::new(Some("Broken"), None);
        broken.set_attribute_value("action", Some(&"bad action".to_variant()));
        root.append_item(&broken);
        root.append(Some("Preferences"), Some("win.preferences"));

        let items = flat_action_items_from_menu_model(&root);

        assert_eq!(menu_item_labels(&items), ["Preferences"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn action_groups_from_menu_model_ignores_invalid_actions() {
        let root = gio::Menu::new();
        let broken = gio::MenuItem::new(Some("Broken"), None);
        broken.set_attribute_value("action", Some(&"bad action".to_variant()));
        root.append_item(&broken);
        root.append(Some("About"), Some("app.about"));

        let groups = action_groups_from_menu_model(&root);

        assert_eq!(groups.len(), 1);
        assert_eq!(menu_item_labels(&groups[0].items), ["About"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn parse_detailed_action_preserves_string_target() {
        let (action_name, target_value) = action_row::parse_detailed_action("fmsort.by::size")
            .expect("detailed action with string target parses");

        assert_eq!(action_name, "fmsort.by");
        assert_eq!(
            target_value.and_then(|value| value.str().map(str::to_owned)),
            Some("size".to_owned())
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn custom_action_menu_model_references_registered_child_ids() {
        let menu = build_custom_grouped_action_menu_model(&[BigMenuActionGroup::new(
            None,
            vec![
                BigMenuActionItem::new("Preferences", "win.preferences"),
                BigMenuActionItem::new("About", "app.about"),
            ],
        )]);
        let section = menu.item_link(0, "section").expect("section link");

        assert_eq!(menu.n_items(), 1);
        assert_eq!(section.n_items(), 2);
        assert_eq!(
            section
                .item_attribute_value(0, "custom", Some(glib::VariantTy::STRING))
                .and_then(|value| value.get::<String>()),
            Some("big-action-0".to_owned())
        );
        assert_eq!(
            section
                .item_attribute_value(1, "custom", Some(glib::VariantTy::STRING))
                .and_then(|value| value.get::<String>()),
            Some("big-action-1".to_owned())
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn custom_action_menu_model_preserves_sections_and_submenus() {
        let root = gio::Menu::new();
        let view_section = gio::Menu::new();
        let view = gio::Menu::new();
        view.append(Some("Detailed List"), Some("win.view-mode::details"));
        view.append(Some("Icon Grid"), Some("win.view-mode::icons"));
        view_section.append_submenu(Some("View"), &view);
        root.append_section(None, &view_section);

        let (custom_menu, action_items) = build_custom_action_menu_model_from_menu(&root);
        let custom_section = custom_menu.item_link(0, "section").expect("root section");
        let custom_submenu = custom_section
            .item_link(0, "submenu")
            .expect("view submenu");

        assert_eq!(action_items.len(), 2);
        assert_eq!(
            custom_section
                .item_attribute_value(0, "label", Some(glib::VariantTy::STRING))
                .and_then(|value| value.get::<String>()),
            Some("View".to_owned())
        );
        assert_eq!(custom_submenu.n_items(), 2);
        assert_eq!(
            custom_submenu
                .item_attribute_value(0, "custom", Some(glib::VariantTy::STRING))
                .and_then(|value| value.get::<String>()),
            Some("big-action-0".to_owned())
        );
    }

    #[test]
    fn hamburger_spec_defaults_to_open_menu_icon() {
        let spec = BigHamburgerMenuSpec::new("Main Menu");

        assert_eq!(spec.icon_name, "open-menu-symbolic");
        assert_eq!(spec.label, "Main Menu");
        assert!(spec.css_classes.is_empty());
        assert!(spec.items.is_empty());
    }

    // GMenu is a GLib object model; ordinary tests cover the contract while
    // Miri stays focused on Rust-owned invariants.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn flat_model_has_no_sections_or_submenus() {
        let menu = build_flat_menu_model(&[BigMenuActionItem::new("About", "app.about")]);

        for index in 0..menu.n_items() {
            assert!(menu.item_link(index, "section").is_none());
            assert!(menu.item_link(index, "submenu").is_none());
        }
    }

    #[test]
    fn menu_item_labels_preserve_order() {
        let items = [
            BigMenuActionItem::new("Add Files", "header.add-files"),
            BigMenuActionItem::new("Add Folder", "header.add-folder"),
        ];

        assert_eq!(menu_item_labels(&items), ["Add Files", "Add Folder"]);
    }

    #[test]
    fn action_item_enabled_state_is_explicit() {
        let enabled = BigMenuActionItem::new("Reset", "bulk.reset");
        let disabled = BigMenuActionItem::new("Reset", "bulk.reset").disabled();
        let restored = disabled.clone().with_enabled(true);

        assert!(enabled.enabled);
        assert!(!disabled.enabled);
        assert!(restored.enabled);
    }

    #[test]
    fn action_item_visual_metadata_is_optional() {
        let plain = BigMenuActionItem::new("Save", "win.save");
        let decorated = plain
            .clone()
            .leading_icon_name("document-save-symbolic")
            .shortcut_label("Ctrl+S");

        assert_eq!(plain.leading_icon_name, None);
        assert_eq!(plain.shortcut_label, None);
        assert_eq!(
            decorated.leading_icon_name.as_deref(),
            Some("document-save-symbolic")
        );
        assert_eq!(decorated.shortcut_label.as_deref(), Some("Ctrl+S"));
        assert_eq!(decorated.label, "Save");
        assert_eq!(decorated.action, "win.save");
    }
}
