// SPDX-License-Identifier: MIT

//! Presentation constants for the tab-strip click-to-move / drop interaction:
//! CSS class names, close-button opacity/sensitivity values, and log-message
//! templates. Pure data + string helpers, free of any widget code.

/// Which side of a tab a drop lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabDropSide {
    /// Drop before the tab.
    Left,
    /// Drop after the tab.
    Right,
}

// ---- Logger -------------------------------------------------------
/// Logger name for tab-move diagnostics.
pub const TAB_MOVE_LOGGER_NAME: &str = "big_terminal.tabs.move";

// ---- CSS classes --------------------------------------------------
/// CSS class on the tab being moved.
pub const CSS_CLASS_TAB_MOVING: &str = "tab-moving";
/// CSS class on the tab bar while in move mode.
pub const CSS_CLASS_TAB_BAR_MOVE_MODE: &str = "tab-bar-move-mode";
/// CSS class on a tab that is the current drop target.
pub const CSS_CLASS_TAB_DROP_TARGET: &str = "tab-drop-target";
/// CSS class highlighting a drop on the left side of a tab.
pub const CSS_CLASS_TAB_DROP_LEFT: &str = "tab-drop-left";
/// CSS class highlighting a drop inside (grouping into) a tab.
pub const CSS_CLASS_TAB_DROP_INSIDE: &str = "tab-drop-inside";
/// CSS class highlighting a drop on the right side of a tab.
pub const CSS_CLASS_TAB_DROP_RIGHT: &str = "tab-drop-right";
/// CSS class on a tab that belongs to a group.
pub const CSS_CLASS_IN_GROUP: &str = "in-group";
/// All drop-highlight CSS classes, for bulk removal between updates.
pub const TAB_DROP_HIGHLIGHT_CLASSES: &[&str] = &[
    CSS_CLASS_TAB_DROP_TARGET,
    CSS_CLASS_TAB_DROP_LEFT,
    CSS_CLASS_TAB_DROP_INSIDE,
    CSS_CLASS_TAB_DROP_RIGHT,
];
/// The drop-highlight CSS class for a given side.
#[must_use]
pub const fn drop_side_css_class(side: TabDropSide) -> &'static str {
    match side {
        TabDropSide::Left => CSS_CLASS_TAB_DROP_LEFT,
        TabDropSide::Right => CSS_CLASS_TAB_DROP_RIGHT,
    }
}

// ---- Close-button opacity -----------------------------------------
/// Close-button opacity when hidden.
pub const CLOSE_BUTTON_OPACITY_HIDDEN: f64 = 0.0;
/// Close-button opacity when visible.
pub const CLOSE_BUTTON_OPACITY_VISIBLE: f64 = 1.0;
/// Close-button sensitivity when hidden.
pub const CLOSE_BUTTON_SENSITIVE_HIDDEN: bool = false;
/// Close-button sensitivity when visible.
pub const CLOSE_BUTTON_SENSITIVE_VISIBLE: bool = true;

// ---- Log templates -------------------------------------------------
/// Log line emitted when a move is attempted on a single-tab strip.
pub const LOG_CANNOT_MOVE_SINGLE_TAB: &str = "Cannot move tab: only one tab exists.";
/// Template for the move-started log line (`{label}` placeholder).
pub const LOG_TAB_MOVE_STARTED_TEMPLATE: &str = "Tab move started for: {label}";
/// Log line emitted when a move is cancelled.
pub const LOG_TAB_MOVE_CANCELLED: &str = "Tab move cancelled.";
/// Template for the move-completed log line (`{label}`/`{old}`/`{new}`).
pub const LOG_TAB_MOVED_TEMPLATE: &str = "Tab '{label}' moved from {old} to {new}";
/// Render [`LOG_TAB_MOVE_STARTED_TEMPLATE`] with the tab label.
#[must_use]
pub fn format_tab_move_started_log(label: &str) -> String {
    LOG_TAB_MOVE_STARTED_TEMPLATE.replace("{label}", label)
}
/// Render [`LOG_TAB_MOVED_TEMPLATE`] with the tab label and old/new indices.
#[must_use]
pub fn format_tab_moved_log(label: &str, old_index: usize, new_index: usize) -> String {
    LOG_TAB_MOVED_TEMPLATE
        .replace("{label}", label)
        .replace("{old}", &old_index.to_string())
        .replace("{new}", &new_index.to_string())
}
