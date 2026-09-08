// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Searchable action-list surface with optional empty state.

use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;

use crate::list::action_nav_row::{BigActionNavRow, BigActionNavRowSpec};

const DEFAULT_SPACING: i32 = 8;
const DEFAULT_MARGIN_TOP: i32 = 8;
const DEFAULT_MARGIN_BOTTOM: i32 = 12;
const DEFAULT_MARGIN_START: i32 = 12;
const DEFAULT_MARGIN_END: i32 = 12;

/// One row in a searchable action list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchableActionRowSpec {
    /// Stable app-owned id emitted on activation.
    pub id: String,
    /// Row title.
    pub title: String,
    /// Optional row subtitle.
    pub subtitle: Option<String>,
    /// Explicit action label for the row button/accessibility.
    pub action_label: String,
    /// Lower-cost caller-provided text used by the built-in filter.
    pub search_text: String,
}

impl BigSearchableActionRowSpec {
    /// Create a row spec.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        action_label: impl Into<String>,
        search_text: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            action_label: action_label.into(),
            search_text: search_text.into(),
        }
    }

    /// Set row subtitle.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

/// Optional empty state shown when the list has no rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchableActionListEmptySpec {
    /// Empty-state title.
    pub title: String,
    /// Empty-state description.
    pub description: String,
    /// Empty-state icon name.
    pub icon_name: String,
}

impl BigSearchableActionListEmptySpec {
    /// Create an empty state spec.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        description: impl Into<String>,
        icon_name: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            icon_name: icon_name.into(),
        }
    }
}

/// Searchable action-list display contract.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchableActionListSpec {
    /// Search entry placeholder.
    pub search_placeholder: String,
    /// Search entry accessible label.
    pub search_accessible_label: String,
    /// Rows.
    pub rows: Vec<BigSearchableActionRowSpec>,
    /// Optional empty state shown when rows are empty.
    pub empty: Option<BigSearchableActionListEmptySpec>,
    /// CSS classes applied to the list box.
    pub list_css_classes: Vec<String>,
    /// CSS classes applied to the search entry.
    pub search_css_classes: Vec<String>,
    /// CSS classes applied to the scroller.
    pub scroller_css_classes: Vec<String>,
    /// Search entry top margin.
    pub search_margin_top: i32,
    /// Search entry bottom margin.
    pub search_margin_bottom: i32,
    /// Search entry start margin.
    pub search_margin_start: i32,
    /// Search entry end margin.
    pub search_margin_end: i32,
    /// Space between search and list.
    pub spacing: i32,
    /// Top margin.
    pub margin_top: i32,
    /// Bottom margin.
    pub margin_bottom: i32,
    /// Start margin.
    pub margin_start: i32,
    /// End margin.
    pub margin_end: i32,
}

impl BigSearchableActionListSpec {
    /// Create a searchable action-list spec.
    #[must_use]
    pub fn new(
        search_placeholder: impl Into<String>,
        search_accessible_label: impl Into<String>,
        rows: impl IntoIterator<Item = BigSearchableActionRowSpec>,
    ) -> Self {
        Self {
            search_placeholder: search_placeholder.into(),
            search_accessible_label: search_accessible_label.into(),
            rows: rows.into_iter().collect(),
            empty: None,
            list_css_classes: vec!["boxed-list".to_owned()],
            search_css_classes: Vec::new(),
            scroller_css_classes: Vec::new(),
            search_margin_top: 0,
            search_margin_bottom: 0,
            search_margin_start: 0,
            search_margin_end: 0,
            spacing: DEFAULT_SPACING,
            margin_top: DEFAULT_MARGIN_TOP,
            margin_bottom: DEFAULT_MARGIN_BOTTOM,
            margin_start: DEFAULT_MARGIN_START,
            margin_end: DEFAULT_MARGIN_END,
        }
    }

    /// Add an empty state.
    #[must_use]
    pub fn empty(mut self, empty: BigSearchableActionListEmptySpec) -> Self {
        self.empty = Some(empty);
        self
    }

