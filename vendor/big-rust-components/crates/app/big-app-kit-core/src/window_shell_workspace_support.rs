// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Workspace support policies used by the workspace shell spec.

use std::collections::BTreeSet;

use super::{
    BigWindowActionSpec, BigWindowOpacityPercent, BigWindowShellSpecError, require_non_empty,
    validate_actions,
};

/// Workspace capability requested by a workspace shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigWorkspaceCapability {
    /// Custom workspace tab strip.
    WorkspaceTabs,
    /// Split-pane workspace body.
    SplitPanes,
    /// Sidebar region.
    Sidebar,
    /// Dock region.
    Dock,
    /// Workspace overlay region.
    WorkspaceOverlay,
    /// User-configurable toolbar placement, visibility, and ordering.
    ConfigurableToolbars,
    /// Per-tab/per-pane links between complementary workspace panes.
    LinkedPanes,
    /// Context-sensitive sidebar, dock, toolbar, and overlay surfaces.
    ContextualSurfaces,
    /// SSH/SFTP-backed file sessions owned by the file-manager workflow.
    RemoteFileSessions,
}

/// Edge where a workspace toolbar can be mounted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigWorkspaceToolbarPlacement {
    /// Toolbar above the workspace body.
    Top,
    /// Toolbar below the workspace body.
    Bottom,
    /// Toolbar to the leading/start side.
    Start,
    /// Toolbar to the trailing/end side.
    End,
}

/// Workspace toolbar configuration capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWorkspaceToolbarPolicy {
    /// Placements the user may choose.
    pub allowed_placements: BTreeSet<BigWorkspaceToolbarPlacement>,
    /// Initial placement used before persisted settings apply.
    pub default_placement: BigWorkspaceToolbarPlacement,
    /// Whether users may hide nonessential toolbar groups.
    pub supports_visibility_toggles: bool,
    /// Whether users may reorder toolbar groups.
    pub supports_reordering: bool,
}

impl BigWorkspaceToolbarPolicy {
    /// Create a toolbar policy with `default_placement` in the allowed set.
    #[must_use]
    pub fn new(default_placement: BigWorkspaceToolbarPlacement) -> Self {
        Self {
            allowed_placements: [default_placement].into_iter().collect(),
            default_placement,
            supports_visibility_toggles: true,
            supports_reordering: true,
        }
    }

    /// Replace allowed placements.
    #[must_use]
    pub fn allowed_placements(
        mut self,
        placements: impl IntoIterator<Item = BigWorkspaceToolbarPlacement>,
    ) -> Self {
        self.allowed_placements = placements.into_iter().collect();
        self
    }

    /// Disable or enable toolbar group visibility toggles.
    #[must_use]
    pub fn visibility_toggles(mut self, supports_visibility_toggles: bool) -> Self {
        self.supports_visibility_toggles = supports_visibility_toggles;
        self
    }

    /// Disable or enable toolbar group reordering.
    #[must_use]
    pub fn reordering(mut self, supports_reordering: bool) -> Self {
        self.supports_reordering = supports_reordering;
        self
    }

    pub(super) fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        if self.allowed_placements.is_empty() {
            return Err(BigWindowShellSpecError::EmptyToolbarPlacementSet);
        }
        if !self.allowed_placements.contains(&self.default_placement) {
            return Err(BigWindowShellSpecError::ToolbarDefaultPlacementNotAllowed);
        }
        Ok(())
    }
}

impl Default for BigWorkspaceToolbarPolicy {
    fn default() -> Self {
        Self::new(BigWorkspaceToolbarPlacement::Top).allowed_placements([
            BigWorkspaceToolbarPlacement::Top,
            BigWorkspaceToolbarPlacement::Bottom,
            BigWorkspaceToolbarPlacement::Start,
            BigWorkspaceToolbarPlacement::End,
        ])
    }
}

/// Visual policy for an app-owned side panel aligned with workspace chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigWorkspaceSidePanelChromePolicy {
    /// Side-panel width in logical pixels.
    pub width_logical_pixels: i32,
    /// Optional header band width when the rendered side panel includes chrome
    /// outside the app-owned content width, such as a scrolled-window gutter or
    /// border. Defaults to [`Self::width_logical_pixels`].
    pub header_width_logical_pixels: Option<i32>,
    /// Background opacity for the side-panel band.
    pub side_panel_opacity: BigWindowOpacityPercent,
    /// Background opacity for the main body band.
    pub body_opacity: BigWindowOpacityPercent,
    /// Whether the header should paint a side-panel-colored leading band.
    pub should_extend_into_header: bool,
}

