// SPDX-License-Identifier: MIT

//! [`BigSplitTree`] — the live-widget split arrangement of panes within one
//! container.
//!
//! Where [`crate::pane::PaneNode`] is the split layout as *pure data* (serde-
//! friendly, testable), `BigSplitTree` is its *imperative widget* counterpart:
//! it mutates a live `gtk::Paned` tree in place as panes are split, closed,
//! zoomed, or moved between containers — preserving each pane's widget state
//! across the operation (a re-render-the-whole-tree approach would not).
//!
//! Promoted from big-terminal's terminal shell (the pane-tree it shipped is the most
//! capable split container in the suite); generic over [`SplitPaneSurface`] so the
//! editor and file-manager shells (and `BigPaneTabs`) can reuse the exact same
//! mechanics. The only thing the tree needs from a pane is the [`SplitPaneSurface`]
//! surface; everything terminal/editor/file-manager-specific stays in the consumer.
//!
//! # Focus CSS contract
//!
//! When a tab is split, the focused pane carries the `pane-focused` CSS class
//! (added on focus-enter, removed from siblings); single-pane tabs carry none
//! (per [`BigPaneHeaderPolicy::WhenSplit`]). Consumers style `.pane-focused` to
//! indicate the active pane.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::gtk::prelude::*;
use relm4::gtk::{Box as GtkBox, EventControllerFocus, Orientation, Paned, Widget};

use crate::layout::split_panes::{BigPaneHeaderPolicy, BigSplitPlacement};

/// What [`BigSplitTree`] needs from a pane to host it in the split layout: a
/// root widget to (un)parent, header-visibility control for the split-vs-single
/// chrome policy, and focus handoff. Decouples the generic split mechanics from
/// any concrete pane type, so the same tree drives terminal / editor /
/// file-manager shells. Implementors keep their own content, title, and
/// activity concerns; the tree only arranges roots and tracks focus.
pub trait SplitPaneSurface {
    /// Root widget mounted into the `gtk::Paned` slots / tab container.
    fn pane_root(&self) -> Widget;
    /// Show / hide the per-pane header (visible only while the tab is split).
    fn set_header_visible(&self, visible: bool);
    /// Move keyboard focus into this pane's content.
    fn grab_focus(&self);
}

/// Clear the focus on `widget`'s toplevel, if it has one. Called before
/// unparenting a focused pane subtree: GTK emits a `set_focus_child(nil)`
/// warning if a `GtkPaned` child is removed while it (or a descendant) still
/// holds focus. The caller refocuses the surviving pane afterwards.
fn clear_toplevel_focus(widget: &Widget) {
    if let Some(root) = widget.root() {
        root.set_focus(None::<&Widget>);
    }
}

/// A pane temporarily zoomed to fill its tab: the rest of the split subtree is
/// detached and parked, and the zoomed pane is reparented into the container
/// until [`BigSplitTree::toggle_zoom`] restores it to `slot_parent`.
struct ZoomState<P: SplitPaneSurface + 'static> {
    saved_subtree: Widget,
    slot_parent: Paned,
    slot_is_start: bool,
    pane: Rc<P>,
}

/// A live `gtk::Paned` split tree of [`SplitPaneSurface`] panes inside one tab /
/// container. Owns the panes (`Rc<P>`), tracks the focused one, and performs
/// incremental split / close / zoom / subtree-move by mutating the widget tree
/// in place. Construct with [`new`](Self::new); the container widget is
/// [`root_widget`](Self::root_widget).
pub struct BigSplitTree<P: SplitPaneSurface + 'static> {
    container: GtkBox,
    panes: RefCell<Vec<Rc<P>>>,
    focused: RefCell<Option<Rc<P>>>,
    zoom: RefCell<Option<ZoomState<P>>>,
    on_focus_changed: RefCell<Option<Rc<dyn Fn()>>>,
}