    /// Replace list CSS classes.
    #[must_use]
    pub fn list_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.list_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Replace search CSS classes.
    #[must_use]
    pub fn search_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.search_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Replace scroller CSS classes.
    #[must_use]
    pub fn scroller_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.scroller_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Set outer margins.
    #[must_use]
    pub fn margins(mut self, top: i32, bottom: i32, start: i32, end: i32) -> Self {
        self.margin_top = top;
        self.margin_bottom = bottom;
        self.margin_start = start;
        self.margin_end = end;
        self
    }

    /// Set search entry margins.
    #[must_use]
    pub fn search_margins(mut self, top: i32, bottom: i32, start: i32, end: i32) -> Self {
        self.search_margin_top = top;
        self.search_margin_bottom = bottom;
        self.search_margin_start = start;
        self.search_margin_end = end;
        self
    }

    /// Set vertical spacing.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Resolve layout values into safe ranges.
    #[must_use]
    pub fn resolved(&self) -> Self {
        let mut resolved = self.clone();
        resolved.spacing = resolved.spacing.max(0);
        resolved.margin_top = resolved.margin_top.max(0);
        resolved.margin_bottom = resolved.margin_bottom.max(0);
        resolved.margin_start = resolved.margin_start.max(0);
        resolved.margin_end = resolved.margin_end.max(0);
        resolved.search_margin_top = resolved.search_margin_top.max(0);
        resolved.search_margin_bottom = resolved.search_margin_bottom.max(0);
        resolved.search_margin_start = resolved.search_margin_start.max(0);
        resolved.search_margin_end = resolved.search_margin_end.max(0);
        resolved
    }
}

/// Built searchable action-list parts.
#[derive(Debug, Clone)]
pub struct BigSearchableActionList {
    root: gtk::Box,
    search: gtk::SearchEntry,
    list_box: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    empty: Option<adw::StatusPage>,
}

impl BigSearchableActionList {
    /// Build a searchable action list.
    #[must_use]
    pub fn new(
        spec: BigSearchableActionListSpec,
        on_activate: Rc<dyn Fn(String) + 'static>,
    ) -> Self {
        let resolved = spec.resolved();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(resolved.spacing)
            .margin_top(resolved.margin_top)
            .margin_bottom(resolved.margin_bottom)
            .margin_start(resolved.margin_start)
            .margin_end(resolved.margin_end)
            .build();

        let search = gtk::SearchEntry::builder()
            .placeholder_text(&resolved.search_placeholder)
            .hexpand(true)
            .css_classes(css_classes_array(&resolved.search_css_classes))
            .margin_top(resolved.search_margin_top)
            .margin_bottom(resolved.search_margin_bottom)
            .margin_start(resolved.search_margin_start)
            .margin_end(resolved.search_margin_end)
            .build();
        search.update_property(&[gtk::accessible::Property::Label(
            &resolved.search_accessible_label,
        )]);
        root.append(&search);

        let list_box = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(css_classes_array(&resolved.list_css_classes))
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .css_classes(css_classes_array(&resolved.scroller_css_classes))
            .child(&list_box)
            .build();
        root.append(&scroller);

        let empty = resolved.empty.map(|empty| {
            let status = adw::StatusPage::builder()
                .title(empty.title)
                .description(empty.description)
                .icon_name(empty.icon_name)
                .vexpand(true)
                .visible(false)
                .build();
            root.append(&status);
            status
        });

        let search_texts = Rc::new(
            resolved
                .rows
                .iter()
                .map(|row| row.search_text.to_lowercase())
                .collect::<Vec<_>>(),
        );
        let query = Rc::new(std::cell::RefCell::new(String::new()));

        for row in resolved.rows {
            let mut row_spec = BigActionNavRowSpec::new(row.title).action_label(row.action_label);
            if let Some(subtitle) = row.subtitle {
                row_spec = row_spec.subtitle(subtitle);
            }
            let action_row = BigActionNavRow::new(row_spec).into_root();
            let id = row.id;
            let on_activate = on_activate.clone();
            action_row.connect_activated(move |_| {
                on_activate(id.clone());
            });
            list_box.append(&action_row);
        }

        {
            let search_texts = search_texts.clone();
            let query = query.clone();
            list_box.set_filter_func(move |row| {
                let query = query.borrow();
                if query.is_empty() {
                    return true;
                }
                let index = usize::try_from(row.index()).unwrap_or(0);
                search_texts
                    .get(index)
                    .is_some_and(|text| search_text_matches_query(text, query.as_str()))
            });
        }

        let list_box_clone = list_box.clone();
        let scroller_clone = scroller.clone();
        let empty_clone = empty.clone();
        let search_texts_clone = search_texts.clone();
        let query_clone = query.clone();
        search.connect_search_changed(move |entry| {
            *query_clone.borrow_mut() = entry.text().trim().to_lowercase();
            list_box_clone.invalidate_filter();
            update_empty_visibility(
                &search_texts_clone,
                query_clone.borrow().as_str(),
                &scroller_clone,
                empty_clone.as_ref(),
            );
        });
        update_empty_visibility(
            &search_texts,
            query.borrow().as_str(),
            &scroller,
            empty.as_ref(),
        );

        Self {
            root,
            search,
            list_box,
            scroller,
            empty,
        }
    }