impl BigWorkspaceSidePanelChromePolicy {
    /// Create a side-panel chrome policy with opaque side panel and body.
    #[must_use]
    pub const fn new(width_logical_pixels: i32) -> Self {
        Self {
            width_logical_pixels,
            header_width_logical_pixels: None,
            side_panel_opacity: BigWindowOpacityPercent::OPAQUE,
            body_opacity: BigWindowOpacityPercent::OPAQUE,
            should_extend_into_header: true,
        }
    }

    /// Set the side-panel colored header band width.
    #[must_use]
    pub const fn header_width(mut self, header_width_logical_pixels: i32) -> Self {
        self.header_width_logical_pixels = Some(header_width_logical_pixels);
        self
    }

    /// Set the side-panel background opacity.
    #[must_use]
    pub const fn side_panel_opacity(mut self, side_panel_opacity: BigWindowOpacityPercent) -> Self {
        self.side_panel_opacity = side_panel_opacity;
        self
    }

    /// Set the main body background opacity.
    #[must_use]
    pub const fn body_opacity(mut self, body_opacity: BigWindowOpacityPercent) -> Self {
        self.body_opacity = body_opacity;
        self
    }

    /// Set whether the side-panel color extends through the header.
    #[must_use]
    pub const fn extend_into_header(mut self, should_extend_into_header: bool) -> Self {
        self.should_extend_into_header = should_extend_into_header;
        self
    }

    pub(super) fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        if self.width_logical_pixels <= 0 {
            return Err(BigWindowShellSpecError::InvalidSidePanelWidth);
        }
        if let Some(header_width_logical_pixels) = self.header_width_logical_pixels
            && header_width_logical_pixels <= 0
        {
            return Err(BigWindowShellSpecError::InvalidSidePanelWidth);
        }
        Ok(())
    }
}

/// Semantic role of a workspace pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigWorkspacePaneRole {
    /// Terminal or shell pane.
    Terminal,
    /// File-manager pane.
    FileManager,
    /// Text editor pane.
    TextEditor,
    /// AI assistant pane or provider configuration surface.
    AiAssistant,
}

/// Which pane is the primary workflow owner for a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWorkspacePaneLinkDirection {
    /// Primary pane pushes state to the secondary pane.
    PrimaryDrivesSecondary,
    /// Both panes may push the shared state, with app-level de-duplication.
    Bidirectional,
}

/// Directory report emitted by a terminal or file-manager pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWorkspaceDirectoryReport {
    /// Hostname from OSC 7, a remote session, or an empty string when unknown.
    pub hostname: String,
    /// Absolute path in that pane backend.
    pub path: String,
}

impl BigWorkspaceDirectoryReport {
    /// Create a directory report.
    #[must_use]
    pub fn new(hostname: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            hostname: hostname.into(),
            path: path.into(),
        }
    }
}

/// Backend currently shown by a linked file-manager pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigFileManagerBackendKind {
    /// Local filesystem backend.
    Local,
    /// SSH/SFTP-backed remote backend.
    Remote,
}

/// Remote URI protocol exposed by file-oriented workspace apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigRemoteFileProtocol {
    /// SMB/CIFS shares, e.g. `smb://server/share`.
    Smb,
    /// FTP URLs, e.g. `ftp://server/path`.
    Ftp,
    /// SSH URLs, e.g. `ssh://server/path`.
    Ssh,
    /// SFTP URLs, e.g. `sftp://server/path`.
    Sftp,
}

/// Remote file integration backend selected by the app or user preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigRemoteFileAccessBackend {
    /// Prefer the best available desktop backend at runtime.
    #[default]
    Auto,
    /// Use GVfs/GIO URI integration.
    Gvfs,
    /// Use KIO URI integration.
    Kio,
}

/// Remote URI integration contract for file manager and editor surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigRemoteFileAccessPolicy {
    /// URI protocols the product must open directly.
    pub supported_protocols: BTreeSet<BigRemoteFileProtocol>,
    /// Preferred backend before persisted user settings apply.
    pub preferred_backend: BigRemoteFileAccessBackend,
    /// Whether preferences must expose backend choice when GVfs and KIO exist.
    pub exposes_backend_preference_when_available: bool,
    /// Whether validation must prove direct URI handling instead of only using
    /// the backend's FUSE mount path.
    pub requires_direct_uri_validation: bool,
}

impl BigRemoteFileAccessPolicy {
    /// Create a remote file access policy.
    #[must_use]
    pub fn new(protocols: impl IntoIterator<Item = BigRemoteFileProtocol>) -> Self {
        Self {
            supported_protocols: protocols.into_iter().collect(),
            preferred_backend: BigRemoteFileAccessBackend::Auto,
            exposes_backend_preference_when_available: true,
            requires_direct_uri_validation: true,
        }
    }

