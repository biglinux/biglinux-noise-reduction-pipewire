// SPDX-License-Identifier: MIT

//! The assembled `Tabbed` window — the keystone payoff (RESTRUCTURE §13.3 item 3).
//!
//! [`BigPaneTabs`] mounts [`PaneContent`] leaves in the custom [`BigTabStrip`]
//! (RESTRUCTURE D15 — the shell uses big-terminal's custom tab strip, not
//! `adw::TabView`): each tab holds a [`PaneNode`] tree rendered as nested
//! `gtk::Paned` (so a tab can split terminal + editor + files side by side), and
//! the container OWNS every leaf's [`PaneGuard`], dropping them when the tab
//! closes — leak-safe by construction (INVARIANTS H9). Tab titles track
//! [`PaneContent::title`] live via [`PaneContent::connect_title_changed`].
//!
//! It is the generic container the three mount points share; the existing
//! display-free [`crate::layout::tab_workspace`] spec models *which* tabs exist,
//! while this widget answers *what is inside* them. A consumer builds content,
//! boxes it as `PaneContent`, and hands it over — no window/tab/split plumbing.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;

use super::{BigSplitOrientation, PaneContent, PaneGuard, PaneNode};
use crate::layout::split_panes::BigSplitPlacement;
use crate::layout::tab_strip_host::{BigTabPage, BigTabStrip};
use crate::layout::tab_workspace::BigTabId;

/// One realized tab: its [`BigTabPage`], the split-tree layout, and the guards
/// (keyed by leaf id) keeping its content alive. Dropping the entry drops the
/// guards → the content subtree tears down. `node`/`leaves` are interior-mutable
/// so a live split/close can re-render the tab in place.
struct TabEntry {
    page: Rc<BigTabPage>,
    node: RefCell<PaneNode>,
    leaves: RefCell<HashMap<BigTabId, PaneGuard>>,
}

/// A tabbed container that hosts `PaneContent` leaves, arrangeable in splits.
///
/// The root widget ([`root`](Self::root)) is a vertical box of the custom
/// [`BigTabStrip`] tab bar over its content stack. Mount single panes with
/// [`add_pane`](Self::add_pane) or split layouts with
/// [`add_split`](Self::add_split); both return the created [`BigTabPage`].
/// Closing a tab releases that tab's content (guards dropped on detach).
pub struct BigPaneTabs {
    root: gtk::Box,
    strip: BigTabStrip,
    tabs: Rc<RefCell<Vec<TabEntry>>>,
    /// Counter for synthesizing leaf ids for single-pane tabs.
    next_id: Cell<u64>,
}

impl Default for BigPaneTabs {
    fn default() -> Self {
        Self::new()
    }
}

impl BigPaneTabs {
    /// Build an empty tabbed container (tab strip + content stack, no tabs yet).
    #[must_use]
    pub fn new() -> Self {
        let strip = BigTabStrip::default();

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(strip.tab_bar());
        root.append(strip.content_widget());
        strip.content_widget().set_vexpand(true);

        let tabs: Rc<RefCell<Vec<TabEntry>>> = Rc::new(RefCell::new(Vec::new()));

        // On detach (close/transfer): drop that tab's guards so its content
        // subtree finalizes. The strip already removed the page + its child.
        let tabs_for_close = tabs.clone();
        strip.connect_page_detached(move |_strip, page, _pos| {
            tabs_for_close
                .borrow_mut()
                .retain(|entry| !Rc::ptr_eq(&entry.page, page));
        });

        Self {
            root,
            strip,
            tabs,
            next_id: Cell::new(0),
        }
    }

    /// Synthesize a fresh leaf id for a single-pane tab.
    fn fresh_id(&self) -> BigTabId {
        let n = self.next_id.get();
        self.next_id.set(n + 1);
        format!("pane-{n}")
    }

    /// The widget to embed (tab strip over the content stack).
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// The underlying tab strip (for chrome wiring, menu model, listeners).
    #[must_use]
    pub fn strip(&self) -> &BigTabStrip {
        &self.strip
    }

    /// Number of open tabs.
    #[must_use]
    pub fn tab_count(&self) -> usize {
        self.tabs.borrow().len()
    }

    /// Make `page` the visible tab.
    pub fn select_page(&self, page: &Rc<BigTabPage>) {
        self.strip.set_selected_page(page);
    }

