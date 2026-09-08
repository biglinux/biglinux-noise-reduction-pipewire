// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Control-center category, search, and navigation shell.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;

use super::sidebar_dialog::{BigSidebarDialog, BigSidebarDialogSpec};
use crate::feedback::tooltip;

/// Display-free specification describing big control center category behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigControlCenterCategorySpec {
    /// Stable identifier used as routing key in the content stack.
    pub id: &'static str,
    /// Sidebar group label this category lives under.
    pub section: &'static str,
    /// Translated display title rendered in the sidebar row.
    pub title: &'static str,
    /// Symbolic icon name (Adwaita/freedesktop icon spec).
    pub icon: &'static str,
    /// Whitespace-separated keywords powering the search-entry filter.
    pub keywords: &'static str,
}

impl BigControlCenterCategorySpec {
    /// Construct a category spec from compile-time string slices.
    #[must_use]
    pub const fn new(
        id: &'static str,
        section: &'static str,
        title: &'static str,
        icon: &'static str,
        keywords: &'static str,
    ) -> Self {
        Self {
            id,
            section,
            title,
            icon,
            keywords,
        }
    }
}

/// Sizing constraints for the control-center split view (sidebar
/// widths, content widths, default window size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigControlCenterSizing {
    /// Minimum sidebar width in CSS pixels before collapse.
    pub sidebar_min_width: i32,
    /// Maximum sidebar width in CSS pixels (used by the split view).
    pub sidebar_max_width: i32,
    /// Minimum content-pane width in CSS pixels.
    pub content_min_width: i32,
    /// Maximum content-pane width in CSS pixels.
    pub content_max_width: i32,
    /// Initial window width applied on first map.
    pub window_default_width: i32,
    /// Initial window height applied on first map.
    pub window_default_height: i32,
}

/// Localized strings the control-center shell needs at build time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigControlCenterLabels {
    /// Translated chrome title (consumed at window construction).
    pub window_title: String,
    /// Translated tooltip text for the headerbar search entry.
    pub search_tooltip: String,
    /// Translated placeholder shown when the search entry is empty.
    pub search_placeholder: String,
}

impl BigControlCenterLabels {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        window_title: impl Into<String>,
        search_tooltip: impl Into<String>,
        search_placeholder: impl Into<String>,
    ) -> Self {
        Self {
            window_title: window_title.into(),
            search_tooltip: search_tooltip.into(),
            search_placeholder: search_placeholder.into(),
        }
    }
}

/// Realised widget tree of the control-center shell: navigation split
/// view, sidebar list, search entry, and content stack.
pub struct BigControlCenterShell {
    root: adw::NavigationSplitView,
    /// Category list rendered on the left pane.
    pub sidebar: gtk::ListBox,
    /// Headerbar search entry filtering the sidebar.
    pub search: gtk::SearchEntry,
    /// Content pane swapping the current category view.
    pub stack: adw::ViewStack,
    content_index: Rc<RefCell<HashMap<String, String>>>,
    categories: Vec<BigControlCenterCategorySpec>,
}

