// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Renderer for the shared GTK-free Big toolbar layout model.
//!
//! Consumers own the concrete tool widgets. This module owns only the four edge
//! mount bars, their accessibility metadata, CSS class contract, and the cheap
//! re-render pass that rehomes app widgets in layout order.

use big_app_kit::toolbar_layout::{BigToolbarEdge, BigToolbarLayout};
use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::i18n::{gettext_noop, t};

const DEFAULT_CSS_CLASS_PREFIX: &str = "big-custom-toolbar";
const DEFAULT_SPACING: i32 = 6;
const DEFAULT_MARGIN: i32 = 0;
const TOP_ACCESSIBLE_NAME: &str = gettext_noop("Top toolbar");
const BOTTOM_ACCESSIBLE_NAME: &str = gettext_noop("Bottom toolbar");
const START_ACCESSIBLE_NAME: &str = gettext_noop("Start toolbar");
const END_ACCESSIBLE_NAME: &str = gettext_noop("End toolbar");

/// Display contract for a custom toolbar renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigCustomToolbarSpec {
    css_class_prefix: String,
    top_accessible_name_msgid: &'static str,
    bottom_accessible_name_msgid: &'static str,
    start_accessible_name_msgid: &'static str,
    end_accessible_name_msgid: &'static str,
    accessible_names: Option<[String; 4]>,
    spacing: i32,
    margin_top: i32,
    margin_bottom: i32,
    margin_start: i32,
    margin_end: i32,
}

impl Default for BigCustomToolbarSpec {
    fn default() -> Self {
        Self {
            css_class_prefix: DEFAULT_CSS_CLASS_PREFIX.to_owned(),
            top_accessible_name_msgid: TOP_ACCESSIBLE_NAME,
            bottom_accessible_name_msgid: BOTTOM_ACCESSIBLE_NAME,
            start_accessible_name_msgid: START_ACCESSIBLE_NAME,
            end_accessible_name_msgid: END_ACCESSIBLE_NAME,
            accessible_names: None,
            spacing: DEFAULT_SPACING,
            margin_top: DEFAULT_MARGIN,
            margin_bottom: DEFAULT_MARGIN,
            margin_start: DEFAULT_MARGIN,
            margin_end: DEFAULT_MARGIN,
        }
    }
}

impl BigCustomToolbarSpec {
    /// Create a custom toolbar spec with shared defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the CSS class prefix used for every edge bar.
    #[must_use]
    pub fn css_class_prefix(mut self, css_class_prefix: impl Into<String>) -> Self {
        self.css_class_prefix = css_class_prefix.into();
        self
    }

    /// Set the component textdomain msgids used as accessible bar names.
    #[must_use]
    pub fn accessible_name_msgids(
        mut self,
        top: &'static str,
        bottom: &'static str,
        start: &'static str,
        end: &'static str,
    ) -> Self {
        self.top_accessible_name_msgid = top;
        self.bottom_accessible_name_msgid = bottom;
        self.start_accessible_name_msgid = start;
        self.end_accessible_name_msgid = end;
        self
    }

    /// Set pre-translated accessible bar names from the app's own textdomain.
    /// Non-empty entries override the component msgids.
    #[must_use]
    pub fn accessible_names(
        mut self,
        top: impl Into<String>,
        bottom: impl Into<String>,
        start: impl Into<String>,
        end: impl Into<String>,
    ) -> Self {
        self.accessible_names = Some([top.into(), bottom.into(), start.into(), end.into()]);
        self
    }

    /// Set spacing between toolbar widgets.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Set all edge-bar margins to the same value.
    #[must_use]
    pub fn margins(mut self, margin: i32) -> Self {
        self.margin_top = margin;
        self.margin_bottom = margin;
        self.margin_start = margin;
        self.margin_end = margin;
        self
    }

    /// Set individual edge-bar margins.
    #[must_use]
    pub fn edge_margins(mut self, top: i32, bottom: i32, start: i32, end: i32) -> Self {
        self.margin_top = top;
        self.margin_bottom = bottom;
        self.margin_start = start;
        self.margin_end = end;
        self
    }

