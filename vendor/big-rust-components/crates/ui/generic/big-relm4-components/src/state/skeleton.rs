// SPDX-License-Identifier: MIT

//! Skeleton-screen placeholder spec.
//!
//! Used to fill list / grid surfaces while data hydrates. Respects
//! reduced-motion preferences: when the consuming layer detects it,
//! it should set [`BigSkeletonSpec::shimmer`] to `false` before
//! materialising the widget.

/// Visual shape of a single skeleton placeholder row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigSkeletonShape {
    /// Single horizontal bar (used for text lines).
    Bar,
    /// Card with title + body bars.
    Card,
    /// Avatar circle.
    Avatar,
    /// Avatar + two-line row.
    AvatarRow,
}

/// Skeleton placeholder spec (shape, row count, shimmer toggle).
///
/// # Examples
///
/// Build a list-row skeleton, honour reduced-motion, then cap the row count:
///
/// ```
/// use big_relm4_components::state::skeleton::{BigSkeletonShape, BigSkeletonSpec};
///
/// // Reduced-motion-friendly skeleton with 8 rows.
/// let spec = BigSkeletonSpec::list_rows()
///     .without_shimmer()
///     .with_rows(8);
///
/// assert_eq!(spec.shape, BigSkeletonShape::AvatarRow);
/// assert_eq!(spec.rows, 8);
/// assert!(!spec.shimmer);
/// ```
///
/// In a Relm4 component the spec is the typed `Init`:
///
/// ```ignore
/// impl relm4::SimpleComponent for LibrarySkeleton {
///     type Init = BigSkeletonSpec;
///     type Input = ();
///     type Output = ();
///     // model owns spec; view renders `spec.rows` placeholders of `spec.shape`.
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSkeletonSpec {
    /// Visual shape.
    pub shape: BigSkeletonShape,
    /// Number of placeholder rows / cards to render.
    pub rows: u8,
    /// Animate a shimmer/pulse. Disable when `prefers_reduced_motion`.
    pub shimmer: bool,
}

impl BigSkeletonSpec {
    /// 4-row list skeleton.
    #[must_use]
    pub fn list_rows() -> Self {
        Self {
            shape: BigSkeletonShape::AvatarRow,
            rows: 4,
            shimmer: true,
        }
    }

    /// 6-card grid skeleton.
    #[must_use]
    pub fn grid_cards() -> Self {
        Self {
            shape: BigSkeletonShape::Card,
            rows: 6,
            shimmer: true,
        }
    }

    /// Override the row count. Clamped to a sensible upper bound to
    /// avoid runaway widget trees.
    #[must_use]
    pub fn with_rows(mut self, rows: u8) -> Self {
        self.rows = rows.min(32);
        self
    }

    /// Disable shimmer (e.g. when reduced-motion is detected).
    #[must_use]
    pub fn without_shimmer(mut self) -> Self {
        self.shimmer = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_default_is_avatar_row_with_shimmer() {
        let s = BigSkeletonSpec::list_rows();
        assert_eq!(s.shape, BigSkeletonShape::AvatarRow);
        assert_eq!(s.rows, 4);
        assert!(s.shimmer);
    }

    #[test]
    fn grid_default_is_cards() {
        let s = BigSkeletonSpec::grid_cards();
        assert_eq!(s.shape, BigSkeletonShape::Card);
        assert_eq!(s.rows, 6);
    }

    #[test]
    fn with_rows_clamps_to_upper_bound() {
        let s = BigSkeletonSpec::list_rows().with_rows(200);
        assert_eq!(s.rows, 32);
    }

    #[test]
    fn without_shimmer_respects_reduced_motion() {
        let s = BigSkeletonSpec::list_rows().without_shimmer();
        assert!(!s.shimmer);
    }
}