    /// Return the root container.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return the search entry.
    #[must_use]
    pub fn search(&self) -> &gtk::SearchEntry {
        &self.search
    }

    /// Return the list box.
    #[must_use]
    pub fn list_box(&self) -> &gtk::ListBox {
        &self.list_box
    }

    /// Return the scroller.
    #[must_use]
    pub fn scroller(&self) -> &gtk::ScrolledWindow {
        &self.scroller
    }

    /// Return the optional empty state.
    #[must_use]
    pub fn empty(&self) -> Option<&adw::StatusPage> {
        self.empty.as_ref()
    }

    /// Consume `self` and yield the root container.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

fn update_empty_visibility(
    search_texts: &[String],
    query: &str,
    scroller: &gtk::ScrolledWindow,
    empty: Option<&adw::StatusPage>,
) {
    let Some(empty) = empty else {
        return;
    };
    let has_visible = if query.is_empty() {
        !search_texts.is_empty()
    } else {
        search_texts
            .iter()
            .any(|text| search_text_matches_query(text, query))
    };
    scroller.set_visible(has_visible);
    empty.set_visible(!has_visible);
}

fn css_classes_array(classes: &[String]) -> Vec<&str> {
    classes.iter().map(String::as_str).collect()
}

fn search_text_matches_query(search_text: &str, query: &str) -> bool {
    query.is_empty() || search_text.contains(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchable_action_list_spec_clamps_layout_values() {
        let mut spec = BigSearchableActionListSpec::new("Search", "Search", []);
        spec.spacing = -1;
        spec.margin_top = -2;
        spec.margin_bottom = -3;
        spec.margin_start = -4;
        spec.margin_end = -5;
        spec.search_margin_top = -6;
        spec.search_margin_bottom = -7;
        spec.search_margin_start = -8;
        spec.search_margin_end = -9;

        let resolved = spec.resolved();

        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.margin_top, 0);
        assert_eq!(resolved.margin_bottom, 0);
        assert_eq!(resolved.margin_start, 0);
        assert_eq!(resolved.margin_end, 0);
        assert_eq!(resolved.search_margin_top, 0);
        assert_eq!(resolved.search_margin_bottom, 0);
        assert_eq!(resolved.search_margin_start, 0);
        assert_eq!(resolved.search_margin_end, 0);
    }

    #[test]
    fn searchable_action_row_keeps_id_title_action_and_search_text() {
        let row = BigSearchableActionRowSpec::new("session-1", "Dev", "Split", "dev host")
            .subtitle("user@host");

        assert_eq!(row.id, "session-1");
        assert_eq!(row.title, "Dev");
        assert_eq!(row.subtitle.as_deref(), Some("user@host"));
        assert_eq!(row.action_label, "Split");
        assert_eq!(row.search_text, "dev host");
    }

    #[test]
    fn searchable_action_query_matches_pre_normalized_text() {
        assert!(search_text_matches_query("dev host bruno", "host"));
        assert!(search_text_matches_query("dev host bruno", ""));
        assert!(!search_text_matches_query("dev host bruno", "prod"));
    }
}