    fn resolve(&self) -> BigCustomToolbarResolved {
        let css_class_prefix = if self.css_class_prefix.trim().is_empty() {
            DEFAULT_CSS_CLASS_PREFIX.to_owned()
        } else {
            self.css_class_prefix.clone()
        };
        let override_name = |slot: usize| {
            self.accessible_names
                .as_ref()
                .map(|names| names[slot].trim().to_owned())
                .filter(|name| !name.is_empty())
        };
        BigCustomToolbarResolved {
            css_class_prefix,
            top_accessible_name: override_name(0).unwrap_or_else(|| {
                t(non_empty_msgid(
                    self.top_accessible_name_msgid,
                    TOP_ACCESSIBLE_NAME,
                ))
            }),
            bottom_accessible_name: override_name(1).unwrap_or_else(|| {
                t(non_empty_msgid(
                    self.bottom_accessible_name_msgid,
                    BOTTOM_ACCESSIBLE_NAME,
                ))
            }),
            start_accessible_name: override_name(2).unwrap_or_else(|| {
                t(non_empty_msgid(
                    self.start_accessible_name_msgid,
                    START_ACCESSIBLE_NAME,
                ))
            }),
            end_accessible_name: override_name(3).unwrap_or_else(|| {
                t(non_empty_msgid(
                    self.end_accessible_name_msgid,
                    END_ACCESSIBLE_NAME,
                ))
            }),
            spacing: self.spacing.max(0),
            margin_top: self.margin_top.max(0),
            margin_bottom: self.margin_bottom.max(0),
            margin_start: self.margin_start.max(0),
            margin_end: self.margin_end.max(0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BigCustomToolbarResolved {
    css_class_prefix: String,
    top_accessible_name: String,
    bottom_accessible_name: String,
    start_accessible_name: String,
    end_accessible_name: String,
    spacing: i32,
    margin_top: i32,
    margin_bottom: i32,
    margin_start: i32,
    margin_end: i32,
}

/// Four toolbar edge bars backed by [`BigToolbarLayout`].
#[derive(Clone)]
pub struct BigCustomToolbar {
    top: gtk::Box,
    bottom: gtk::Box,
    start: gtk::Box,
    end: gtk::Box,
}

impl BigCustomToolbar {
    /// Build the four toolbar edge containers from `spec`.
    #[must_use]
    pub fn new(spec: BigCustomToolbarSpec) -> Self {
        let resolved = spec.resolve();
        Self {
            top: build_edge_bar(
                BigToolbarEdge::Top,
                &resolved,
                &resolved.top_accessible_name,
            ),
            bottom: build_edge_bar(
                BigToolbarEdge::Bottom,
                &resolved,
                &resolved.bottom_accessible_name,
            ),
            start: build_edge_bar(
                BigToolbarEdge::Start,
                &resolved,
                &resolved.start_accessible_name,
            ),
            end: build_edge_bar(
                BigToolbarEdge::End,
                &resolved,
                &resolved.end_accessible_name,
            ),
        }
    }

    /// Rebuild all visible edge bars from `layout`, in layout order.
    ///
    /// `widget_for` maps a toolbar id to its live app-owned widget. Missing ids
    /// are skipped, and empty edge bars are hidden.
    pub fn render(
        &self,
        layout: &BigToolbarLayout,
        widget_for: &dyn Fn(&str) -> Option<gtk::Widget>,
    ) {
        for bar in [&self.top, &self.bottom, &self.start, &self.end] {
            crate::layout::widget_container::clear_box_children(bar);
        }
        self.render_edge(BigToolbarEdge::Top, layout, widget_for);
        self.render_edge(BigToolbarEdge::Bottom, layout, widget_for);
        self.render_edge(BigToolbarEdge::Start, layout, widget_for);
        self.render_edge(BigToolbarEdge::End, layout, widget_for);
    }

    /// Borrow the top edge root.
    #[must_use]
    pub fn top_root(&self) -> &gtk::Box {
        &self.top
    }

    /// Borrow the bottom edge root.
    #[must_use]
    pub fn bottom_root(&self) -> &gtk::Box {
        &self.bottom
    }

    /// Borrow the leading-side edge root.
    #[must_use]
    pub fn start_root(&self) -> &gtk::Box {
        &self.start
    }

    /// Borrow the trailing-side edge root.
    #[must_use]
    pub fn end_root(&self) -> &gtk::Box {
        &self.end
    }

    fn render_edge(
        &self,
        edge: BigToolbarEdge,
        layout: &BigToolbarLayout,
        widget_for: &dyn Fn(&str) -> Option<gtk::Widget>,
    ) {
        let bar = self.bar_for(edge);
        let mut has_children = false;
        for id in layout.items_on(edge) {
            if let Some(widget) = widget_for(id) {
                bar.append(&widget);
                has_children = true;
            }
        }
        bar.set_visible(has_children);
    }

    fn bar_for(&self, edge: BigToolbarEdge) -> &gtk::Box {
        match edge {
            BigToolbarEdge::Top => &self.top,
            BigToolbarEdge::Bottom => &self.bottom,
            BigToolbarEdge::Start => &self.start,
            BigToolbarEdge::End => &self.end,
            BigToolbarEdge::Hidden => unreachable!("hidden toolbar edge is not rendered"),
        }
    }
}

fn build_edge_bar(
    edge: BigToolbarEdge,
    resolved: &BigCustomToolbarResolved,
    accessible_name: &str,
) -> gtk::Box {
    let edge_class = format!("{}-{}", resolved.css_class_prefix, edge_css_suffix(edge));
    let bar = gtk::Box::builder()
        .orientation(if edge.is_horizontal() {
            gtk::Orientation::Horizontal
        } else {
            gtk::Orientation::Vertical
        })
        .spacing(resolved.spacing)
        .margin_top(resolved.margin_top)
        .margin_bottom(resolved.margin_bottom)
        .margin_start(resolved.margin_start)
        .margin_end(resolved.margin_end)
        .visible(false)
        .css_classes([resolved.css_class_prefix.as_str(), edge_class.as_str()])
        .build();
    bar.set_accessible_role(gtk::AccessibleRole::Toolbar);
    bar.update_property(&[gtk::accessible::Property::Label(accessible_name)]);
    bar
}

#[cfg(test)]
fn visible_toolbar_ids<'a>(
    layout: &'a BigToolbarLayout,
    edge: BigToolbarEdge,
    has_widget: &dyn Fn(&str) -> bool,
) -> Vec<&'a str> {
    if edge == BigToolbarEdge::Hidden {
        return Vec::new();
    }
    layout
        .items_on(edge)
        .iter()
        .map(String::as_str)
        .filter(|id| has_widget(id))
        .collect()
}

fn edge_css_suffix(edge: BigToolbarEdge) -> &'static str {
    match edge {
        BigToolbarEdge::Top => "top",
        BigToolbarEdge::Bottom => "bottom",
        BigToolbarEdge::Start => "start",
        BigToolbarEdge::End => "end",
        BigToolbarEdge::Hidden => "hidden",
    }
}

fn non_empty_msgid(msgid: &'static str, fallback: &'static str) -> &'static str {
    if msgid.trim().is_empty() {
        fallback
    } else {
        msgid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_app_kit::toolbar_layout::{BigToolbarCatalog, BigToolbarMigrations};

    const KNOWN: &[&str] = &["open", "search", "zoom", "status", "details"];

    fn catalog() -> BigToolbarCatalog<'static> {
        BigToolbarCatalog {
            version: 1,
            known: KNOWN,
            locked_hidden: &[],
            default_top: &["open", "search"],
            default_bottom: &["zoom", "status"],
            default_start: &["details"],
            default_end: &[],
            default_hidden: &[],
            append_missing_to: BigToolbarEdge::Hidden,
            migrations: BigToolbarMigrations {
                pre_v2_hide: &[],
                pre_v3_top: None,
                pre_v4_restore_top_if_hidden: &[],
                pre_v5_hide: &[],
                pre_v2_restore_top_if_hidden: None,
                required_top_if_hidden: &[],
            },
        }
    }

