// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Rich layout capture/restore for a [`BigSplitTree`] — the terminal's saved
//! layout mechanism promoted to the shared layer: the live `gtk::Paned` tree
//! serializes to a JSON node tree (orientation + split ratio + per-pane app
//! payload) and restores by re-splitting from the first leaf.
//!
//! What a pane *is* stays app-side: capture asks `pane_payload(pane)` (return
//! `None` to drop a pane, e.g. a companion the app cannot rebuild); restore
//! asks `build_pane(payload)` for each leaf. A dropped child collapses its
//! `Paned` node so the sibling takes the slot, matching the terminal.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk::glib::{self, ControlFlow};
use relm4::gtk::{Orientation, Paned, Widget};
use serde_json::{Value, json};

use super::split_tree::{BigSplitTree, SplitPaneSurface};

/// Node key: node type.
pub const NODE_KEY_TYPE: &str = "type";
/// Node type: an inner split node.
pub const NODE_TYPE_PANED: &str = "paned";
/// Node type: a leaf pane.
pub const NODE_TYPE_PANE: &str = "pane";
/// Node key: split orientation (`horizontal`/`vertical`).
pub const NODE_KEY_ORIENTATION: &str = "orientation";
/// Node key: split position as a `0..=1` ratio of the paned size.
pub const NODE_KEY_POSITION_RATIO: &str = "position_ratio";
/// Node key: first (start) child of a split.
pub const NODE_KEY_CHILD1: &str = "child1";
/// Node key: second (end) child of a split.
pub const NODE_KEY_CHILD2: &str = "child2";
/// Node key: the app payload of a leaf pane.
pub const NODE_KEY_PAYLOAD: &str = "payload";

const ORIENTATION_HORIZONTAL: &str = "horizontal";
const ORIENTATION_VERTICAL: &str = "vertical";

/// Serialize the tree's live paned structure. Panes for which `pane_payload`
/// returns `None` are dropped (their split collapses); returns `None` when no
/// pane was captured at all.
pub fn serialize_split_tree<P: SplitPaneSurface + 'static>(
    tree: &BigSplitTree<P>,
    pane_payload: &dyn Fn(&Rc<P>) -> Option<Value>,
) -> Option<Value> {
    let panes = tree.panes();
    let root = tree.root_widget().first_child()?;
    serialize_widget(&root, &panes, pane_payload)
}

fn serialize_widget<P: SplitPaneSurface + 'static>(
    widget: &Widget,
    panes: &[Rc<P>],
    pane_payload: &dyn Fn(&Rc<P>) -> Option<Value>,
) -> Option<Value> {
    if let Some(paned) = widget.downcast_ref::<Paned>() {
        return serialize_paned(paned, panes, pane_payload);
    }
    let pane = panes.iter().find(|pane| pane.pane_root() == *widget)?;
    let payload = pane_payload(pane)?;
    Some(json!({
        NODE_KEY_TYPE: NODE_TYPE_PANE,
        NODE_KEY_PAYLOAD: payload,
    }))
}

fn serialize_paned<P: SplitPaneSurface + 'static>(
    paned: &Paned,
    panes: &[Rc<P>],
    pane_payload: &dyn Fn(&Rc<P>) -> Option<Value>,
) -> Option<Value> {
    let child1 = paned
        .start_child()
        .and_then(|widget| serialize_widget(&widget, panes, pane_payload));
    let child2 = paned
        .end_child()
        .and_then(|widget| serialize_widget(&widget, panes, pane_payload));
    let (child1, child2) = match (child1, child2) {
        (Some(child1), Some(child2)) => (child1, child2),
        (Some(only), None) | (None, Some(only)) => return Some(only),
        (None, None) => return None,
    };

    let total = if paned.orientation() == Orientation::Horizontal {
        paned.width()
    } else {
        paned.height()
    };
    let ratio = if total > 0 {
        f64::from(paned.position()) / f64::from(total)
    } else {
        0.5_f64
    };
    Some(json!({
        NODE_KEY_TYPE: NODE_TYPE_PANED,
        NODE_KEY_ORIENTATION: if paned.orientation() == Orientation::Horizontal {
            ORIENTATION_HORIZONTAL
        } else {
            ORIENTATION_VERTICAL
        },
        NODE_KEY_POSITION_RATIO: ratio,
        NODE_KEY_CHILD1: child1,
        NODE_KEY_CHILD2: child2,
    }))
}