impl BigControlCenterShell {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        categories: &[BigControlCenterCategorySpec],
        labels: BigControlCenterLabels,
        sizing: BigControlCenterSizing,
        translate: Rc<dyn Fn(&str) -> String>,
    ) -> Self {
        let categories = categories.to_vec();
        let sidebar = build_sidebar(&categories, Rc::clone(&translate));
        let stack = adw::ViewStack::new();
        let content_index = Rc::new(RefCell::new(HashMap::new()));
        let search = build_search_entry(
            &sidebar,
            &categories,
            Rc::clone(&translate),
            Rc::clone(&content_index),
            labels.search_placeholder.as_str(),
        );
        populate_sidebar(&sidebar, &stack, &categories, Rc::clone(&translate));

        // Compose the shared split scaffold: flat/seamless sidebar header with
        // an inline title↔search stack (GNOME Settings style — search replaces
        // the title in the header, no extra row), the scrolled category list as
        // the sidebar body, the category `ViewStack` as content.
        let dialog = BigSidebarDialog::new(
            &BigSidebarDialogSpec::new(labels.window_title.clone()).sidebar_width_range(
                f64::from(sizing.sidebar_min_width),
                f64::from(sizing.sidebar_max_width),
            ),
        );
        let (title_search, search_toggle) = build_sidebar_header(&search, &labels);
        dialog.set_sidebar_header(&title_search);
        dialog.sidebar_header().pack_start(&search_toggle);
        dialog.set_sidebar(&build_sidebar_body(&sidebar));
        dialog.set_content(&stack);
        wire_content_title(&stack, &dialog, &categories, Rc::clone(&translate));

        let root = dialog.root().clone();

        Self {
            root,
            sidebar,
            search,
            stack,
            content_index,
            categories,
        }
    }

    /// Return a reference to the `root` exposed by this [`BigControlCenterShell`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::NavigationSplitView {
        &self.root
    }

    /// Cache the normalised search text for page `id`; called whenever
    /// a page rebuilds its content so the search entry can find it
    /// without re-scanning the widget tree.
    pub fn index_page(&self, id: &str, normalized_text: String) {
        self.content_index
            .borrow_mut()
            .insert(id.to_owned(), normalized_text);
    }

    /// Programmatically select the sidebar row with id `target` (and
    /// activate it). No-op when no such row exists.
    pub fn select(&self, target: &str) {
        if let Some(row) = find_row_by_id(&self.sidebar, target) {
            self.sidebar.select_row(Some(&row));
            row.activate();
        }
    }

    /// Return the current `category` value held by this [`BigControlCenterShell`].
    #[must_use]
    pub fn category(&self, id: &str) -> Option<&BigControlCenterCategorySpec> {
        self.categories.iter().find(|category| category.id == id)
    }
}

impl Default for BigControlCenterSizing {
    fn default() -> Self {
        Self {
            sidebar_min_width: 240,
            sidebar_max_width: 280,
            content_min_width: 640,
            content_max_width: 760,
            window_default_width: 1080,
            window_default_height: 760,
        }
    }
}

fn build_sidebar(
    categories: &[BigControlCenterCategorySpec],
    translate: Rc<dyn Fn(&str) -> String>,
) -> gtk::ListBox {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Browse)
        .build();
    list.add_css_class("navigation-sidebar");

    let categories_for_header = categories.to_vec();
    list.set_header_func(move |row, before| {
        let section = section_for_row(&categories_for_header, row);
        let prev_section =
            before.and_then(|before_row| section_for_row(&categories_for_header, before_row));
        if section == prev_section {
            row.set_header(None::<&gtk::Widget>);
            return;
        }
        let Some(section) = section else { return };
        let header = gtk::Label::builder()
            .label(translate(section))
            .xalign(0.0)
            .margin_top(if before.is_some() { 12 } else { 4 })
            .margin_bottom(4)
            .margin_start(12)
            .margin_end(12)
            .build();
        header.add_css_class("heading");
        header.add_css_class("dim-label");
        row.set_header(Some(&header));
    });

    list
}

fn section_for_row<'a>(
    categories: &'a [BigControlCenterCategorySpec],
    row: &gtk::ListBoxRow,
) -> Option<&'a str> {
    let id = row.widget_name();
    categories
        .iter()
        .find(|category| category.id == id.as_str())
        .map(|category| category.section)
}

fn id_for_row<'a>(
    categories: &'a [BigControlCenterCategorySpec],
    row: &gtk::ListBoxRow,
) -> Option<&'a str> {
    let id = row.widget_name();
    categories
        .iter()
        .find(|category| category.id == id.as_str())
        .map(|category| category.id)
}

