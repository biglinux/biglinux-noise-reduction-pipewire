// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Base error, transparency, and color contracts for window shells.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

/// Error returned when a shell spec cannot be resolved into a valid contract.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWindowShellSpecError {
    /// Application id is empty or whitespace-only.
    EmptyApplicationId,
    /// Window title is empty or whitespace-only.
    EmptyWindowTitle,
    /// Hosted module identity is empty or whitespace-only.
    EmptyHostedModuleId,
    /// Persistence identity is empty or whitespace-only.
    EmptyPersistenceId,
    /// An action id is empty or whitespace-only.
    EmptyActionId,
    /// An action label is empty or whitespace-only.
    EmptyActionLabel,
    /// An action accessible name is empty or whitespace-only.
    EmptyActionAccessibleName,
    /// Window accessible name is empty or whitespace-only.
    EmptyWindowAccessibleName,
    /// Workspace accessible name is empty or whitespace-only.
    EmptyWorkspaceAccessibleName,
    /// Workspace tab-list accessible name is empty or whitespace-only.
    EmptyTabListAccessibleName,
    /// Workspace split-area accessible name is empty or whitespace-only.
    EmptySplitAreaAccessibleName,
    /// Default window size must be positive.
    InvalidDefaultSize,
    /// Side-panel width must be positive.
    InvalidSidePanelWidth,
    /// Opacity percentage must stay in the inclusive range `0..=100`.
    InvalidOpacityPercent,
    /// Workspace shell was requested without required workspace tabs.
    MissingWorkspaceTabsCapability,
    /// Workspace shell was requested without required split panes.
    MissingSplitPanesCapability,
    /// Workspace overlay capability requires a base overlay root.
    WorkspaceOverlayRequiresBaseOverlay,
    /// Toolbar policy requires at least one allowed placement.
    EmptyToolbarPlacementSet,
    /// Default toolbar placement must be listed as allowed.
    ToolbarDefaultPlacementNotAllowed,
    /// Linked pane id is empty or whitespace-only.
    EmptyPaneLinkId,
    /// Linked pane primary and secondary roles must be different.
    DuplicateLinkedPaneRoles,
    /// Remote file policy requires at least one URI protocol.
    EmptyRemoteFileProtocolSet,
    /// Chrome band policy requires at least one allowed placement.
    EmptyBandPlacementSet,
    /// Default chrome band placement must be listed as allowed.
    BandDefaultPlacementNotAllowed,
    /// Chrome placement policy lists a band more than once.
    DuplicateChromeBand,
}

impl fmt::Display for BigWindowShellSpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyApplicationId => formatter.write_str("application id must not be empty"),
            Self::EmptyWindowTitle => formatter.write_str("window title must not be empty"),
            Self::EmptyHostedModuleId => formatter.write_str("hosted module id must not be empty"),
            Self::EmptyPersistenceId => formatter.write_str("persistence id must not be empty"),
            Self::EmptyActionId => formatter.write_str("action id must not be empty"),
            Self::EmptyActionLabel => formatter.write_str("action label must not be empty"),
            Self::EmptyActionAccessibleName => {
                formatter.write_str("action accessible name must not be empty")
            }
            Self::EmptyWindowAccessibleName => {
                formatter.write_str("window accessible name must not be empty")
            }
            Self::EmptyWorkspaceAccessibleName => {
                formatter.write_str("workspace accessible name must not be empty")
            }
            Self::EmptyTabListAccessibleName => {
                formatter.write_str("tab list accessible name must not be empty")
            }
            Self::EmptySplitAreaAccessibleName => {
                formatter.write_str("split area accessible name must not be empty")
            }
            Self::InvalidDefaultSize => {
                formatter.write_str("default width and height must be positive")
            }
            Self::InvalidSidePanelWidth => formatter.write_str("side-panel width must be positive"),
            Self::InvalidOpacityPercent => {
                formatter.write_str("opacity percentage must be between 0 and 100")
            }
            Self::MissingWorkspaceTabsCapability => {
                formatter.write_str("workspace shell requires workspace tabs")
            }
            Self::MissingSplitPanesCapability => {
                formatter.write_str("workspace shell requires split panes")
            }
            Self::WorkspaceOverlayRequiresBaseOverlay => {
                formatter.write_str("workspace overlay requires a base overlay root")
            }
            Self::EmptyToolbarPlacementSet => {
                formatter.write_str("toolbar policy requires at least one placement")
            }
            Self::ToolbarDefaultPlacementNotAllowed => {
                formatter.write_str("toolbar default placement must be allowed")
            }
            Self::EmptyPaneLinkId => formatter.write_str("linked pane id must not be empty"),
            Self::DuplicateLinkedPaneRoles => {
                formatter.write_str("linked pane primary and secondary roles must be different")
            }
            Self::EmptyRemoteFileProtocolSet => {
                formatter.write_str("remote file policy requires at least one URI protocol")
            }
            Self::EmptyBandPlacementSet => {
                formatter.write_str("chrome band policy requires at least one placement")
            }
            Self::BandDefaultPlacementNotAllowed => {
                formatter.write_str("chrome band default placement must be allowed")
            }
            Self::DuplicateChromeBand => {
                formatter.write_str("chrome placement policy lists a band more than once")
            }
        }
    }
}