    /// Require SMB, FTP, SSH, and SFTP URI support.
    #[must_use]
    pub fn desktop_remote_uri_protocols() -> Self {
        Self::new([
            BigRemoteFileProtocol::Smb,
            BigRemoteFileProtocol::Ftp,
            BigRemoteFileProtocol::Ssh,
            BigRemoteFileProtocol::Sftp,
        ])
    }

    /// Set the preferred integration backend.
    #[must_use]
    pub fn preferred_backend(mut self, preferred_backend: BigRemoteFileAccessBackend) -> Self {
        self.preferred_backend = preferred_backend;
        self
    }

    pub(super) fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        if self.supported_protocols.is_empty() {
            return Err(BigWindowShellSpecError::EmptyRemoteFileProtocolSet);
        }
        Ok(())
    }
}

/// Current-directory synchronization contract for linked panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigLinkedDirectoryPolicy {
    /// BigTerminal-style link: terminal current directory drives the linked file
    /// manager. Local file managers follow only local-host reports; remote file
    /// managers follow their bound remote terminal regardless of reported host.
    TerminalDrivesFileManager,
    /// FileManager-first link: file-manager navigation drives a terminal bound
    /// to the same workspace tab/pane group.
    FileManagerDrivesTerminal,
    /// Both panes may update the current directory. Consumers must suppress
    /// echo loops by tracking the last applied absolute path per link.
    Bidirectional,
}

impl BigLinkedDirectoryPolicy {
    /// Whether a file-manager pane should consume a terminal directory report.
    ///
    /// `is_report_host_local` is supplied by the app because only the app knows
    /// its local hostname aliases. Remote file-manager backends intentionally do
    /// not compare the report hostname with the SSH target: remote shells often
    /// report a generic host name that can equal the local machine name.
    #[must_use]
    pub const fn should_file_manager_follow_terminal_report(
        self,
        backend_kind: BigFileManagerBackendKind,
        is_report_host_local: bool,
    ) -> bool {
        match self {
            Self::TerminalDrivesFileManager | Self::Bidirectional => match backend_kind {
                BigFileManagerBackendKind::Local => is_report_host_local,
                BigFileManagerBackendKind::Remote => true,
            },
            Self::FileManagerDrivesTerminal => false,
        }
    }

    /// Whether a terminal pane should consume a file-manager directory change.
    #[must_use]
    pub const fn should_terminal_follow_file_manager_navigation(self) -> bool {
        matches!(self, Self::FileManagerDrivesTerminal | Self::Bidirectional)
    }
}

/// One per-tab or per-pane link between complementary workspace panes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWorkspacePaneLinkSpec {
    /// Stable id local to the owning tab or split group.
    pub link_id: String,
    /// Primary workflow pane role.
    pub primary_role: BigWorkspacePaneRole,
    /// Secondary pane role linked to the primary workflow.
    pub secondary_role: BigWorkspacePaneRole,
    /// Direction for state updates.
    pub direction: BigWorkspacePaneLinkDirection,
    /// Directory synchronization rule.
    pub directory_policy: BigLinkedDirectoryPolicy,
}

impl BigWorkspacePaneLinkSpec {
    /// Create a linked-pane spec.
    #[must_use]
    pub fn new(
        link_id: impl Into<String>,
        primary_role: BigWorkspacePaneRole,
        secondary_role: BigWorkspacePaneRole,
        direction: BigWorkspacePaneLinkDirection,
        directory_policy: BigLinkedDirectoryPolicy,
    ) -> Self {
        Self {
            link_id: link_id.into(),
            primary_role,
            secondary_role,
            direction,
            directory_policy,
        }
    }

    /// Standard file-manager-first link: file-manager navigation drives a
    /// terminal bound to the same workspace tab or split group.
    #[must_use]
    pub fn file_manager_with_terminal(link_id: impl Into<String>) -> Self {
        Self::new(
            link_id,
            BigWorkspacePaneRole::FileManager,
            BigWorkspacePaneRole::Terminal,
            BigWorkspacePaneLinkDirection::PrimaryDrivesSecondary,
            BigLinkedDirectoryPolicy::FileManagerDrivesTerminal,
        )
    }

    /// Standard terminal-first link: terminal cwd drives a linked file manager.
    #[must_use]
    pub fn terminal_with_file_manager(link_id: impl Into<String>) -> Self {
        Self::new(
            link_id,
            BigWorkspacePaneRole::Terminal,
            BigWorkspacePaneRole::FileManager,
            BigWorkspacePaneLinkDirection::PrimaryDrivesSecondary,
            BigLinkedDirectoryPolicy::TerminalDrivesFileManager,
        )
    }

    pub(super) fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        require_non_empty(&self.link_id, BigWindowShellSpecError::EmptyPaneLinkId)?;
        if self.primary_role == self.secondary_role {
            return Err(BigWindowShellSpecError::DuplicateLinkedPaneRoles);
        }
        Ok(())
    }
}