/// The pre-order first leaf payload of `node` — what the app builds the tab's
/// ROOT pane from before calling [`restore_split_tree`].
#[must_use]
pub fn first_pane_payload(node: &Value) -> Option<&Value> {
    let fields = node.as_object()?;
    match fields.get(NODE_KEY_TYPE).and_then(Value::as_str)? {
        NODE_TYPE_PANE => fields.get(NODE_KEY_PAYLOAD),
        NODE_TYPE_PANED => fields
            .get(NODE_KEY_CHILD1)
            .and_then(first_pane_payload)
            .or_else(|| fields.get(NODE_KEY_CHILD2).and_then(first_pane_payload)),
        _ => None,
    }
}

/// Rebuild the splits of `node` inside `tree`, whose only pane is `root_pane`
/// (already built from [`first_pane_payload`]). `build_pane` turns each
/// remaining leaf payload into a new pane; a leaf it declines is skipped with
/// its subtree.
pub fn restore_split_tree<P: SplitPaneSurface + 'static>(
    tree: &Rc<BigSplitTree<P>>,
    root_pane: &Rc<P>,
    node: &Value,
    build_pane: &dyn Fn(&Value) -> Option<Rc<P>>,
) {
    descend(tree, root_pane, node, build_pane);
}

fn descend<P: SplitPaneSurface + 'static>(
    tree: &Rc<BigSplitTree<P>>,
    target: &Rc<P>,
    node: &Value,
    build_pane: &dyn Fn(&Value) -> Option<Rc<P>>,
) {
    let Some(fields) = node.as_object() else {
        return;
    };
    if fields.get(NODE_KEY_TYPE).and_then(Value::as_str) != Some(NODE_TYPE_PANED) {
        return;
    }
    let orientation = match fields.get(NODE_KEY_ORIENTATION).and_then(Value::as_str) {
        Some(ORIENTATION_HORIZONTAL) => Orientation::Horizontal,
        _ => Orientation::Vertical,
    };
    let ratio = fields
        .get(NODE_KEY_POSITION_RATIO)
        .and_then(Value::as_f64)
        .unwrap_or(0.5);
    let (Some(child1), Some(child2)) = (fields.get(NODE_KEY_CHILD1), fields.get(NODE_KEY_CHILD2))
    else {
        return;
    };
    let Some(new_pane) = first_pane_payload(child2).and_then(build_pane) else {
        // child2 unbuildable → its subtree is dropped; child1 keeps the slot.
        descend(tree, target, child1, build_pane);
        return;
    };
    let Some(paned) = tree.split_at_pane(target, orientation, &new_pane) else {
        return;
    };
    apply_position_ratio_when_allocated(&paned, ratio);
    descend(tree, target, child1, build_pane);
    descend(tree, &new_pane, child2, build_pane);
}

/// Set the paned position to `ratio` of its size once it has one (allocation
/// happens after map; bounded retries via idle).
pub fn apply_position_ratio_when_allocated(paned: &Paned, ratio: f64) {
    let weak = paned.downgrade();
    let attempts = Rc::new(Cell::new(0_u8));
    glib::idle_add_local(move || {
        let Some(paned) = weak.upgrade() else {
            return ControlFlow::Break;
        };
        let total = if paned.orientation() == Orientation::Horizontal {
            paned.width()
        } else {
            paned.height()
        };
        if total > 0 {
            #[allow(clippy::cast_possible_truncation)]
            let position = (f64::from(total) * ratio).round() as i32;
            paned.set_position(position);
            return ControlFlow::Break;
        }
        let attempt = attempts.get().saturating_add(1);
        attempts.set(attempt);
        if attempt > 24 {
            ControlFlow::Break
        } else {
            ControlFlow::Continue
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_pane_payload_walks_child1_first() {
        let node = json!({
            "type": "paned",
            "orientation": "horizontal",
            "position_ratio": 0.4,
            "child1": {"type": "pane", "payload": {"path": "/a"}},
            "child2": {"type": "pane", "payload": {"path": "/b"}},
        });
        assert_eq!(
            first_pane_payload(&node),
            Some(&json!({"path": "/a"})),
            "pre-order first leaf"
        );
    }

    #[test]
    fn first_pane_payload_skips_to_child2_when_child1_missing() {
        let node = json!({
            "type": "paned",
            "child2": {"type": "pane", "payload": {"path": "/b"}},
        });
        assert_eq!(first_pane_payload(&node), Some(&json!({"path": "/b"})));
        assert_eq!(first_pane_payload(&json!({"type": "unknown"})), None);
    }
}
