// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Searchable single-selection list surface with optional empty state.

use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;

use crate::list::search_result_row::{BigSearchResultRow, BigSearchResultRowSpec};

const DEFAULT_SPACING: i32 = 8;
const DEFAULT_MARGIN_TOP: i32 = 0;
const DEFAULT_MARGIN_BOTTOM: i32 = 0;
const DEFAULT_MARGIN_START: i32 = 0;
const DEFAULT_MARGIN_END: i32 = 0;
const DEFAULT_ROW_HORIZONTAL_MARGIN: i32 = 12;
const DEFAULT_ROW_VERTICAL_MARGIN: i32 = 8;

/// One row in a searchable single-selection list.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchableSelectionRowSpec {
    /// Stable caller-owned id returned when this row is selected.
    pub id: String,
    /// Row title.
    pub title: String,
    /// Optional row subtitle.
    pub subtitle: Option<String>,
    /// Lower-cost caller-provided text used by the built-in filter.
    pub search_text: String,
    /// Row accessible label.
    pub accessible_label: Option<String>,
    /// CSS classes applied to the row title.
    pub title_css_classes: Vec<String>,
    /// Horizontal row margin.
    pub horizontal_margin: i32,
    /// Vertical row margin.
    pub vertical_margin: i32,
}

impl BigSearchableSelectionRowSpec {
    /// Create a row spec.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        search_text: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            search_text: search_text.into(),
            accessible_label: None,
            title_css_classes: Vec::new(),
            horizontal_margin: DEFAULT_ROW_HORIZONTAL_MARGIN,
            vertical_margin: DEFAULT_ROW_VERTICAL_MARGIN,
        }
    }

    /// Set row subtitle.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Set row accessible label.
    #[must_use]
    pub fn accessible_label(mut self, accessible_label: impl Into<String>) -> Self {
        self.accessible_label = Some(accessible_label.into());
        self
    }

    /// Replace title CSS classes.
    #[must_use]
    pub fn title_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.title_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Set row margins.
    #[must_use]
    pub fn row_margins(mut self, horizontal_margin: i32, vertical_margin: i32) -> Self {
        self.horizontal_margin = horizontal_margin;
        self.vertical_margin = vertical_margin;
        self
    }

    /// Resolve layout values into safe ranges.
    #[must_use]
    pub fn resolved(&self) -> Self {
        let mut resolved = self.clone();
        resolved.horizontal_margin = resolved.horizontal_margin.max(0);
        resolved.vertical_margin = resolved.vertical_margin.max(0);
        resolved
    }
}

/// Optional empty state shown when the filter has no visible rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchableSelectionListEmptySpec {
    /// Empty-state title.
    pub title: String,
    /// Empty-state description.
    pub description: String,
    /// Empty-state icon name.
    pub icon_name: String,
}

impl BigSearchableSelectionListEmptySpec {
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

/// Searchable single-selection list display contract.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchableSelectionListSpec {
    /// Search entry placeholder.
    pub search_placeholder: String,
    /// Search entry accessible label.
    pub search_accessible_label: String,
    /// Rows.
    pub rows: Vec<BigSearchableSelectionRowSpec>,
    /// Initially selected row id.
    pub selected_id: Option<String>,
    /// Optional empty state shown when no rows match.
    pub empty: Option<BigSearchableSelectionListEmptySpec>,
    /// List accessible label.
    pub list_accessible_label: Option<String>,
    /// CSS classes applied to the list box.
    pub list_css_classes: Vec<String>,
    /// CSS classes applied to the search entry.
    pub search_css_classes: Vec<String>,
    /// CSS classes applied to the scroller.
    pub scroller_css_classes: Vec<String>,
    /// Minimum scroller content width.
    pub min_content_width: i32,
    /// Minimum scroller content height.
    pub min_content_height: i32,
    /// Maximum scroller content height.
    pub max_content_height: i32,
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

impl BigSearchableSelectionListSpec {
    /// Create a searchable single-selection list spec.
    #[must_use]
    pub fn new(
        search_placeholder: impl Into<String>,
        search_accessible_label: impl Into<String>,
        rows: impl IntoIterator<Item = BigSearchableSelectionRowSpec>,
    ) -> Self {
        Self {
            search_placeholder: search_placeholder.into(),
            search_accessible_label: search_accessible_label.into(),
            rows: rows.into_iter().collect(),
            selected_id: None,
            empty: None,
            list_accessible_label: None,
            list_css_classes: Vec::new(),
            search_css_classes: Vec::new(),
            scroller_css_classes: Vec::new(),
            min_content_width: 0,
            min_content_height: 0,
            max_content_height: 0,
            spacing: DEFAULT_SPACING,
            margin_top: DEFAULT_MARGIN_TOP,
            margin_bottom: DEFAULT_MARGIN_BOTTOM,
            margin_start: DEFAULT_MARGIN_START,
            margin_end: DEFAULT_MARGIN_END,
        }
    }