impl<P: SplitPaneSurface + 'static> BigSplitTree<P> {
    /// Build a single-pane tree rooted at `pane`. The pane's header starts
    /// hidden (a lone pane needs none); focus is tracked from creation.
    #[must_use]
    pub fn new(pane: &Rc<P>) -> Rc<Self> {
        let container = GtkBox::new(Orientation::Vertical, 0);
        container.add_css_class("big-split-tree");
        container.append(&pane.pane_root());
        pane.set_header_visible(false);
        let body = Rc::new(Self {
            container,
            panes: RefCell::new(vec![pane.clone()]),
            focused: RefCell::new(Some(pane.clone())),
            zoom: RefCell::new(None),
            on_focus_changed: RefCell::new(None),
        });
        body.attach_focus_tracker(pane);
        body
    }

    /// Set the callback fired when the focused pane changes (or when a pane is
    /// re-activated). Used to refresh window chrome bound to the active pane.
    pub fn set_on_focus_changed(&self, cb: Rc<dyn Fn()>) {
        *self.on_focus_changed.borrow_mut() = Some(cb);
    }

    /// The container widget holding the split tree — mount this in the tab.
    #[must_use]
    pub fn root_widget(&self) -> &GtkBox {
        &self.container
    }

    /// The currently focused pane, falling back to the first pane.
    #[must_use]
    pub fn focused_pane(&self) -> Option<Rc<P>> {
        self.focused
            .borrow()
            .clone()
            .or_else(|| self.panes.borrow().first().cloned())
    }

    /// All panes in the tree, in tree order.
    #[must_use]
    pub fn panes(&self) -> Vec<Rc<P>> {
        self.panes.borrow().clone()
    }

    /// Whether `pane` belongs to this tree (by `Rc` identity).
    #[must_use]
    pub fn contains_pane(&self, pane: &Rc<P>) -> bool {
        self.panes.borrow().iter().any(|p| Rc::ptr_eq(p, pane))
    }

    /// Split the focused pane along `orientation`, inserting `new_pane`.
    pub fn split(self: &Rc<Self>, orientation: Orientation, new_pane: &Rc<P>) {
        if let Some(target) = self.focused_pane() {
            self.split_at_pane(&target, orientation, new_pane);
        }
    }

    /// Split `target` along `orientation`, inserting `new_pane` after it.
    /// Returns the inserted `gtk::Paned`, or `None` if `target` has no parent.
    pub fn split_at_pane(
        self: &Rc<Self>,
        target: &Rc<P>,
        orientation: Orientation,
        new_pane: &Rc<P>,
    ) -> Option<Paned> {
        self.split_existing_at_pane(target, orientation, new_pane, false)
    }

    /// Split `target` along `orientation`, inserting `new_pane` on the side
    /// given by `new_pane_first`. Returns the inserted `gtk::Paned`, or `None`
    /// if `target`'s parent is not a recognized container.
    pub fn split_existing_at_pane(
        self: &Rc<Self>,
        target: &Rc<P>,
        orientation: Orientation,
        new_pane: &Rc<P>,
        new_pane_first: bool,
    ) -> Option<Paned> {
        if self.zoom.borrow().is_some() {
            self.restore_zoom();
        }
        let target_widget: Widget = target.pane_root();
        let parent = target_widget.parent()?;
        let paned = Paned::builder()
            .orientation(orientation)
            .resize_start_child(true)
            .resize_end_child(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .build();
        if !attach_split(
            &parent,
            &target_widget,
            &paned,
            &new_pane.pane_root(),
            new_pane_first,
        ) {
            log::warn!(
                "split aborted: unexpected pane parent `{}`",
                parent.type_().name()
            );
            return None;
        }
        self.panes.borrow_mut().push(new_pane.clone());
        *self.focused.borrow_mut() = Some(new_pane.clone());
        self.attach_focus_tracker(new_pane);
        self.refresh_header_visibility();
        new_pane.grab_focus();
        Some(paned)
    }

    /// Split `target` using the shared placement contract, inserting
    /// `new_pane` on the edge described by `placement`.
    pub fn split_existing_at_pane_with_placement(
        self: &Rc<Self>,
        target: &Rc<P>,
        placement: BigSplitPlacement,
        new_pane: &Rc<P>,
    ) -> Option<Paned> {
        self.split_existing_at_pane(
            target,
            placement.gtk_orientation(),
            new_pane,
            placement.places_new_pane_before_target(),
        )
    }

    /// Split `target` along `orientation`, inserting an existing widget
    /// `subtree` (with its `subtree_panes`, moved here from another tree via
    /// [`take_subtree`](Self::take_subtree)). Returns the inserted `gtk::Paned`,
    /// or `None` if `target` is absent or `subtree_panes` is empty.
    pub fn split_subtree_at_pane(
        self: &Rc<Self>,
        target: &Rc<P>,
        orientation: Orientation,
        subtree: &Widget,
        subtree_panes: Vec<Rc<P>>,
        subtree_first: bool,
    ) -> Option<Paned> {
        if self.zoom.borrow().is_some() {
            self.restore_zoom();
        }
        if !self.contains_pane(target) || subtree_panes.is_empty() {
            return None;
        }
        let target_widget: Widget = target.pane_root();
        let parent = target_widget.parent()?;
        let paned = Paned::builder()
            .orientation(orientation)
            .resize_start_child(true)
            .resize_end_child(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .build();
        if !attach_split(&parent, &target_widget, &paned, subtree, subtree_first) {
            log::warn!(
                "split-subtree aborted: unexpected pane parent `{}`",
                parent.type_().name()
            );
            return None;
        }
        let first_pane = subtree_panes.first().cloned();
        {
            let mut pane_list = self.panes.borrow_mut();
            pane_list.extend(subtree_panes.iter().cloned());
        }
        for pane in subtree_panes {
            self.attach_focus_tracker(&pane);
        }
        if let Some(pane) = first_pane {
            *self.focused.borrow_mut() = Some(pane.clone());
            pane.grab_focus();
        }
        self.refresh_header_visibility();
        Some(paned)
    }

    /// Split `target` using the shared placement contract, inserting an
    /// existing `subtree` on the edge described by `placement`.
    pub fn split_subtree_at_pane_with_placement(
        self: &Rc<Self>,
        target: &Rc<P>,
        placement: BigSplitPlacement,
        subtree: &Widget,
        subtree_panes: Vec<Rc<P>>,
    ) -> Option<Paned> {
        self.split_subtree_at_pane(
            target,
            placement.gtk_orientation(),
            subtree,
            subtree_panes,
            placement.places_new_pane_before_target(),
        )
    }

    /// Detach the whole split subtree and its panes from this tree, returning
    /// them so the caller can graft them into another tree (cross-tab move).
    /// Leaves this tree empty.
    pub fn take_subtree(&self) -> Option<(Widget, Vec<Rc<P>>)> {
        if self.zoom.borrow().is_some() {
            self.restore_zoom();
        }
        let subtree = self.container.first_child()?;
        self.container.remove(&subtree);
        let panes: Vec<Rc<P>> = self.panes.borrow_mut().drain(..).collect();
        for pane in &panes {
            pane.pane_root().remove_css_class("pane-focused");
            pane.set_header_visible(false);
        }
        *self.focused.borrow_mut() = None;
        Some((subtree, panes))
    }

    /// Toggle zoom: maximize the focused pane to fill the tab (parking the rest
    /// of the subtree), or restore the previously zoomed layout.
    pub fn toggle_zoom(self: &Rc<Self>) {
        if self.zoom.borrow().is_some() {
            self.restore_zoom();
            return;
        }
        let Some(target) = self.focused_pane() else {
            return;
        };
        let target_widget: Widget = target.pane_root();
        let Some(slot_parent) = target_widget
            .parent()
            .and_then(|p| p.downcast::<Paned>().ok())
        else {
            return;
        };
        let slot_is_start = slot_parent
            .start_child()
            .is_some_and(|c| c == target_widget);
        if slot_is_start {
            slot_parent.set_start_child(None::<&Widget>);
        } else {
            slot_parent.set_end_child(None::<&Widget>);
        }
        let Some(saved_subtree) = self.container.first_child() else {
            return;
        };
        self.container.remove(&saved_subtree);
        self.container.append(&target_widget);
        target.grab_focus();
        *self.zoom.borrow_mut() = Some(ZoomState {
            saved_subtree,
            slot_parent,
            slot_is_start,
            pane: target,
        });
    }

    fn restore_zoom(&self) {
        let Some(state) = self.zoom.borrow_mut().take() else {
            return;
        };
        let pane_widget: Widget = state.pane.pane_root();
        self.container.remove(&pane_widget);
        if state.slot_is_start {
            state.slot_parent.set_start_child(Some(&pane_widget));
        } else {
            state.slot_parent.set_end_child(Some(&pane_widget));
        }
        self.container.append(&state.saved_subtree);
        state.pane.grab_focus();
    }

    /// Detach the focused pane from the tree (collapsing its parent split to the
    /// sibling) and return it — the caller re-homes it elsewhere. `None` if the
    /// tree has ≤1 pane.
    pub fn detach_focused(&self) -> Option<Rc<P>> {
        let target = self.focused_pane()?;
        self.detach_pane(&target)
    }

    /// Detach `target` from the tree, collapsing its parent split into the
    /// surviving sibling, and return it. `None` if `target` is absent or the
    /// lone pane.
    pub fn detach_pane(&self, target: &Rc<P>) -> Option<Rc<P>> {
        if self.zoom.borrow().is_some() {
            self.restore_zoom();
        }
        if !self.panes.borrow().iter().any(|p| Rc::ptr_eq(p, target)) {
            return None;
        }
        if self.panes.borrow().len() <= 1 {
            return None;
        }
        let target_widget: Widget = target.pane_root();
        let parent_paned = target_widget
            .parent()
            .and_then(|p| p.downcast::<Paned>().ok())?;
        let grand = parent_paned.parent()?;
        let was_start = parent_paned
            .start_child()
            .is_some_and(|c| c == target_widget);
        let sibling = if was_start {
            parent_paned.end_child()
        } else {
            parent_paned.start_child()
        }?;
        // See `close_pane`: clear focus before unparenting the focused subtree.
        clear_toplevel_focus(&target_widget);
        parent_paned.set_start_child(None::<&Widget>);
        parent_paned.set_end_child(None::<&Widget>);

        if let Some(box_grand) = grand.downcast_ref::<GtkBox>() {
            box_grand.remove(&parent_paned);
            box_grand.append(&sibling);
        } else if let Some(paned_grand) = grand.downcast_ref::<Paned>() {
            let parent_was_start = paned_grand
                .start_child()
                .is_some_and(|c| c == parent_paned.clone().upcast::<Widget>());
            if parent_was_start {
                paned_grand.set_start_child(Some(&sibling));
            } else {
                paned_grand.set_end_child(Some(&sibling));
            }
        } else {
            log::warn!(
                "detach-pane aborted: unexpected paned parent `{}`",
                grand.type_().name()
            );
            return None;
        }

        self.panes.borrow_mut().retain(|p| !Rc::ptr_eq(p, target));
        target.pane_root().remove_css_class("pane-focused");
        target.set_header_visible(false);
        let next = self.panes.borrow().first().cloned();
        if let Some(ref pane) = next {
            pane.grab_focus();
        }
        *self.focused.borrow_mut() = next;
        self.refresh_header_visibility();
        Some(target.clone())
    }

    /// Close (drop) the focused pane, collapsing its parent split to the
    /// sibling. Returns `false` if the tree has ≤1 pane.
    pub fn close_focused(&self) -> bool {
        let Some(target) = self.focused_pane() else {
            return false;
        };
        self.close_pane(&target)
    }

    /// Close (drop) `target`, collapsing its parent split into the surviving
    /// sibling. Returns `false` if `target` is absent or the lone pane.
    pub fn close_pane(&self, target: &Rc<P>) -> bool {
        if self.zoom.borrow().is_some() {
            self.restore_zoom();
        }
        if !self.panes.borrow().iter().any(|p| Rc::ptr_eq(p, target)) {
            return false;
        }
        if self.panes.borrow().len() <= 1 {
            return false;
        }
        let target_widget: Widget = target.pane_root();
        let Some(parent_paned) = target_widget
            .parent()
            .and_then(|p| p.downcast::<Paned>().ok())
        else {
            return false;
        };
        let Some(grand) = parent_paned.parent() else {
            return false;
        };
        let was_start = parent_paned
            .start_child()
            .is_some_and(|c| c == target_widget);
        let sibling = if was_start {
            parent_paned.end_child()
        } else {
            parent_paned.start_child()
        };
        let Some(sibling) = sibling else {
            return false;
        };
        // Clear toplevel focus before unparenting. GTK warns
        // ("gtk_paned_set_focus_child … on widget (nil)") if a GtkPaned child is
        // detached while it (or a descendant) still holds focus — closing the
        // focused pane does exactly that. The survivor is refocused below.
        clear_toplevel_focus(&target_widget);
        parent_paned.set_start_child(None::<&Widget>);
        parent_paned.set_end_child(None::<&Widget>);

        if let Some(box_grand) = grand.downcast_ref::<GtkBox>() {
            box_grand.remove(&parent_paned);
            box_grand.append(&sibling);
        } else if let Some(paned_grand) = grand.downcast_ref::<Paned>() {
            let parent_was_start = paned_grand
                .start_child()
                .is_some_and(|c| c == parent_paned.clone().upcast::<Widget>());
            if parent_was_start {
                paned_grand.set_start_child(Some(&sibling));
            } else {
                paned_grand.set_end_child(Some(&sibling));
            }
        } else {
            log::warn!(
                "close-pane aborted: unexpected paned parent `{}`",
                grand.type_().name()
            );
            return false;
        }

        self.panes.borrow_mut().retain(|p| !Rc::ptr_eq(p, target));
        let next = self.panes.borrow().first().cloned();
        if self.panes.borrow().len() <= 1 {
            for pane in self.panes.borrow().iter() {
                pane.pane_root().remove_css_class("pane-focused");
            }
        }
        if let Some(ref pane) = next {
            pane.grab_focus();
        }
        *self.focused.borrow_mut() = next;
        self.refresh_header_visibility();
        true
    }

    fn activate_pane(&self, pane: Rc<P>, notify_if_unchanged: bool) {
        let changed = self
            .focused
            .borrow()
            .as_ref()
            .is_none_or(|current| !Rc::ptr_eq(current, &pane));
        for sibling in self.panes.borrow().iter() {
            sibling.pane_root().remove_css_class("pane-focused");
        }
        if BigPaneHeaderPolicy::WhenSplit.is_visible(self.panes.borrow().len()) {
            pane.pane_root().add_css_class("pane-focused");
        }
        *self.focused.borrow_mut() = Some(pane);
        if changed || notify_if_unchanged {
            let cb = self.on_focus_changed.borrow().clone();
            if let Some(cb) = cb {
                cb();
            }
        }
    }

    fn attach_focus_tracker(self: &Rc<Self>, pane: &Rc<P>) {
        let focus_ctrl = EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        let pane_weak = Rc::downgrade(pane);
        focus_ctrl.connect_enter(move |_| {
            let Some(body) = weak.upgrade() else {
                return;
            };
            let Some(pane) = pane_weak.upgrade() else {
                return;
            };
            body.activate_pane(pane, true);
        });
        pane.pane_root().add_controller(focus_ctrl);
    }

    fn refresh_header_visibility(&self) {
        let panes = self.panes.borrow();
        let visible = BigPaneHeaderPolicy::WhenSplit.is_visible(panes.len());
        for pane in panes.iter() {
            pane.set_header_visible(visible);
        }
    }
}

/// Reparent `target` under a new `paned`, placing `sibling` on the other side,
/// where `target`'s current `parent` is either a `gtk::Box` (the tree root) or
/// a `gtk::Paned` slot. Returns `false` for an unrecognized parent type.
fn attach_split(
    parent: &Widget,
    target: &Widget,
    paned: &Paned,
    sibling: &impl IsA<Widget>,
    sibling_first: bool,
) -> bool {
    if let Some(box_parent) = parent.downcast_ref::<GtkBox>() {
        box_parent.remove(target);
        if sibling_first {
            paned.set_start_child(Some(sibling));
            paned.set_end_child(Some(target));
        } else {
            paned.set_start_child(Some(target));
            paned.set_end_child(Some(sibling));
        }
        box_parent.append(paned);
        true
    } else if let Some(paned_parent) = parent.downcast_ref::<Paned>() {
        let was_start = paned_parent.start_child().is_some_and(|c| &c == target);
        if was_start {
            paned_parent.set_start_child(None::<&Widget>);
        } else {
            paned_parent.set_end_child(None::<&Widget>);
        }
        if sibling_first {
            paned.set_start_child(Some(sibling));
            paned.set_end_child(Some(target));
        } else {
            paned.set_start_child(Some(target));
            paned.set_end_child(Some(sibling));
        }
        if was_start {
            paned_parent.set_start_child(Some(paned));
        } else {
            paned_parent.set_end_child(Some(paned));
        }
        true
    } else {
        false
    }
}