fn populate_sidebar(
    list: &gtk::ListBox,
    stack: &adw::ViewStack,
    categories: &[BigControlCenterCategorySpec],
    translate: Rc<dyn Fn(&str) -> String>,
) {
    for def in categories {
        list.append(&build_sidebar_row(def, Rc::clone(&translate)));
    }

    let stack_weak = stack.downgrade();
    let categories_for_selection = categories.to_vec();
    list.connect_row_selected(move |_, row| {
        let Some(row) = row else { return };
        let Some(id) = id_for_row(&categories_for_selection, row) else {
            return;
        };
        if let Some(stack) = stack_weak.upgrade() {
            stack.set_visible_child_name(id);
        }
    });

    let stack_weak = stack.downgrade();
    let categories_for_activation = categories.to_vec();
    list.connect_row_activated(move |list, row| {
        list.select_row(Some(row));
        let Some(id) = id_for_row(&categories_for_activation, row) else {
            return;
        };
        if let Some(stack) = stack_weak.upgrade() {
            stack.set_visible_child_name(id);
        }
    });
}

fn build_sidebar_row(
    def: &BigControlCenterCategorySpec,
    translate: Rc<dyn Fn(&str) -> String>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(true);
    row.set_selectable(true);
    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    let icon = gtk::Image::from_icon_name(def.icon);
    icon.set_pixel_size(18);
    let title = translate(def.title);
    let label = gtk::Label::builder()
        .label(&title)
        .xalign(0.0)
        .hexpand(true)
        .build();
    row_box.append(&icon);
    row_box.append(&label);

    let button = gtk::Button::builder()
        .child(&row_box)
        .css_classes(["flat"])
        .hexpand(true)
        .build();
    let row_for_click = row.downgrade();
    button.connect_clicked(move |_| {
        if let Some(row) = row_for_click.upgrade() {
            row.activate();
        }
    });
    row.set_child(Some(&button));
    row.set_widget_name(def.id);
    if let Some(description) = accessible_section_description(&title, &translate(def.section)) {
        row.update_property(&[
            gtk::accessible::Property::Label(&title),
            gtk::accessible::Property::Description(&description),
        ]);
        button.update_property(&[
            gtk::accessible::Property::Label(&title),
            gtk::accessible::Property::Description(&description),
        ]);
    } else {
        row.update_property(&[gtk::accessible::Property::Label(&title)]);
        button.update_property(&[gtk::accessible::Property::Label(&title)]);
    }
    row
}

fn accessible_section_description(title: &str, section: &str) -> Option<String> {
    let trimmed = section.trim();
    if trimmed.is_empty() || trimmed == title.trim() {
        return None;
    }
    Some(trimmed.to_owned())
}

/// Build the sidebar header's title widget — a stack that swaps the window
/// title for the search entry in place (no extra row) — and the search toggle
/// button. The toggle is packed into the scaffold's flat sidebar header by the
/// caller; activating it reveals the inline entry and focuses it, Esc closes it.
fn build_sidebar_header(
    search: &gtk::SearchEntry,
    labels: &BigControlCenterLabels,
) -> (gtk::Stack, gtk::ToggleButton) {
    let title = adw::WindowTitle::new(&labels.window_title, "");
    search.set_hexpand(true);

    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    stack.add_named(&title, Some("title"));
    stack.add_named(search, Some("search"));
    stack.set_visible_child_name("title");

    let toggle = gtk::ToggleButton::builder()
        .icon_name("system-search-symbolic")
        .build();
    tooltip::set(&toggle, labels.search_tooltip.as_str());
    toggle.update_property(&[gtk::accessible::Property::Label(
        labels.search_tooltip.as_str(),
    )]);

    let stack_for_toggle = stack.downgrade();
    let search_for_toggle = search.downgrade();
    toggle.connect_active_notify(move |toggle| {
        let (Some(stack), Some(search)) = (stack_for_toggle.upgrade(), search_for_toggle.upgrade())
        else {
            return;
        };
        if toggle.is_active() {
            stack.set_visible_child_name("search");
            search.grab_focus();
        } else {
            stack.set_visible_child_name("title");
            search.set_text("");
        }
    });

    let toggle_for_stop = toggle.downgrade();
    search.connect_stop_search(move |_| {
        if let Some(toggle) = toggle_for_stop.upgrade() {
            toggle.set_active(false);
        }
    });

    (stack, toggle)
}