    /// Set the initially selected row id.
    #[must_use]
    pub fn selected_id(mut self, selected_id: impl Into<String>) -> Self {
        self.selected_id = Some(selected_id.into());
        self
    }

    /// Add an empty state.
    #[must_use]
    pub fn empty(mut self, empty: BigSearchableSelectionListEmptySpec) -> Self {
        self.empty = Some(empty);
        self
    }

    /// Set the list accessible label.
    #[must_use]
    pub fn list_accessible_label(mut self, list_accessible_label: impl Into<String>) -> Self {
        self.list_accessible_label = Some(list_accessible_label.into());
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

    /// Set scroller size constraints.
    #[must_use]
    pub fn scroller_size(
        mut self,
        min_content_width: i32,
        min_content_height: i32,
        max_content_height: i32,
    ) -> Self {
        self.min_content_width = min_content_width;
        self.min_content_height = min_content_height;
        self.max_content_height = max_content_height;
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
        resolved.rows = resolved.rows.iter().map(|row| row.resolved()).collect();
        resolved.spacing = resolved.spacing.max(0);
        resolved.margin_top = resolved.margin_top.max(0);
        resolved.margin_bottom = resolved.margin_bottom.max(0);
        resolved.margin_start = resolved.margin_start.max(0);
        resolved.margin_end = resolved.margin_end.max(0);
        resolved.min_content_width = resolved.min_content_width.max(0);
        resolved.min_content_height = resolved.min_content_height.max(0);
        resolved.max_content_height = resolved.max_content_height.max(0);
        resolved
    }
}

/// Built searchable single-selection list parts.
#[derive(Debug, Clone)]
pub struct BigSearchableSelectionList {
    root: gtk::Box,
    search: gtk::SearchEntry,
    list_box: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    empty: Option<adw::StatusPage>,
    rows: Rc<[gtk::ListBoxRow]>,
    row_ids: Rc<[String]>,
    search_texts: Rc<[String]>,
}

impl BigSearchableSelectionList {
    /// Build a searchable single-selection list.
    #[must_use]
    pub fn new(spec: BigSearchableSelectionListSpec) -> Self {
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
            .build();
        search.update_property(&[gtk::accessible::Property::Label(
            &resolved.search_accessible_label,
        )]);
        root.append(&search);

        let list_box = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(css_classes_array(&resolved.list_css_classes))
            .build();
        if let Some(label) = resolved.list_accessible_label.as_deref() {
            list_box.update_property(&[gtk::accessible::Property::Label(label)]);
        }

        let scroller = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .css_classes(css_classes_array(&resolved.scroller_css_classes))
            .child(&list_box)
            .build();
        apply_scroller_size(&scroller, &resolved);
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

        let mut rows = Vec::with_capacity(resolved.rows.len());
        let mut row_ids = Vec::with_capacity(resolved.rows.len());
        let mut search_texts = Vec::with_capacity(resolved.rows.len());
        let mut selected_row: Option<gtk::ListBoxRow> = None;

        for row_spec in resolved.rows {
            let row = build_selection_row(&row_spec);
            if resolved.selected_id.as_deref() == Some(row_spec.id.as_str()) {
                selected_row = Some(row.clone());
            }
            let row_id = row_spec.id;
            list_box.append(&row);
            rows.push(row);
            row_ids.push(row_id);
            search_texts.push(row_spec.search_text.to_lowercase());
        }

        if let Some(row) = selected_row.or_else(|| rows.first().cloned()) {
            list_box.select_row(Some(&row));
        }

        let list = Self {
            root,
            search,
            list_box,
            scroller,
            empty,
            rows: Rc::from(rows),
            row_ids: Rc::from(row_ids),
            search_texts: Rc::from(search_texts),
        };
        list.connect_search_filter();
        list.refresh_filter("");
        list
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

    /// Return the currently selected row id.
    #[must_use]
    pub fn selected_id(&self) -> Option<String> {
        let row = self.list_box.selected_row()?;
        if !row.is_visible() {
            return None;
        }
        let index = usize::try_from(row.index()).ok()?;
        self.row_ids.get(index).cloned()
    }

    /// Select the row matching `id`.
    pub fn select_id(&self, id: &str) -> bool {
        let Some(index) = self.row_ids.iter().position(|row_id| row_id == id) else {
            return false;
        };
        let Some(row) = self.rows.get(index) else {
            return false;
        };
        self.list_box.select_row(Some(row));
        true
    }

    /// Connect a callback that receives the selected id after selection changes.
    pub fn connect_selected_id_changed(
        &self,
        callback: impl Fn(Option<String>) + 'static,
    ) -> gtk::glib::SignalHandlerId {
        let list = self.clone();
        self.list_box
            .connect_row_selected(move |_, _| callback(list.selected_id()))
    }

    /// Consume `self` and yield the root container.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }

    fn connect_search_filter(&self) {
        let list = self.clone();
        self.search.connect_search_changed(move |entry| {
            list.refresh_filter(entry.text().as_str());
        });
    }

    fn refresh_filter(&self, query: &str) {
        let query = query.trim().to_lowercase();
        let mut first_visible: Option<gtk::ListBoxRow> = None;
        for (row, search_text) in self.rows.iter().zip(self.search_texts.iter()) {
            let visible = search_text_matches_query(search_text, query.as_str());
            row.set_visible(visible);
            if visible && first_visible.is_none() {
                first_visible = Some(row.clone());
            }
        }

        let has_visible_selection = self
            .list_box
            .selected_row()
            .is_some_and(|row| row.is_visible());
        if !has_visible_selection {
            self.list_box.select_row(first_visible.as_ref());
        }
        update_empty_visibility(first_visible.is_some(), &self.scroller, self.empty.as_ref());
    }
}

fn build_selection_row(row_spec: &BigSearchableSelectionRowSpec) -> gtk::ListBoxRow {
    let mut spec = BigSearchResultRowSpec::new(row_spec.title.as_str())
        .accessible_label(
            row_spec
                .accessible_label
                .as_deref()
                .unwrap_or(row_spec.title.as_str()),
        )
        .title_css_classes(row_spec.title_css_classes.iter().map(String::as_str))
        .horizontal_margin(row_spec.horizontal_margin)
        .vertical_margin(row_spec.vertical_margin);
    if let Some(subtitle) = row_spec.subtitle.as_deref() {
        spec = spec.subtitle(subtitle);
    }
    BigSearchResultRow::new(spec).into_root()
}

fn apply_scroller_size(scroller: &gtk::ScrolledWindow, resolved: &BigSearchableSelectionListSpec) {
    if resolved.min_content_width > 0 {
        scroller.set_min_content_width(resolved.min_content_width);
    }
    if resolved.min_content_height > 0 {
        scroller.set_min_content_height(resolved.min_content_height);
    }
    if resolved.max_content_height > 0 {
        scroller.set_max_content_height(resolved.max_content_height);
    }
}

fn update_empty_visibility(
    has_visible_rows: bool,
    scroller: &gtk::ScrolledWindow,
    empty: Option<&adw::StatusPage>,
) {
    let Some(empty) = empty else {
        return;
    };
    scroller.set_visible(has_visible_rows);
    empty.set_visible(!has_visible_rows);
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
    fn selection_row_spec_clamps_layout_values() {
        let row = BigSearchableSelectionRowSpec::new("monospace", "Monospace", "monospace")
            .row_margins(-1, -2)
            .resolved();

        assert_eq!(row.horizontal_margin, 0);
        assert_eq!(row.vertical_margin, 0);
    }

    #[test]
    fn selection_list_spec_clamps_layout_values() {
        let spec = BigSearchableSelectionListSpec::new("Search", "Search", [])
            .spacing(-1)
            .margins(-2, -3, -4, -5)
            .scroller_size(-6, -7, -8)
            .resolved();

        assert_eq!(spec.spacing, 0);
        assert_eq!(spec.margin_top, 0);
        assert_eq!(spec.margin_bottom, 0);
        assert_eq!(spec.margin_start, 0);
        assert_eq!(spec.margin_end, 0);
        assert_eq!(spec.min_content_width, 0);
        assert_eq!(spec.min_content_height, 0);
        assert_eq!(spec.max_content_height, 0);
    }

    #[test]
    fn selection_row_keeps_id_title_and_search_text() {
        let row = BigSearchableSelectionRowSpec::new("fira-code", "Fira Code", "fira code mono")
            .subtitle("Regular")
            .accessible_label("Fira Code Regular");

        assert_eq!(row.id, "fira-code");
        assert_eq!(row.title, "Fira Code");
        assert_eq!(row.subtitle.as_deref(), Some("Regular"));
        assert_eq!(row.search_text, "fira code mono");
        assert_eq!(row.accessible_label.as_deref(), Some("Fira Code Regular"));
    }

    #[test]
    fn selection_query_matches_pre_normalized_text() {
        assert!(search_text_matches_query("jetbrains mono", "mono"));
        assert!(search_text_matches_query("jetbrains mono", ""));
        assert!(!search_text_matches_query("jetbrains mono", "fira"));
    }
}