impl Error for BigWindowShellSpecError {}

/// An opacity percentage for window transparency policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BigWindowOpacityPercent {
    value: u8,
}

impl BigWindowOpacityPercent {
    /// Fully transparent.
    pub const TRANSPARENT: Self = Self { value: 0 };
    /// Fully opaque.
    pub const OPAQUE: Self = Self { value: 100 };

    /// Create an opacity percentage.
    ///
    /// # Errors
    ///
    /// Returns [`BigWindowShellSpecError::InvalidOpacityPercent`] when `value`
    /// is above `100`.
    pub fn new(value: u8) -> Result<Self, BigWindowShellSpecError> {
        if value > 100 {
            return Err(BigWindowShellSpecError::InvalidOpacityPercent);
        }
        Ok(Self { value })
    }

    /// Return the numeric percentage.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.value
    }
}

/// How the window root should handle transparency.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWindowTransparencyPolicy {
    /// Paint a normal opaque app window.
    #[default]
    Opaque,
    /// Keep the chrome opaque but make the main body translucent.
    TranslucentBody {
        /// Body opacity percentage.
        body_opacity: BigWindowOpacityPercent,
    },
    /// Make both chrome and body translucent with separate opacity values.
    TranslucentChromeAndBody {
        /// Chrome/header/sidebar/dock opacity percentage.
        chrome_opacity: BigWindowOpacityPercent,
        /// Main body opacity percentage.
        body_opacity: BigWindowOpacityPercent,
    },
    /// Let the compositor show through the root. Dialogs/popovers must still
    /// resolve their own opaque role colors.
    TransparentRoot,
}

/// Color-scheme selection for a shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWindowColorScheme {
    /// Follow the desktop color-scheme preference.
    #[default]
    FollowSystem,
    /// Prefer a light scheme.
    PreferLight,
    /// Prefer a dark scheme.
    PreferDark,
    /// Use an app-owned palette id.
    CustomPalette {
        /// Palette id resolved by the app's settings adapter.
        palette_id: String,
    },
}

/// Semantic color roles a shell adapter must preserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigWindowColorRole {
    /// Window root background.
    Window,
    /// Main content/body background.
    Body,
    /// Header/chrome background.
    Header,
    /// Sidebar background.
    Sidebar,
    /// Dock background.
    Dock,
    /// Card or boxed-list background.
    Card,
    /// Dialog background.
    Dialog,
    /// Popover background.
    Popover,
}

/// Color policy for application and workspace shells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWindowColorPolicy {
    /// Desired color scheme.
    pub color_scheme: BigWindowColorScheme,
    /// Roles the GTK adapter must resolve separately so transparency does not
    /// leak into dialogs, popovers, cards, or side surfaces.
    pub resolved_roles: BTreeSet<BigWindowColorRole>,
    /// Whether high-contrast overrides must be kept available.
    pub supports_high_contrast: bool,
}

impl Default for BigWindowColorPolicy {
    fn default() -> Self {
        Self {
            color_scheme: BigWindowColorScheme::FollowSystem,
            resolved_roles: [
                BigWindowColorRole::Window,
                BigWindowColorRole::Body,
                BigWindowColorRole::Header,
                BigWindowColorRole::Dialog,
                BigWindowColorRole::Popover,
            ]
            .into_iter()
            .collect(),
            supports_high_contrast: true,
        }
    }
}

impl BigWindowColorPolicy {
    /// Use an app-owned custom palette id.
    #[must_use]
    pub fn custom_palette(palette_id: impl Into<String>) -> Self {
        Self {
            color_scheme: BigWindowColorScheme::CustomPalette {
                palette_id: palette_id.into(),
            },
            ..Self::default()
        }
    }

    /// Add a semantic color role that the adapter must resolve independently.
    #[must_use]
    pub fn with_role(mut self, role: BigWindowColorRole) -> Self {
        self.resolved_roles.insert(role);
        self
    }
}