    /// Mount a single content as a new tab. The tab title tracks the content's
    /// [`title`](PaneContent::title), updating live on change. The tab's layout
    /// is a single leaf (a synthesized id) which [`split_pane`](Self::split_pane)
    /// can later split.
    pub fn add_pane(&self, content: Box<dyn PaneContent>) -> Rc<BigTabPage> {
        let id = self.fresh_id();
        let guard = PaneGuard::new(content);
        let widget = guard.content().root();
        let page = self.strip.append(&widget);
        page.set_title(&guard.content().title());
        bind_title(&page, guard.content());
        let mut leaves = HashMap::new();
        leaves.insert(id.clone(), guard);
        self.tabs.borrow_mut().push(TabEntry {
            page: page.clone(),
            node: RefCell::new(PaneNode::leaf(id)),
            leaves: RefCell::new(leaves),
        });
        page
    }

    /// Mount a split layout as a new tab: `node` arranges `leaves` (keyed by
    /// their [`BigTabId`]) into nested `gtk::Paned`. The tab title tracks the
    /// FIRST leaf's content. A `Leaf` id not present in `leaves` renders a
    /// visible "missing pane" placeholder rather than panicking.
    pub fn add_split(
        &self,
        node: &PaneNode,
        leaves: Vec<(BigTabId, Box<dyn PaneContent>)>,
    ) -> Rc<BigTabPage> {
        let mut guards: HashMap<BigTabId, PaneGuard> = HashMap::new();
        let mut widgets: HashMap<BigTabId, gtk::Widget> = HashMap::new();
        let mut first_title: Option<String> = None;
        for (id, content) in leaves {
            let guard = PaneGuard::new(content);
            let widget = guard.content().root();
            first_title.get_or_insert_with(|| guard.content().title());
            widgets.insert(id.clone(), widget);
            guards.insert(id, guard);
        }

        let tree = render_pane_node(node, &|id| {
            widgets
                .get(id)
                .cloned()
                .unwrap_or_else(|| gtk::Label::new(Some(&format!("missing pane: {id}"))).upcast())
        });
        let page = self.strip.append(&tree);
        page.set_title(&first_title.unwrap_or_default());
        self.tabs.borrow_mut().push(TabEntry {
            page: page.clone(),
            node: RefCell::new(node.clone()),
            leaves: RefCell::new(guards),
        });
        page
    }

    /// Live-split the leaf `target` inside `page`'s tab, inserting `content` on
    /// the side given by `placement` at divider `ratio`, and re-render the tab.
    /// Returns the new leaf's id, or `None` if `page`/`target` is unknown.
    pub fn split_pane(
        &self,
        page: &Rc<BigTabPage>,
        target: &BigTabId,
        content: Box<dyn PaneContent>,
        placement: BigSplitPlacement,
        ratio: f32,
    ) -> Option<BigTabId> {
        let tabs = self.tabs.borrow();
        let entry = tabs.iter().find(|e| Rc::ptr_eq(&e.page, page))?;
        let new_id = self.fresh_id();
        if !entry
            .node
            .borrow_mut()
            .split_leaf(target, new_id.clone(), placement, ratio)
        {
            return None;
        }
        entry
            .leaves
            .borrow_mut()
            .insert(new_id.clone(), PaneGuard::new(content));
        self.rerender_tab(entry);
        Some(new_id)
    }

    /// Live-close the leaf `target` inside `page`'s tab, collapsing its split
    /// into the sibling and re-rendering. Closing the tab's last leaf closes the
    /// whole tab. Returns `false` if `page`/`target` is unknown.
    pub fn close_pane(&self, page: &Rc<BigTabPage>, target: &BigTabId) -> bool {
        let tabs = self.tabs.borrow();
        let Some(entry) = tabs.iter().find(|e| Rc::ptr_eq(&e.page, page)) else {
            return false;
        };
        let is_lone = matches!(&*entry.node.borrow(), PaneNode::Leaf(id) if id == target);
        if is_lone {
            drop(tabs);
            self.strip.close_page(page);
            return true;
        }
        if !entry.node.borrow_mut().remove_leaf(target) {
            return false;
        }
        entry.leaves.borrow_mut().remove(target);
        self.rerender_tab(entry);
        true
    }