    fn gtk_ready() -> bool {
        gtk::init().is_ok()
    }

    #[test]
    fn visible_toolbar_ids_preserve_layout_order_and_skip_missing_widgets() {
        let layout = BigToolbarLayout::default_for(catalog());

        let ids = visible_toolbar_ids(&layout, BigToolbarEdge::Top, &|id| id != "open");

        assert_eq!(ids, vec!["search"]);
    }

    #[test]
    fn visible_toolbar_ids_do_not_render_hidden_edge() {
        let layout = BigToolbarLayout::default_for(catalog());

        let ids = visible_toolbar_ids(&layout, BigToolbarEdge::Hidden, &|_| true);

        assert!(ids.is_empty());
    }

    #[test]
    fn spec_sanitizes_empty_css_class_and_negative_spacing() {
        let resolved = BigCustomToolbarSpec::new()
            .css_class_prefix(" ")
            .spacing(-3)
            .margins(-2)
            .accessible_name_msgids("", "Bottom", "Start", "End")
            .resolve();

        assert_eq!(resolved.css_class_prefix, DEFAULT_CSS_CLASS_PREFIX);
        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.margin_top, 0);
        assert_eq!(resolved.top_accessible_name, "Top toolbar");
    }

    #[test]
    fn spec_accessible_names_override_msgids_per_slot() {
        let resolved = BigCustomToolbarSpec::new()
            .accessible_names("Command toolbar", " ", "Start bar", "")
            .resolve();

        assert_eq!(resolved.top_accessible_name, "Command toolbar");
        assert_eq!(resolved.bottom_accessible_name, "Bottom toolbar");
        assert_eq!(resolved.start_accessible_name, "Start bar");
        assert_eq!(resolved.end_accessible_name, "End toolbar");
    }

    #[test]
    fn render_hides_empty_edges_and_mounts_widgets_in_order() {
        if !gtk_ready() {
            return;
        }
        let layout = BigToolbarLayout::default_for(catalog());
        let toolbar = BigCustomToolbar::new(BigCustomToolbarSpec::new());

        toolbar.render(&layout, &|id| {
            (id != "open").then(|| gtk::Label::new(Some(id)).upcast())
        });

        assert!(toolbar.top_root().is_visible());
        let top_child = toolbar
            .top_root()
            .first_child()
            .and_downcast::<gtk::Label>()
            .expect("top toolbar should mount the search label");
        assert_eq!(top_child.text(), "search");
        assert!(toolbar.bottom_root().is_visible());
        assert!(toolbar.start_root().is_visible());
        assert!(!toolbar.end_root().is_visible());
    }

    #[test]
    fn render_clears_all_edges_before_reparenting_live_widgets() {
        if !gtk_ready() {
            return;
        }
        let mut layout = BigToolbarLayout::default_for(catalog());
        let toolbar = BigCustomToolbar::new(BigCustomToolbarSpec::new());
        let open = gtk::Label::new(Some("open"));
        let search = gtk::Label::new(Some("search"));
        let zoom = gtk::Label::new(Some("zoom"));
        let status = gtk::Label::new(Some("status"));
        let details = gtk::Label::new(Some("details"));
        let widget_for = |id: &str| match id {
            "open" => Some(open.clone().upcast()),
            "search" => Some(search.clone().upcast()),
            "zoom" => Some(zoom.clone().upcast()),
            "status" => Some(status.clone().upcast()),
            "details" => Some(details.clone().upcast()),
            _ => None,
        };

        toolbar.render(&layout, &widget_for);
        layout.set_edge(catalog(), "search", BigToolbarEdge::Bottom);
        toolbar.render(&layout, &widget_for);

        assert_eq!(
            search.parent(),
            Some(toolbar.bottom_root().clone().upcast())
        );
        assert_eq!(open.parent(), Some(toolbar.top_root().clone().upcast()));
    }
}
