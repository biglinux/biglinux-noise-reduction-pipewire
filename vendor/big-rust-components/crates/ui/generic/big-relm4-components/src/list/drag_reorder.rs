// SPDX-License-Identifier: MIT

//! Drag-and-drop reorder spec for generic list entries.
//!
//! Captures the data side of reordering: list entries, a stable
//! identity per entry, the move operation, and the visual feedback
//! policy. The widget builder layer turns this into GTK drop targets
//! plus highlight CSS.

/// Visual feedback while dragging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigDragHighlight {
    /// No highlight — keep DnD silent.
    None,
    /// Underline / insertion bar between rows.
    InsertionBar,
    /// Highlight the source + target row.
    SourceAndTarget,
}

/// Movement outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigReorderMove {
    /// From.
    pub from: usize,
    /// To.
    pub to: usize,
}

impl BigReorderMove {
    /// Returns `true` if noop.
    #[must_use]
    pub fn is_noop(self) -> bool {
        self.from == self.to
    }
}

/// Reorder spec parameterised over the item id type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDragReorderSpec<Id> {
    /// Ordered item ids.
    pub items: Vec<Id>,
    /// Visual feedback policy.
    pub highlight: BigDragHighlight,
    /// `true` allows moves across distinct list instances (e.g.
    /// playlist → other playlist). Default is `false` (within-list
    /// only).
    pub cross_list: bool,
}

impl<Id: Clone + Eq> BigDragReorderSpec<Id> {
    /// Creates a new instance.
    #[must_use]
    pub fn new(items: Vec<Id>) -> Self {
        Self {
            items,
            highlight: BigDragHighlight::InsertionBar,
            cross_list: false,
        }
    }

    /// Builder: sets highlight.
    #[must_use]
    pub fn with_highlight(mut self, highlight: BigDragHighlight) -> Self {
        self.highlight = highlight;
        self
    }

    /// Configure the `allow_cross_list` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigDragReorderSpec`].
    #[must_use]
    pub fn allow_cross_list(mut self) -> Self {
        self.cross_list = true;
        self
    }

    /// Apply a move. Returns `Err` when `from`/`to` are out of range.
    /// `to` follows the "drop *before* this index" semantics.
    ///
    /// # Errors
    /// Returns a string describing the invalid index when out of range.
    pub fn apply(&mut self, mv: BigReorderMove) -> Result<(), String> {
        if mv.is_noop() {
            return Ok(());
        }
        if mv.from >= self.items.len() {
            return Err(format!("from {} out of range", mv.from));
        }
        if mv.to > self.items.len() {
            return Err(format!("to {} out of range", mv.to));
        }
        let moved_entry = self.items.remove(mv.from);
        let adjusted_to = if mv.to > mv.from { mv.to - 1 } else { mv.to };
        self.items
            .insert(adjusted_to.min(self.items.len()), moved_entry);
        Ok(())
    }

    /// Convenience: apply a move and return the resulting order.
    ///
    /// # Errors
    /// Returns a string describing the invalid index when out of range.
    pub fn after_move(&self, mv: BigReorderMove) -> Result<Vec<Id>, String> {
        let mut clone = self.clone();
        clone.apply(mv)?;
        Ok(clone.items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(items: &[&'static str]) -> BigDragReorderSpec<&'static str> {
        BigDragReorderSpec::new(items.to_vec())
    }

    #[test]
    fn noop_move_keeps_order() {
        let mut s = spec(&["a", "b", "c"]);
        s.apply(BigReorderMove { from: 1, to: 1 }).unwrap();
        assert_eq!(s.items, vec!["a", "b", "c"]);
    }

    #[test]
    fn move_forward_one_slot() {
        let mut s = spec(&["a", "b", "c", "d"]);
        s.apply(BigReorderMove { from: 0, to: 2 }).unwrap();
        // Drop before index 2 with original 0 removed = ["b", "a", "c", "d"]
        assert_eq!(s.items, vec!["b", "a", "c", "d"]);
    }

    #[test]
    fn move_backward_one_slot() {
        let mut s = spec(&["a", "b", "c", "d"]);
        s.apply(BigReorderMove { from: 3, to: 1 }).unwrap();
        assert_eq!(s.items, vec!["a", "d", "b", "c"]);
    }

    #[test]
    fn move_to_end_uses_len() {
        let mut s = spec(&["a", "b", "c"]);
        s.apply(BigReorderMove { from: 0, to: 3 }).unwrap();
        assert_eq!(s.items, vec!["b", "c", "a"]);
    }

    #[test]
    fn out_of_range_from_rejected() {
        let mut s = spec(&["a"]);
        let err = s.apply(BigReorderMove { from: 5, to: 0 }).unwrap_err();
        assert!(err.contains("from 5"));
    }

    #[test]
    fn cross_list_off_by_default() {
        let s = spec(&["a"]);
        assert!(!s.cross_list);
    }

    #[test]
    fn allow_cross_list_flips_flag() {
        let s = spec(&["a"]).allow_cross_list();
        assert!(s.cross_list);
    }

    #[test]
    fn after_move_does_not_mutate_original() {
        let s = spec(&["a", "b", "c"]);
        let new = s.after_move(BigReorderMove { from: 0, to: 2 }).unwrap();
        assert_eq!(s.items, vec!["a", "b", "c"]);
        assert_eq!(new, vec!["b", "a", "c"]);
    }
}