    /// Re-render a tab's split tree from its current `node`+`leaves` and swap it
    /// into the strip's content stack (preserving selection). Pane content
    /// widgets survive (owned by the guards); only the `gtk::Paned` wrappers
    /// rebuild.
    ///
    /// Order matters: pull the old rendered root out of the stack and detach
    /// each live content widget from any leftover `gtk::Paned` BEFORE
    /// re-rendering, so `render_pane_node` re-homes parentless widgets (a still
    /// parented child trips `gtk_paned_set_*_child`).
    fn rerender_tab(&self, entry: &TabEntry) {
        self.strip.detach_page_child(&entry.page);
        let widgets: HashMap<BigTabId, gtk::Widget> = entry
            .leaves
            .borrow()
            .iter()
            .map(|(id, guard)| {
                let widget = guard.content().root();
                detach_content_widget(&widget);
                (id.clone(), widget)
            })
            .collect();
        let tree = render_pane_node(&entry.node.borrow(), &|id| {
            widgets
                .get(id)
                .cloned()
                .unwrap_or_else(|| gtk::Label::new(Some(&format!("missing pane: {id}"))).upcast())
        });
        self.strip.attach_page_child(&entry.page, &tree);
    }
}

/// Unparent a content widget from its current parent so it can be re-homed into
/// a freshly built split tree. Handles the `gtk::Paned` case (clear the matching
/// slot) and falls back to plain `unparent` for any other container.
fn detach_content_widget(widget: &gtk::Widget) {
    let Some(parent) = widget.parent() else {
        return;
    };
    if let Some(paned) = parent.downcast_ref::<gtk::Paned>() {
        if paned.start_child().as_ref() == Some(widget) {
            paned.set_start_child(gtk::Widget::NONE);
        } else if paned.end_child().as_ref() == Some(widget) {
            paned.set_end_child(gtk::Widget::NONE);
        }
    } else {
        widget.unparent();
    }
}

/// Wire a content's live title changes to the tab page label. The closure holds
/// only a WEAK page ref (no widget⇄closure cycle); it lives as long as the
/// content (owned by the guard), so it stops firing once the tab is dropped.
fn bind_title(page: &Rc<BigTabPage>, content: &dyn PaneContent) {
    let page_weak = Rc::downgrade(page);
    content.connect_title_changed(Box::new(move |title| {
        if let Some(page) = page_weak.upgrade() {
            page.set_title(title);
        }
    }));
}

/// Recursively realize a [`PaneNode`] into a widget tree: a `Leaf` calls
/// `resolve` for its content root (the caller maps an id to a `PaneContent` root,
/// or a placeholder for an unknown id); a `Split` builds a `gtk::Paned` of its
/// children at the node's ratio. Shared by [`BigPaneTabs`] (per tab) and the
/// big-shell background host (the whole surface) — RESTRUCTURE B4-T0.
///
/// Leak note: the returned widgets are owned by whatever the caller parents them
/// into; ratio application holds only a weak self-ref on the `gtk::Paned`.
pub fn render_pane_node(
    node: &PaneNode,
    resolve: &dyn Fn(&BigTabId) -> gtk::Widget,
) -> gtk::Widget {
    match node {
        PaneNode::Leaf(id) => resolve(id),
        PaneNode::Split {
            orientation,
            ratio,
            first,
            second,
        } => {
            let paned = gtk::Paned::new(gtk_orientation(*orientation));
            paned.set_start_child(Some(&render_pane_node(first, resolve)));
            paned.set_end_child(Some(&render_pane_node(second, resolve)));
            paned.set_resize_start_child(true);
            paned.set_resize_end_child(true);
            apply_ratio(&paned, *ratio, *orientation);
            paned.upcast()
        }
    }
}

fn gtk_orientation(orientation: BigSplitOrientation) -> gtk::Orientation {
    match orientation {
        BigSplitOrientation::Horizontal => gtk::Orientation::Horizontal,
        BigSplitOrientation::Vertical => gtk::Orientation::Vertical,
    }
}

/// Place the divider at `ratio` of the split axis once the paned has a non-zero
/// allocation (best-effort, applied once). The ratio is also preserved in the
/// `PaneNode` for persistence; live user-drag overrides it afterwards.
fn apply_ratio(paned: &gtk::Paned, ratio: f32, orientation: BigSplitOrientation) {
    let ratio = ratio.clamp(0.0, 1.0);
    let applied = Rc::new(Cell::new(false));
    paned.connect_map(move |paned| {
        if applied.get() {
            return;
        }
        let total = match orientation {
            BigSplitOrientation::Horizontal => paned.width(),
            BigSplitOrientation::Vertical => paned.height(),
        };
        if total > 0 {
            paned.set_position((ratio * total as f32).round() as i32);
            applied.set(true);
        }
    });
}
