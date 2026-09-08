//! Shared split-pane layout policies.
//!
//! Apps own pane widgets. This module owns reusable layout intent.

/// Direction one split divider runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigSplitOrientation {
    /// Left/right split.
    Horizontal,
    /// Top/bottom split.
    Vertical,
}

/// Pre-defined multi-pane arrangement an app can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigSplitPreset {
    /// Three columns side by side.
    TripleColumns,
    /// Three rows stacked top-to-bottom.
    TripleRows,
    /// 2x2 grid: horizontal split, then vertical split on the new pane.
    Quad,
}

impl BigSplitPreset {
    /// Expand the preset into the ordered list of split orientations to apply.
    #[must_use]
    pub const fn orientations(self) -> &'static [BigSplitOrientation] {
        match self {
            Self::TripleColumns => &[
                BigSplitOrientation::Horizontal,
                BigSplitOrientation::Horizontal,
            ],
            Self::TripleRows => &[BigSplitOrientation::Vertical, BigSplitOrientation::Vertical],
            Self::Quad => &[
                BigSplitOrientation::Horizontal,
                BigSplitOrientation::Vertical,
            ],
        }
    }
}

/// Visibility rule for per-pane header chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigPaneHeaderPolicy {
    /// Show the pane header even on the only pane.
    Always,
    /// Never show a per-pane header (relies on the window header).
    Never,
    /// Show the per-pane header only once the split has more than
    /// one pane.
    WhenSplit,
}

impl BigPaneHeaderPolicy {
    /// Return `true` when the pane header should be shown for the given pane count.
    #[must_use]
    pub const fn is_visible(self, pane_count: usize) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::WhenSplit => pane_count > 1,
        }
    }
}

/// Which edge of a pane a drag-to-split drop lands against — the four edges a
/// pointer can be nearest to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigSplitPlacement {
    /// Split off a new pane on the left.
    Left,
    /// Split off a new pane on the right.
    Right,
    /// Split off a new pane on top.
    Top,
    /// Split off a new pane on the bottom.
    Bottom,
}

impl BigSplitPlacement {
    /// GTK orientation needed to create a split with this placement.
    #[must_use]
    pub fn gtk_orientation(self) -> gtk::Orientation {
        match self {
            Self::Left | Self::Right => gtk::Orientation::Horizontal,
            Self::Top | Self::Bottom => gtk::Orientation::Vertical,
        }
    }

    /// Whether the newly inserted pane/subtree must be placed before the
    /// target pane in the resulting `gtk::Paned`.
    #[must_use]
    pub const fn places_new_pane_before_target(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }

    /// Default placement for callers that only know the split orientation and
    /// want the new pane on the trailing side of the target.
    #[must_use]
    pub fn trailing_for_orientation(orientation: gtk::Orientation) -> Self {
        if orientation == gtk::Orientation::Vertical {
            Self::Bottom
        } else {
            Self::Right
        }
    }
}

/// Classify a pointer at `(x, y)` within a pane of `width`×`height` into the
/// nearest edge — the drag-to-split drop placement. Zero/negative dimensions
/// are clamped to 1 so the result is always well-defined.
#[must_use]
pub fn split_placement_for_pointer(x: f64, y: f64, width: i32, height: i32) -> BigSplitPlacement {
    let width = f64::from(width.max(1));
    let height = f64::from(height.max(1));
    let left = x.max(0.0);
    let right = (width - x).max(0.0);
    let top = y.max(0.0);
    let bottom = (height - y).max(0.0);

    let mut placement = BigSplitPlacement::Left;
    let mut distance = left;
    if right < distance {
        placement = BigSplitPlacement::Right;
        distance = right;
    }
    if top < distance {
        placement = BigSplitPlacement::Top;
        distance = top;
    }
    if bottom < distance {
        placement = BigSplitPlacement::Bottom;
    }
    placement
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_placement_uses_nearest_edge() {
        assert_eq!(
            split_placement_for_pointer(3.0, 50.0, 100, 100),
            BigSplitPlacement::Left
        );
        assert_eq!(
            split_placement_for_pointer(97.0, 50.0, 100, 100),
            BigSplitPlacement::Right
        );
        assert_eq!(
            split_placement_for_pointer(50.0, 2.0, 100, 100),
            BigSplitPlacement::Top
        );
        assert_eq!(
            split_placement_for_pointer(50.0, 98.0, 100, 100),
            BigSplitPlacement::Bottom
        );
    }

    #[test]
    fn split_placement_clamps_degenerate_size() {
        // Zero dimensions clamp to 1; result still well-defined.
        assert_eq!(
            split_placement_for_pointer(0.0, 0.0, 0, 0),
            BigSplitPlacement::Left
        );
    }

    #[test]
    fn split_placement_maps_to_gtk_split_parts() {
        assert_eq!(
            BigSplitPlacement::Left.gtk_orientation(),
            gtk::Orientation::Horizontal
        );
        assert!(BigSplitPlacement::Left.places_new_pane_before_target());
        assert_eq!(
            BigSplitPlacement::Right.gtk_orientation(),
            gtk::Orientation::Horizontal
        );
        assert!(!BigSplitPlacement::Right.places_new_pane_before_target());
        assert_eq!(
            BigSplitPlacement::Top.gtk_orientation(),
            gtk::Orientation::Vertical
        );
        assert!(BigSplitPlacement::Top.places_new_pane_before_target());
        assert_eq!(
            BigSplitPlacement::Bottom.gtk_orientation(),
            gtk::Orientation::Vertical
        );
        assert!(!BigSplitPlacement::Bottom.places_new_pane_before_target());
    }

    #[test]
    fn split_placement_trailing_side_matches_orientation() {
        assert_eq!(
            BigSplitPlacement::trailing_for_orientation(gtk::Orientation::Horizontal),
            BigSplitPlacement::Right
        );
        assert_eq!(
            BigSplitPlacement::trailing_for_orientation(gtk::Orientation::Vertical),
            BigSplitPlacement::Bottom
        );
    }

    #[test]
    fn split_presets_expand_to_expected_orientations() {
        assert_eq!(
            BigSplitPreset::TripleColumns.orientations(),
            &[
                BigSplitOrientation::Horizontal,
                BigSplitOrientation::Horizontal,
            ]
        );
        assert_eq!(
            BigSplitPreset::TripleRows.orientations(),
            &[BigSplitOrientation::Vertical, BigSplitOrientation::Vertical]
        );
        assert_eq!(
            BigSplitPreset::Quad.orientations(),
            &[
                BigSplitOrientation::Horizontal,
                BigSplitOrientation::Vertical
            ]
        );
    }

    #[test]
    fn pane_header_policy_hides_single_pane_chrome() {
        assert!(!BigPaneHeaderPolicy::WhenSplit.is_visible(1));
        assert!(BigPaneHeaderPolicy::WhenSplit.is_visible(2));
        assert!(BigPaneHeaderPolicy::Always.is_visible(1));
        assert!(!BigPaneHeaderPolicy::Never.is_visible(2));
    }
}