/// The sidebar body: the scrolled category list.
fn build_sidebar_body(sidebar: &gtk::ListBox) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(sidebar)
        .build()
}

fn build_search_entry(
    sidebar: &gtk::ListBox,
    categories: &[BigControlCenterCategorySpec],
    translate: Rc<dyn Fn(&str) -> String>,
    index: Rc<RefCell<HashMap<String, String>>>,
    placeholder: &str,
) -> gtk::SearchEntry {
    let entry = gtk::SearchEntry::builder()
        .placeholder_text(placeholder)
        .build();

    let entry_for_filter = entry.downgrade();
    let categories_for_filter = categories.to_vec();
    sidebar.set_filter_func(move |row| {
        let Some(entry) = entry_for_filter.upgrade() else {
            return true;
        };
        let query = entry.text().to_string();
        let needle = normalize_search_text(&query);
        if needle.is_empty() {
            return true;
        }
        let Some(def) = category_for_row(&categories_for_filter, row) else {
            return false;
        };
        control_center_category_matches(
            def,
            &needle,
            &translate(def.title),
            &translate(def.section),
            index.borrow().get(def.id).map(String::as_str),
        )
    });

    let sidebar_for_search = sidebar.downgrade();
    entry.connect_search_changed(move |_| {
        if let Some(sidebar) = sidebar_for_search.upgrade() {
            sidebar.invalidate_filter();
            if let Some(first) = first_visible_row(&sidebar) {
                sidebar.select_row(Some(&first));
            }
        }
    });

    entry
}

fn category_for_row<'a>(
    categories: &'a [BigControlCenterCategorySpec],
    row: &gtk::ListBoxRow,
) -> Option<&'a BigControlCenterCategorySpec> {
    let id = row.widget_name();
    categories
        .iter()
        .find(|category| category.id == id.as_str())
}

fn first_visible_row(list: &gtk::ListBox) -> Option<gtk::ListBoxRow> {
    let mut idx = 0;
    while let Some(row) = list.row_at_index(idx) {
        if row.is_visible() {
            return Some(row);
        }
        idx += 1;
    }
    None
}

fn find_row_by_id(list: &gtk::ListBox, target: &str) -> Option<gtk::ListBoxRow> {
    let mut idx = 0;
    while let Some(row) = list.row_at_index(idx) {
        if row.widget_name().as_str() == target {
            return Some(row);
        }
        idx += 1;
    }
    None
}

/// Keep the scaffold's content-header title in sync with the visible category.
fn wire_content_title(
    stack: &adw::ViewStack,
    dialog: &BigSidebarDialog,
    categories: &[BigControlCenterCategorySpec],
    translate: Rc<dyn Fn(&str) -> String>,
) {
    let categories_for_title = categories.to_vec();
    // Capture only the content-title widget, WEAKLY: a strong `dialog.clone()`
    // here is owned by `stack`'s signal, while the dialog's content pane owns
    // `stack` — a ref cycle that leaks the NavigationSplitView + ViewStack on
    // every dialog close (window finalizes, these two never do).
    let content_title = dialog.content_title_widget().downgrade();
    stack.connect_visible_child_notify(move |stack| {
        let Some(content_title) = content_title.upgrade() else {
            return;
        };
        if let Some(name) = stack.visible_child_name() {
            let title = categories_for_title
                .iter()
                .find(|category| category.id == name.as_str())
                .map(|category| translate(category.title))
                .unwrap_or_default();
            content_title.set_title(&title);
        }
    });
}

