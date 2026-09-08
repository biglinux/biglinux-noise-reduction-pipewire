// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Workspace shell spec and resolved contract.

use std::collections::BTreeSet;

use super::*;

/// Display-free workspace shell spec for an BigTerminal-grade BigLinux window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWorkspaceWindowShellSpec {
    /// Base application-window shell composed by this workspace shell.
    pub application_shell: BigApplicationWindowShellSpec,
    /// Required and optional workspace capabilities.
    pub capabilities: BTreeSet<BigWorkspaceCapability>,
    /// Workspace accessibility metadata.
    pub workspace_accessibility_spec: BigWorkspaceWindowAccessibilitySpec,
    /// Optional toolbar configuration policy.
    ///
    /// Legacy toolbar-only form of chrome placement. It folds into a
    /// [`BigChromeBand::Toolbar`] [`BigChromeBandPolicy`] via
    /// `BigChromeBandPolicy::from(&toolbar_policy)`. The general model is
    /// [`Self::chrome_placement`]; full unification (removing this field and
    /// re-pointing callers) lands with the GTK band-host adapter in a later
    /// wave, so existing `.toolbar_policy()` callers stay valid.
    pub toolbar_policy: Option<BigWorkspaceToolbarPolicy>,
    /// Optional declarative chrome-placement policy.
    ///
    /// The general chrome-as-data model: one [`BigChromeBandPolicy`] per movable
    /// band (headerbar, tab strip, toolbar, footer/status) plus the per-control
    /// [`BigChromePlacementPolicy::control_catalog`]. This supersedes the
    /// toolbar-only [`Self::toolbar_policy`]; both may be set during the
    /// migration.
    pub chrome_placement: Option<BigChromePlacementPolicy>,
    /// Optional visual policy for an app-owned side panel aligned with chrome.
    pub side_panel_chrome_policy: Option<BigWorkspaceSidePanelChromePolicy>,
    /// Optional context-sensitive surface policy.
    pub contextual_surface_policy: Option<BigWorkspaceContextualSurfacePolicy>,
    /// Per-tab or per-pane link contracts.
    pub pane_link_specs: Vec<BigWorkspacePaneLinkSpec>,
    /// Optional remote URI integration policy.
    pub remote_file_access_policy: Option<BigRemoteFileAccessPolicy>,
}

impl BigWorkspaceWindowShellSpec {
    /// Create a workspace shell spec with tabs and split panes enabled.
    #[must_use]
    pub fn new(application_shell: BigApplicationWindowShellSpec) -> Self {
        Self {
            application_shell,
            capabilities: [
                BigWorkspaceCapability::WorkspaceTabs,
                BigWorkspaceCapability::SplitPanes,
            ]
            .into_iter()
            .collect(),
            workspace_accessibility_spec: BigWorkspaceWindowAccessibilitySpec::default(),
            toolbar_policy: None,
            chrome_placement: None,
            side_panel_chrome_policy: None,
            contextual_surface_policy: None,
            pane_link_specs: Vec::new(),
            remote_file_access_policy: None,
        }
    }

    /// Create a standard workspace shell directly from application identity.
    ///
    /// This is the ergonomic entry point for common BigLinux apps. It keeps the
    /// same defaults as [`Self::new`] while avoiding repeated construction of
    /// the composed [`BigApplicationWindowShellSpec`].
    #[must_use]
    pub fn standard(application_id: impl Into<String>, window_title: impl Into<String>) -> Self {
        Self::new(BigApplicationWindowShellSpec::new(
            application_id,
            window_title,
        ))
    }

    /// Override the composed application-window default size.
    #[must_use]
    pub fn default_size(mut self, width: i32, height: i32) -> Self {
        self.application_shell = self.application_shell.default_size(width, height);
        self
    }

    /// Mark the composed application window as hosted by `big-host` or another
    /// module host.
    #[must_use]
    pub fn hosted_module(mut self, module_id: impl Into<String>) -> Self {
        self.application_shell = self.application_shell.hosted_module(module_id);
        self
    }

    /// Set the composed application-window close-request policy.
    #[must_use]
    pub fn close_policy(mut self, close_policy: BigWindowClosePolicy) -> Self {
        self.application_shell = self.application_shell.close_policy(close_policy);
        self
    }

    /// Set the composed application-window transparency policy.
    #[must_use]
    pub fn transparency_policy(mut self, transparency_policy: BigWindowTransparencyPolicy) -> Self {
        self.application_shell = self
            .application_shell
            .transparency_policy(transparency_policy);
        self
    }