/// Policy for context-sensitive shell surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWorkspaceContextualSurfacePolicy {
    /// Pane role whose controls should be shown by default.
    pub primary_role: BigWorkspacePaneRole,
    /// Extra pane roles that may appear as sidebars, docks, overlays, or splits.
    pub companion_roles: BTreeSet<BigWorkspacePaneRole>,
    /// Whether the shell should hide irrelevant commands when a companion pane
    /// is shown.
    pub hides_irrelevant_commands: bool,
}

impl BigWorkspaceContextualSurfacePolicy {
    /// Create a contextual-surface policy.
    #[must_use]
    pub fn new(primary_role: BigWorkspacePaneRole) -> Self {
        Self {
            primary_role,
            companion_roles: BTreeSet::new(),
            hides_irrelevant_commands: true,
        }
    }

    /// Add a companion role.
    #[must_use]
    pub fn companion_role(mut self, companion_role: BigWorkspacePaneRole) -> Self {
        self.companion_roles.insert(companion_role);
        self
    }

    /// Set whether irrelevant commands should be hidden for the current context.
    #[must_use]
    pub fn hide_irrelevant_commands(mut self, hides_irrelevant_commands: bool) -> Self {
        self.hides_irrelevant_commands = hides_irrelevant_commands;
        self
    }
}

/// Closed set of workspace-specific regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigWorkspaceWindowRegion {
    /// Custom workspace tab strip.
    WorkspaceTabs,
    /// Split-pane workspace body.
    SplitPaneBody,
    /// Sidebar region.
    Sidebar,
    /// Dock region.
    Dock,
    /// Workspace overlay region.
    WorkspaceOverlay,
}

/// Accessibility metadata for workspace-specific shell surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWorkspaceWindowAccessibilitySpec {
    /// Accessible name for the workspace body.
    pub workspace_accessible_name: String,
    /// Accessible name for the tab list.
    pub tab_list_accessible_name: String,
    /// Accessible name for the split-pane area.
    pub split_area_accessible_name: String,
    /// Optional accessible name for the sidebar region.
    pub sidebar_accessible_name: Option<String>,
    /// Optional accessible name for the dock region.
    pub dock_accessible_name: Option<String>,
    /// Workspace actions that must remain keyboard and assistive-tech reachable.
    pub workspace_actions: Vec<BigWindowActionSpec>,
}

impl BigWorkspaceWindowAccessibilitySpec {
    /// Create workspace accessibility metadata.
    #[must_use]
    pub fn new(
        workspace_accessible_name: impl Into<String>,
        tab_list_accessible_name: impl Into<String>,
        split_area_accessible_name: impl Into<String>,
    ) -> Self {
        Self {
            workspace_accessible_name: workspace_accessible_name.into(),
            tab_list_accessible_name: tab_list_accessible_name.into(),
            split_area_accessible_name: split_area_accessible_name.into(),
            sidebar_accessible_name: None,
            dock_accessible_name: None,
            workspace_actions: Vec::new(),
        }
    }

    /// Set sidebar accessible name.
    #[must_use]
    pub fn sidebar_accessible_name(mut self, accessible_name: impl Into<String>) -> Self {
        self.sidebar_accessible_name = Some(accessible_name.into());
        self
    }

    /// Set dock accessible name.
    #[must_use]
    pub fn dock_accessible_name(mut self, accessible_name: impl Into<String>) -> Self {
        self.dock_accessible_name = Some(accessible_name.into());
        self
    }

    /// Add a workspace action.
    #[must_use]
    pub fn workspace_action(mut self, action: BigWindowActionSpec) -> Self {
        self.workspace_actions.push(action);
        self
    }

    pub(super) fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        require_non_empty(
            &self.workspace_accessible_name,
            BigWindowShellSpecError::EmptyWorkspaceAccessibleName,
        )?;
        require_non_empty(
            &self.tab_list_accessible_name,
            BigWindowShellSpecError::EmptyTabListAccessibleName,
        )?;
        require_non_empty(
            &self.split_area_accessible_name,
            BigWindowShellSpecError::EmptySplitAreaAccessibleName,
        )?;
        if let Some(accessible_name) = &self.sidebar_accessible_name {
            require_non_empty(
                accessible_name,
                BigWindowShellSpecError::EmptyActionAccessibleName,
            )?;
        }
        if let Some(accessible_name) = &self.dock_accessible_name {
            require_non_empty(
                accessible_name,
                BigWindowShellSpecError::EmptyActionAccessibleName,
            )?;
        }
        validate_actions(&self.workspace_actions)
    }
}

impl Default for BigWorkspaceWindowAccessibilitySpec {
    fn default() -> Self {
        Self::new("Workspace", "Workspace tabs", "Split workspace")
    }
}
