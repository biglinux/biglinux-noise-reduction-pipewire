// SPDX-License-Identifier: MIT

//! Notification / activity badge spec.
//!
//! Tabs, sidebar rows, and toolbar buttons sometimes need a small
//! indicator: unread count, work-in-progress dot, error state.
//! [`BigNotificationBadge`] captures the data so widget builders and
//! AT-SPI smokes can reason about it uniformly.

/// Kind of badge to render: numeric count, dot, or pulse animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigBadgeKind {
    /// Numeric count (e.g. "3" unread).
    Count,
    /// Activity indicator (dot or pulse, no number).
    Dot,
    /// Pulsing state — animated until acknowledged.
    Pulse,
}

/// Colour tone applied to a [`BigNotificationBadge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigBadgeTone {
    /// Neutral / default surface accent.
    Neutral,
    /// Accent (matches app theme).
    Accent,
    /// Success (green).
    Success,
    /// Warning (yellow/orange).
    Warning,
    /// Error / destructive (red).
    Error,
}

/// Notification / activity badge widget. Count / Dot / Pulse variants with
/// reduced-motion compliance.
///
/// # Capabilities
///
/// `badge`, `badge+count`, `badge+activity`
///
/// # Archetypes
///
/// `terminal`, `editor`, `file-manager`, `pkg-manager`, `system-monitor`, `network-manager`, `container-manager`, `clipboard`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigNotificationBadge {
    /// Badge presentation mode (`Dot`, `Count`, `Activity`).
    pub kind: BigBadgeKind,
    /// Semantic color tone (`Neutral`, `Info`, `Warning`, `Danger`).
    pub tone: BigBadgeTone,
    /// Numeric count when `kind == Count`. Ignored for `Dot` / `Pulse`.
    pub count: u32,
    /// Cap for the visible count. Counts above this render as
    /// `"{cap}+"` (e.g. `"99+"`).
    pub display_cap: u32,
    /// Accessibility label override; defaults to a sensible auto-label.
    pub a11y_label: Option<String>,
    /// `true` to animate (`Pulse` always animates; `Dot`/`Count` are
    /// static when false).
    pub animate: bool,
}

impl Default for BigNotificationBadge {
    fn default() -> Self {
        Self {
            kind: BigBadgeKind::Dot,
            tone: BigBadgeTone::Accent,
            count: 0,
            display_cap: 99,
            a11y_label: None,
            animate: false,
        }
    }
}

impl BigNotificationBadge {
    /// Count badge.
    #[must_use]
    pub fn count(value: u32) -> Self {
        Self {
            kind: BigBadgeKind::Count,
            count: value,
            ..Self::default()
        }
    }

    /// Activity dot (no count).
    #[must_use]
    pub fn dot() -> Self {
        Self::default()
    }

    /// Pulse (animated) indicator.
    #[must_use]
    pub fn pulse() -> Self {
        Self {
            kind: BigBadgeKind::Pulse,
            animate: true,
            ..Self::default()
        }
    }

    /// Builder: sets tone.
    #[must_use]
    pub fn with_tone(mut self, tone: BigBadgeTone) -> Self {
        self.tone = tone;
        self
    }

    /// Disable animation (e.g. when reduced-motion is active).
    #[must_use]
    pub fn without_animation(mut self) -> Self {
        // Pulse loses its semantic meaning when not animated; downgrade to Dot.
        if self.kind == BigBadgeKind::Pulse {
            self.kind = BigBadgeKind::Dot;
        }
        self.animate = false;
        self
    }

    /// Visible label according to kind / count / cap.
    #[must_use]
    pub fn display_label(&self) -> String {
        match self.kind {
            BigBadgeKind::Count => {
                if self.count > self.display_cap {
                    format!("{}+", self.display_cap)
                } else {
                    self.count.to_string()
                }
            }
            BigBadgeKind::Dot | BigBadgeKind::Pulse => String::new(),
        }
    }

    /// Effective accessible label (auto-generated if none was set).
    #[must_use]
    pub fn effective_a11y_label(&self) -> String {
        if let Some(label) = &self.a11y_label {
            return label.clone();
        }
        match self.kind {
            BigBadgeKind::Count => format!("{} notifications", self.count),
            BigBadgeKind::Dot => "Activity".to_string(),
            BigBadgeKind::Pulse => "Attention".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_renders_number_under_cap() {
        let b = BigNotificationBadge::count(7);
        assert_eq!(b.display_label(), "7");
    }

    #[test]
    fn count_clamps_to_cap_with_plus_suffix() {
        let b = BigNotificationBadge::count(150);
        assert_eq!(b.display_label(), "99+");
    }

    #[test]
    fn dot_and_pulse_have_no_visible_label() {
        assert!(BigNotificationBadge::dot().display_label().is_empty());
        assert!(BigNotificationBadge::pulse().display_label().is_empty());
    }

    #[test]
    fn pulse_animates_by_default() {
        assert!(BigNotificationBadge::pulse().animate);
    }

    #[test]
    fn without_animation_downgrades_pulse_to_dot() {
        let b = BigNotificationBadge::pulse().without_animation();
        assert_eq!(b.kind, BigBadgeKind::Dot);
        assert!(!b.animate);
    }

    #[test]
    fn auto_a11y_label_describes_count() {
        let b = BigNotificationBadge::count(3);
        assert_eq!(b.effective_a11y_label(), "3 notifications");
    }

    #[test]
    fn tone_carries_through() {
        let b = BigNotificationBadge::count(1).with_tone(BigBadgeTone::Error);
        assert_eq!(b.tone, BigBadgeTone::Error);
    }
}