    /// Make only the composed application-window body translucent.
    #[must_use]
    pub fn translucent_body(mut self, body_opacity: BigWindowOpacityPercent) -> Self {
        self.application_shell = self.application_shell.translucent_body(body_opacity);
        self
    }

    /// Make the composed application-window chrome and body translucent.
    #[must_use]
    pub fn translucent_chrome_and_body(
        mut self,
        chrome_opacity: BigWindowOpacityPercent,
        body_opacity: BigWindowOpacityPercent,
    ) -> Self {
        self.application_shell = self
            .application_shell
            .translucent_chrome_and_body(chrome_opacity, body_opacity);
        self
    }

    /// Request a transparent root for the composed application window.
    #[must_use]
    pub fn transparent_root(mut self) -> Self {
        self.application_shell = self.application_shell.transparent_root();
        self
    }

    /// Set the composed application-window toast host policy.
    #[must_use]
    pub fn toast_policy(mut self, toast_policy: BigWindowToastPolicy) -> Self {
        self.application_shell = self.application_shell.toast_policy(toast_policy);
        self
    }

    /// Set the composed application-window color policy.
    #[must_use]
    pub fn color_policy(mut self, color_policy: BigWindowColorPolicy) -> Self {
        self.application_shell = self.application_shell.color_policy(color_policy);
        self
    }

    /// Set the composed application-window persistence policy.
    #[must_use]
    pub fn persistence_spec(mut self, persistence_spec: BigWindowStatePersistenceSpec) -> Self {
        self.application_shell = self.application_shell.persistence_spec(persistence_spec);
        self
    }

    /// Set the composed application-window overlay-root policy.
    #[must_use]
    pub fn overlay_policy(mut self, overlay_policy: BigWindowOverlayPolicy) -> Self {
        self.application_shell = self.application_shell.overlay_policy(overlay_policy);
        self
    }

    /// Set the composed application-window accessibility metadata.
    #[must_use]
    pub fn application_accessibility_spec(
        mut self,
        accessibility_spec: BigApplicationWindowAccessibilitySpec,
    ) -> Self {
        self.application_shell = self
            .application_shell
            .accessibility_spec(accessibility_spec);
        self
    }

    /// Set only the composed application-window accessible name.
    #[must_use]
    pub fn application_accessible_name(mut self, accessible_name: impl Into<String>) -> Self {
        self.application_shell = self.application_shell.accessible_name(accessible_name);
        self
    }

    /// Add a primary action to the composed application accessibility contract.
    #[must_use]
    pub fn primary_action(mut self, action: BigWindowActionSpec) -> Self {
        self.application_shell = self.application_shell.primary_action(action);
        self
    }

    /// Add an action to the composed application-window header surface.
    #[must_use]
    pub fn header_action(mut self, action: BigWindowActionSpec) -> Self {
        self.application_shell = self.application_shell.header_action(action);
        self
    }

    /// Add an action to the composed application-window footer/status surface.
    #[must_use]
    pub fn footer_status_action(mut self, action: BigWindowActionSpec) -> Self {
        self.application_shell = self.application_shell.footer_status_action(action);
        self
    }

