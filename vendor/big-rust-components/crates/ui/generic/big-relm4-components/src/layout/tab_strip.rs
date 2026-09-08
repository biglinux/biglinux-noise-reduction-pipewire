//! Shared tab-strip policies.
//!
//! Keep app-specific tab contents outside this crate. Share the stable
//! chrome contracts: spacing, close-button policy, and close visibility.

/// DEFAULT TAB BAR SPACING constant.
pub const DEFAULT_TAB_BAR_SPACING: i32 = 4;
/// DEFAULT TAB LABEL WIDTH CHARS constant.
pub const DEFAULT_TAB_LABEL_WIDTH_CHARS: i32 = 8;
/// DEFAULT TAB STRIP BOX SPACING constant.
pub const DEFAULT_TAB_STRIP_BOX_SPACING: i32 = 6;
/// DEFAULT ACTIVE ONLY CLOSE BALANCE WIDTH constant.
pub const DEFAULT_ACTIVE_ONLY_CLOSE_BALANCE_WIDTH: i32 = 26;

/// CSS class names + accessible labels a custom tab-strip HOST applies for
/// move/drop affordances and its context menu — the strip-level counterpart to
/// [`super::tab_button::BigTabButtonChrome`] (which themes one button).
///
/// [`Default`] uses canonical `big-tab-*` names; an app migrating a hand-rolled
/// strip overrides each field with its existing class names so the rendered
/// move/drop state stays byte-identical to its current stylesheet.
#[derive(Debug, Clone)]
pub struct BigTabStripChrome {
    /// Class on the tab being moved (drag source).
    pub moving: String,
    /// Class on the strip while a move is in flight.
    pub bar_move_mode: String,
    /// Class marking the current drop target.
    pub drop_target: String,
    /// Class for a drop-before-this-tab affordance.
    pub drop_left: String,
    /// Class for a drop-into-this-tab (split) affordance.
    pub drop_inside: String,
    /// Class for a drop-after-this-tab affordance.
    pub drop_right: String,
    /// Class toggled on the toplevel window while the tab context menu is open.
    pub context_menu_open_window: String,
    /// Accessible label for the tab context-menu popover.
    pub popover_accessible_label: String,
}

impl Default for BigTabStripChrome {
    fn default() -> Self {
        Self {
            moving: "big-tab-moving".to_owned(),
            bar_move_mode: "big-tab-bar-move-mode".to_owned(),
            drop_target: "big-tab-drop-target".to_owned(),
            drop_left: "big-tab-drop-left".to_owned(),
            drop_inside: "big-tab-drop-inside".to_owned(),
            drop_right: "big-tab-drop-right".to_owned(),
            context_menu_open_window: "big-tab-context-menu-open".to_owned(),
            popover_accessible_label: "Tab Actions".to_owned(),
        }
    }
}

/// Enumeration of supported big tab close mode variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigTabCloseMode {
    /// Close button always rendered on every tab.
    Always,
    /// Close button only revealed on hover.
    Hover,
    /// Close button only on the active tab.
    Active,
}

impl BigTabCloseMode {
    /// Builds a from setting value.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value {
            "hover" => Self::Hover,
            "active" => Self::Active,
            _ => Self::Always,
        }
    }

    /// Resolve the close-button render state for one tab under this mode.
    #[must_use]
    pub const fn button_state(
        self,
        is_active_tab: bool,
        is_hovered: bool,
        expand_evenly: bool,
    ) -> BigTabCloseButtonState {
        match self {
            Self::Always => BigTabCloseButtonState::visible(),
            Self::Hover => BigTabCloseButtonState::active_only(is_hovered),
            Self::Active => BigTabCloseButtonState::selected_only(is_active_tab, expand_evenly),
        }
    }

    /// Map this mode to the CSS state vector applied by the stylesheet.
    #[must_use]
    pub const fn css_state(self, is_active_tab: bool) -> BigTabCloseCssState {
        match self {
            Self::Always => BigTabCloseCssState {
                hover_mode: false,
                active_only: false,
                active_only_selected: false,
            },
            Self::Hover => BigTabCloseCssState {
                hover_mode: true,
                active_only: false,
                active_only_selected: false,
            },
            Self::Active => BigTabCloseCssState {
                hover_mode: false,
                active_only: true,
                active_only_selected: is_active_tab,
            },
        }
    }
}

/// Snapshot of big tab close css runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigTabCloseCssState {
    /// Hover mode.
    pub hover_mode: bool,
    /// Active only.
    pub active_only: bool,
    /// Active only selected.
    pub active_only_selected: bool,
}

/// Snapshot of big tab close button runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigTabCloseButtonState {
    /// True when the button still reserves a hit-box even while invisible,
    /// preserving column alignment across tabs.
    pub occupies_space: bool,
    /// True when the close button is currently rendered.
    pub visible: bool,
    /// True when the close button accepts clicks (gated by tab dirty state).
    pub sensitive: bool,
    /// True when an off-balance close button compensates for asymmetric tab chrome.
    pub balance_visible: bool,
}

impl BigTabCloseButtonState {
    /// Fully hidden close-button state: invisible and not occupying layout space.
    #[must_use]
    pub const fn hidden() -> Self {
        Self {
            occupies_space: false,
            visible: false,
            sensitive: false,
            balance_visible: false,
        }
    }

    /// Fully visible and sensitive close-button state.
    #[must_use]
    pub const fn visible() -> Self {
        Self {
            occupies_space: true,
            visible: true,
            sensitive: true,
            balance_visible: false,
        }
    }

    /// Hover-reveal state: reserves space but only shows when `revealed` is `true`.
    #[must_use]
    pub const fn active_only(revealed: bool) -> Self {
        Self {
            occupies_space: true,
            visible: revealed,
            sensitive: revealed,
            balance_visible: false,
        }
    }

    /// Selected-only state: visible solely on the active tab, with optional space balancing.
    #[must_use]
    pub const fn selected_only(active: bool, expand_evenly: bool) -> Self {
        Self {
            occupies_space: active,
            visible: active,
            sensitive: active,
            balance_visible: active && expand_evenly,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_close_mode_settings() {
        assert_eq!(
            BigTabCloseMode::from_setting("hover"),
            BigTabCloseMode::Hover
        );
        assert_eq!(
            BigTabCloseMode::from_setting("active"),
            BigTabCloseMode::Active
        );
        assert_eq!(BigTabCloseMode::from_setting(""), BigTabCloseMode::Always);
        assert_eq!(
            BigTabCloseMode::from_setting("unknown"),
            BigTabCloseMode::Always
        );
    }

    #[test]
    fn hover_mode_keeps_space_and_reveals_only_on_hover() {
        assert_eq!(
            BigTabCloseMode::Hover.button_state(false, false, true),
            BigTabCloseButtonState {
                occupies_space: true,
                visible: false,
                sensitive: false,
                balance_visible: false,
            }
        );
        assert!(
            BigTabCloseMode::Hover
                .button_state(false, true, true)
                .visible
        );
    }

    #[test]
    fn active_mode_balances_even_tabs() {
        assert_eq!(
            BigTabCloseMode::Active.button_state(true, false, true),
            BigTabCloseButtonState {
                occupies_space: true,
                visible: true,
                sensitive: true,
                balance_visible: true,
            }
        );
        assert_eq!(
            BigTabCloseMode::Active.button_state(false, true, true),
            BigTabCloseButtonState::hidden()
        );
    }
}