/// Walk the widget subtree under `root` and concatenate every
/// user-visible label and tooltip, then normalise it for substring
/// search. Used to refresh a page's search index after rebuild.
#[must_use]
pub fn collect_searchable_text(root: &gtk::Widget) -> String {
    let mut buf = String::new();
    walk_collect(root, &mut buf);
    buf
}

fn walk_collect(widget: &gtk::Widget, buf: &mut String) {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        push_str(buf, &label.label());
    } else if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
        push_str(buf, &row.title());
        push_opt(buf, row.subtitle().as_deref());
    } else if let Some(row) = widget.downcast_ref::<adw::EntryRow>() {
        push_str(buf, &row.title());
    } else if let Some(row) = widget.downcast_ref::<adw::SwitchRow>() {
        push_str(buf, &row.title());
        push_opt(buf, row.subtitle().as_deref());
    } else if let Some(row) = widget.downcast_ref::<adw::ComboRow>() {
        push_str(buf, &row.title());
        push_opt(buf, row.subtitle().as_deref());
    } else if let Some(row) = widget.downcast_ref::<adw::SpinRow>() {
        push_str(buf, &row.title());
        push_opt(buf, row.subtitle().as_deref());
    } else if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
        push_str(buf, &group.title());
        push_opt(buf, group.description().as_deref());
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        walk_collect(&c, buf);
        child = c.next_sibling();
    }
}

fn push_str(buf: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    buf.push_str(text);
    buf.push(' ');
}

fn push_opt(buf: &mut String, text: Option<&str>) {
    if let Some(t) = text {
        push_str(buf, t);
    }
}

/// Lower-case `text`, fold whitespace, and strip stray punctuation so
/// the result can be substring-matched against the user's query.
#[must_use]
pub fn normalize_search_text(input: &str) -> String {
    input
        .chars()
        .flat_map(|c| {
            let lower = c.to_lowercase().next().unwrap_or(c);
            fold_char(lower).chars().collect::<Vec<_>>()
        })
        .collect()
}

/// Report whether the `control center category matches` condition currently holds.
#[must_use]
pub fn control_center_category_matches(
    category: &BigControlCenterCategorySpec,
    needle: &str,
    translated_title: &str,
    translated_section: &str,
    indexed_content: Option<&str>,
) -> bool {
    let title = normalize_search_text(translated_title);
    let section = normalize_search_text(translated_section);
    let keywords = normalize_search_text(category.keywords);
    title.contains(needle)
        || section.contains(needle)
        || keywords.contains(needle)
        || indexed_content.is_some_and(|content| content.contains(needle))
}

fn fold_char(c: char) -> String {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => "a".into(),
        'é' | 'è' | 'ê' | 'ë' => "e".into(),
        'í' | 'ì' | 'î' | 'ï' => "i".into(),
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => "o".into(),
        'ú' | 'ù' | 'û' | 'ü' => "u".into(),
        'ç' => "c".into(),
        'ñ' => "n".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATEGORY: BigControlCenterCategorySpec = BigControlCenterCategorySpec::new(
        "general",
        "Geral",
        "Preferências",
        "preferences-system-symbolic",
        "startup terminal",
    );

    #[test]
    fn normalize_strips_diacritics_and_lowercases() {
        assert_eq!(
            normalize_search_text("Preferências ÇÃO"),
            "preferencias cao"
        );
    }

    #[test]
    fn category_search_matches_keywords_and_content() {
        assert!(control_center_category_matches(
            &CATEGORY,
            "terminal",
            "Preferences",
            "General",
            None,
        ));
        assert!(control_center_category_matches(
            &CATEGORY,
            "fonte",
            "Preferences",
            "General",
            Some("fonte monospace"),
        ));
    }

    #[test]
    fn sidebar_description_drops_duplicate_title() {
        assert_eq!(accessible_section_description("General", "General"), None);
        assert_eq!(accessible_section_description("General", "  "), None);
        assert_eq!(
            accessible_section_description("Fonts & Text", "Appearance"),
            Some("Appearance".to_owned())
        );
    }
}