    /// Replace the capability set.
    #[must_use]
    pub fn capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = BigWorkspaceCapability>,
    ) -> Self {
        self.capabilities = capabilities.into_iter().collect();
        self
    }

    /// Add a workspace capability.
    #[must_use]
    pub fn capability(mut self, capability: BigWorkspaceCapability) -> Self {
        self.capabilities.insert(capability);
        self
    }

    /// Enable a sidebar region and set its accessible name.
    #[must_use]
    pub fn sidebar_region(mut self, accessible_name: impl Into<String>) -> Self {
        self.capabilities.insert(BigWorkspaceCapability::Sidebar);
        self.workspace_accessibility_spec.sidebar_accessible_name = Some(accessible_name.into());
        self
    }

    /// Enable a sidebar region with shared side-panel chrome.
    ///
    /// Use this for file-manager/editor style windows where the sidebar color
    /// should align with the header band and the panel width is user-configurable
    /// through the app's own settings adapter.
    #[must_use]
    pub fn side_panel_region(
        mut self,
        accessible_name: impl Into<String>,
        chrome_policy: BigWorkspaceSidePanelChromePolicy,
    ) -> Self {
        self = self.sidebar_region(accessible_name);
        self.side_panel_chrome(chrome_policy)
    }

    /// Apply shared side-panel chrome without enabling the workspace sidebar
    /// region.
    ///
    /// Use this when the app already owns the side panel inside its workspace
    /// body but still wants the shared header/body color and opacity policy.
    #[must_use]
    pub fn side_panel_chrome(mut self, chrome_policy: BigWorkspaceSidePanelChromePolicy) -> Self {
        self.side_panel_chrome_policy = Some(chrome_policy);
        self.application_shell.color_policy = self
            .application_shell
            .color_policy
            .clone()
            .with_role(BigWindowColorRole::Sidebar)
            .with_role(BigWindowColorRole::Body);
        self
    }

    /// Enable a dock region and set its accessible name.
    #[must_use]
    pub fn dock_region(mut self, accessible_name: impl Into<String>) -> Self {
        self.capabilities.insert(BigWorkspaceCapability::Dock);
        self.workspace_accessibility_spec.dock_accessible_name = Some(accessible_name.into());
        self.application_shell.color_policy = self
            .application_shell
            .color_policy
            .clone()
            .with_role(BigWindowColorRole::Dock);
        self
    }

    /// Enable a workspace overlay and the required base overlay root.
    #[must_use]
    pub fn workspace_overlay_region(mut self) -> Self {
        self.capabilities
            .insert(BigWorkspaceCapability::WorkspaceOverlay);
        self.application_shell.overlay_policy = BigWindowOverlayPolicy::SharedOverlayRoot;
        self
    }

    /// Set workspace accessibility metadata.
    #[must_use]
    pub fn workspace_accessibility_spec(
        mut self,
        workspace_accessibility_spec: BigWorkspaceWindowAccessibilitySpec,
    ) -> Self {
        self.workspace_accessibility_spec = workspace_accessibility_spec;
        self
    }

    /// Set the workspace, tab-list, and split-area accessible names.
    #[must_use]
    pub fn workspace_accessible_names(
        mut self,
        workspace_accessible_name: impl Into<String>,
        tab_list_accessible_name: impl Into<String>,
        split_area_accessible_name: impl Into<String>,
    ) -> Self {
        let sidebar_accessible_name = self
            .workspace_accessibility_spec
            .sidebar_accessible_name
            .take();
        let dock_accessible_name = self
            .workspace_accessibility_spec
            .dock_accessible_name
            .take();
        let workspace_actions =
            std::mem::take(&mut self.workspace_accessibility_spec.workspace_actions);
        self.workspace_accessibility_spec = BigWorkspaceWindowAccessibilitySpec::new(
            workspace_accessible_name,
            tab_list_accessible_name,
            split_area_accessible_name,
        );
        self.workspace_accessibility_spec.sidebar_accessible_name = sidebar_accessible_name;
        self.workspace_accessibility_spec.dock_accessible_name = dock_accessible_name;
        self.workspace_accessibility_spec.workspace_actions = workspace_actions;
        self
    }

    /// Add an action to the workspace accessibility contract.
    #[must_use]
    pub fn workspace_action(mut self, action: BigWindowActionSpec) -> Self {
        self.workspace_accessibility_spec =
            self.workspace_accessibility_spec.workspace_action(action);
        self
    }

    /// Add configurable workspace toolbar policy.
    #[must_use]
    pub fn toolbar_policy(mut self, toolbar_policy: BigWorkspaceToolbarPolicy) -> Self {
        self.capabilities
            .insert(BigWorkspaceCapability::ConfigurableToolbars);
        self.toolbar_policy = Some(toolbar_policy);
        self
    }

    /// Add the declarative chrome-placement policy.
    ///
    /// This is the general chrome-as-data model covering every movable band; the
    /// toolbar-only [`Self::toolbar_policy`] is the legacy form that folds into a
    /// [`BigChromeBand::Toolbar`] [`BigChromeBandPolicy`].
    #[must_use]
    pub fn chrome_placement(mut self, chrome_placement: BigChromePlacementPolicy) -> Self {
        self.chrome_placement = Some(chrome_placement);
        self
    }

    /// Add visual chrome policy for an app-owned side panel.
    #[must_use]
    pub fn side_panel_chrome_policy(
        mut self,
        side_panel_chrome_policy: BigWorkspaceSidePanelChromePolicy,
    ) -> Self {
        self.side_panel_chrome_policy = Some(side_panel_chrome_policy);
        self
    }

    /// Add context-sensitive workspace surface policy.
    #[must_use]
    pub fn contextual_surface_policy(
        mut self,
        contextual_surface_policy: BigWorkspaceContextualSurfacePolicy,
    ) -> Self {
        self.capabilities
            .insert(BigWorkspaceCapability::ContextualSurfaces);
        self.contextual_surface_policy = Some(contextual_surface_policy);
        self
    }

    /// Add a context-sensitive surface policy from semantic pane roles.
    #[must_use]
    pub fn contextual_pane_roles(
        mut self,
        primary_role: BigWorkspacePaneRole,
        companion_roles: impl IntoIterator<Item = BigWorkspacePaneRole>,
    ) -> Self {
        let mut contextual_surface_policy = BigWorkspaceContextualSurfacePolicy::new(primary_role);
        for companion_role in companion_roles {
            contextual_surface_policy = contextual_surface_policy.companion_role(companion_role);
        }
        self = self.contextual_surface_policy(contextual_surface_policy);
        self
    }

    /// Add a linked-pane contract.
    #[must_use]
    pub fn pane_link(mut self, pane_link_spec: BigWorkspacePaneLinkSpec) -> Self {
        self.capabilities
            .insert(BigWorkspaceCapability::LinkedPanes);
        self.pane_link_specs.push(pane_link_spec);
        self
    }

    /// Add remote URI integration requirements.
    #[must_use]
    pub fn remote_file_access_policy(
        mut self,
        remote_file_access_policy: BigRemoteFileAccessPolicy,
    ) -> Self {
        self.capabilities
            .insert(BigWorkspaceCapability::RemoteFileSessions);
        self.remote_file_access_policy = Some(remote_file_access_policy);
        self
    }

    /// Require the standard desktop remote file session protocols.
    #[must_use]
    pub fn desktop_remote_file_sessions(self) -> Self {
        self.remote_file_access_policy(BigRemoteFileAccessPolicy::desktop_remote_uri_protocols())
    }

    /// Configure a terminal-first workspace with file-manager, editor, and AI
    /// companion surfaces.
    ///
    /// The linked file-manager follows the active terminal directory. Use this
    /// for BigTerminal-style products, then keep app-specific terminal/session
    /// behavior in the consumer adapter.
    #[must_use]
    pub fn terminal_first_workspace(
        self,
        terminal_file_manager_link_id: impl Into<String>,
    ) -> Self {
        self.contextual_pane_roles(
            BigWorkspacePaneRole::Terminal,
            [
                BigWorkspacePaneRole::FileManager,
                BigWorkspacePaneRole::TextEditor,
                BigWorkspacePaneRole::AiAssistant,
            ],
        )
        .pane_link(BigWorkspacePaneLinkSpec::terminal_with_file_manager(
            terminal_file_manager_link_id,
        ))
    }

    /// Configure a file-manager-first workspace with linked terminal and editor
    /// companion surfaces.
    ///
    /// The linked terminal follows the active file-manager directory per tab or
    /// split group. Direct SMB, FTP, SSH, and SFTP URI support is required.
    #[must_use]
    pub fn file_manager_first_workspace(
        self,
        file_manager_terminal_link_id: impl Into<String>,
    ) -> Self {
        self.contextual_pane_roles(
            BigWorkspacePaneRole::FileManager,
            [
                BigWorkspacePaneRole::Terminal,
                BigWorkspacePaneRole::TextEditor,
            ],
        )
        .pane_link(BigWorkspacePaneLinkSpec::file_manager_with_terminal(
            file_manager_terminal_link_id,
        ))
        .desktop_remote_file_sessions()
    }

    /// Configure a text-editor-first workspace with file-manager, terminal, and
    /// AI companion surfaces.
    ///
    /// The companion file manager drives the companion terminal when both are
    /// shown, and direct SMB, FTP, SSH, and SFTP URI support is required.
    #[must_use]
    pub fn text_editor_first_workspace(
        self,
        file_manager_terminal_link_id: impl Into<String>,
    ) -> Self {
        self.contextual_pane_roles(
            BigWorkspacePaneRole::TextEditor,
            [
                BigWorkspacePaneRole::Terminal,
                BigWorkspacePaneRole::FileManager,
                BigWorkspacePaneRole::AiAssistant,
            ],
        )
        .pane_link(BigWorkspacePaneLinkSpec::file_manager_with_terminal(
            file_manager_terminal_link_id,
        ))
        .desktop_remote_file_sessions()
    }

    /// Resolve and validate the workspace shell.
    ///
    /// # Errors
    ///
    /// Returns [`BigWindowShellSpecError`] when the composed application shell is
    /// invalid, required workspace capabilities are missing, workspace overlay
    /// has no base overlay root, or accessibility metadata is incomplete.
    pub fn resolve(&self) -> Result<BigWorkspaceWindowShellResolved, BigWindowShellSpecError> {
        if !self
            .capabilities
            .contains(&BigWorkspaceCapability::WorkspaceTabs)
        {
            return Err(BigWindowShellSpecError::MissingWorkspaceTabsCapability);
        }
        if !self
            .capabilities
            .contains(&BigWorkspaceCapability::SplitPanes)
        {
            return Err(BigWindowShellSpecError::MissingSplitPanesCapability);
        }
        if self
            .capabilities
            .contains(&BigWorkspaceCapability::WorkspaceOverlay)
            && self.application_shell.overlay_policy != BigWindowOverlayPolicy::SharedOverlayRoot
        {
            return Err(BigWindowShellSpecError::WorkspaceOverlayRequiresBaseOverlay);
        }
        self.workspace_accessibility_spec.validate()?;
        if let Some(toolbar_policy) = &self.toolbar_policy {
            toolbar_policy.validate()?;
        }
        if let Some(chrome_placement) = &self.chrome_placement {
            chrome_placement.validate()?;
        }
        if let Some(side_panel_chrome_policy) = &self.side_panel_chrome_policy {
            side_panel_chrome_policy.validate()?;
        }
        for pane_link_spec in &self.pane_link_specs {
            pane_link_spec.validate()?;
        }
        if let Some(remote_file_access_policy) = &self.remote_file_access_policy {
            remote_file_access_policy.validate()?;
        }
        let application_shell = self.application_shell.resolve()?;
        Ok(BigWorkspaceWindowShellResolved {
            application_shell,
            workspace_regions: self.workspace_regions(),
            capabilities: self.capabilities.clone(),
            workspace_accessibility_spec: self.workspace_accessibility_spec.clone(),
            toolbar_policy: self.toolbar_policy.clone(),
            chrome_placement: self.chrome_placement.clone(),
            side_panel_chrome_policy: self.side_panel_chrome_policy,
            contextual_surface_policy: self.contextual_surface_policy.clone(),
            pane_link_specs: self.pane_link_specs.clone(),
            remote_file_access_policy: self.remote_file_access_policy.clone(),
        })
    }

    fn workspace_regions(&self) -> Vec<BigWorkspaceWindowRegion> {
        let mut regions = vec![
            BigWorkspaceWindowRegion::WorkspaceTabs,
            BigWorkspaceWindowRegion::SplitPaneBody,
        ];
        if self.capabilities.contains(&BigWorkspaceCapability::Sidebar) {
            regions.push(BigWorkspaceWindowRegion::Sidebar);
        }
        if self.capabilities.contains(&BigWorkspaceCapability::Dock) {
            regions.push(BigWorkspaceWindowRegion::Dock);
        }
        if self
            .capabilities
            .contains(&BigWorkspaceCapability::WorkspaceOverlay)
        {
            regions.push(BigWorkspaceWindowRegion::WorkspaceOverlay);
        }
        regions
    }
}

/// Resolved workspace shell contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWorkspaceWindowShellResolved {
    /// Resolved base application-window shell.
    pub application_shell: BigApplicationWindowShellResolved,
    /// Closed set of resolved workspace regions.
    pub workspace_regions: Vec<BigWorkspaceWindowRegion>,
    /// Required and optional workspace capabilities.
    pub capabilities: BTreeSet<BigWorkspaceCapability>,
    /// Workspace accessibility metadata.
    pub workspace_accessibility_spec: BigWorkspaceWindowAccessibilitySpec,
    /// Resolved toolbar configuration policy (legacy toolbar-only chrome form).
    pub toolbar_policy: Option<BigWorkspaceToolbarPolicy>,
    /// Resolved declarative chrome-placement policy (general chrome-as-data
    /// model).
    pub chrome_placement: Option<BigChromePlacementPolicy>,
    /// Resolved visual policy for an app-owned side panel aligned with chrome.
    pub side_panel_chrome_policy: Option<BigWorkspaceSidePanelChromePolicy>,
    /// Resolved context-sensitive surface policy.
    pub contextual_surface_policy: Option<BigWorkspaceContextualSurfacePolicy>,
    /// Resolved per-tab or per-pane link contracts.
    pub pane_link_specs: Vec<BigWorkspacePaneLinkSpec>,
    /// Resolved remote URI integration policy.
    pub remote_file_access_policy: Option<BigRemoteFileAccessPolicy>,
}
