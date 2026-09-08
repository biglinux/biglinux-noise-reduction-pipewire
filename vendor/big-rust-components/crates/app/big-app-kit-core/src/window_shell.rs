// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Display-free contracts for BigLinux application and workspace windows.
//!
//! `BigApplicationWindowShellSpec` is the base window contract: app identity,
//! lifecycle, persistence, transparency, colors, header/footer actions, toast
//! host, overlay root, and accessibility metadata.
//! `BigWorkspaceWindowShellSpec` composes that base shell for apps that
//! additionally need custom tabs, split panes, workspace focus, sidebar, dock,
//! and workspace overlays.

#[path = "window_shell_application.rs"]
mod window_shell_application;
#[path = "window_shell_base.rs"]
mod window_shell_base;
#[path = "window_shell_chrome.rs"]
mod window_shell_chrome;
#[path = "window_shell_workspace.rs"]
mod window_shell_workspace;
#[path = "window_shell_workspace_support.rs"]
mod window_shell_workspace_support;

pub use window_shell_application::*;
pub use window_shell_base::*;
pub use window_shell_chrome::*;
pub use window_shell_workspace::*;
pub use window_shell_workspace_support::*;

fn require_non_empty(
    value: &str,
    error: BigWindowShellSpecError,
) -> Result<(), BigWindowShellSpecError> {
    if value.trim().is_empty() {
        return Err(error);
    }
    Ok(())
}

fn validate_actions(actions: &[BigWindowActionSpec]) -> Result<(), BigWindowShellSpecError> {
    for action in actions {
        action.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_application_shell_does_not_enable_workspace_regions() {
        let resolved = BigApplicationWindowShellSpec::new("br.com.biglinux.Terminal", "Terminal")
            .transparent_root()
            .resolve()
            .expect("valid application shell");

        assert_eq!(
            resolved.transparency_policy,
            BigWindowTransparencyPolicy::TransparentRoot
        );
        assert_eq!(
            resolved.regions,
            vec![
                BigApplicationWindowRegion::Header,
                BigApplicationWindowRegion::Body,
                BigApplicationWindowRegion::Toast,
            ]
        );
    }

    #[test]
    fn application_shell_transparency_shortcuts_resolve_typed_policies() {
        let body_opacity = BigWindowOpacityPercent::new(88).expect("valid body opacity");
        let chrome_opacity = BigWindowOpacityPercent::new(76).expect("valid chrome opacity");

        let body_resolved = BigApplicationWindowShellSpec::new("br.com.biglinux.Body", "Body")
            .translucent_body(body_opacity)
            .resolve()
            .expect("valid body transparency");
        assert_eq!(
            body_resolved.transparency_policy,
            BigWindowTransparencyPolicy::TranslucentBody { body_opacity }
        );

        let chrome_resolved =
            BigApplicationWindowShellSpec::new("br.com.biglinux.Chrome", "Chrome")
                .translucent_chrome_and_body(chrome_opacity, body_opacity)
                .resolve()
                .expect("valid chrome transparency");
        assert_eq!(
            chrome_resolved.transparency_policy,
            BigWindowTransparencyPolicy::TranslucentChromeAndBody {
                chrome_opacity,
                body_opacity,
            }
        );
    }

    #[test]
    fn hosted_application_shell_requires_module_identity() {
        let error = BigApplicationWindowShellSpec::new("br.com.biglinux.Hosted", "Hosted")
            .hosted_module(" ")
            .resolve()
            .expect_err("empty hosted module id must fail");

        assert_eq!(error, BigWindowShellSpecError::EmptyHostedModuleId);
    }

    #[test]
    fn header_action_requires_accessible_name() {
        let error = BigApplicationWindowShellSpec::new("br.com.biglinux.Actions", "Actions")
            .header_action(BigWindowActionSpec::new(
                "win.new-tab",
                "New Tab",
                " ",
                BigWindowActionSurface::Header,
            ))
            .resolve()
            .expect_err("empty accessible name must fail");

        assert_eq!(error, BigWindowShellSpecError::EmptyActionAccessibleName);
    }

    #[test]
    fn workspace_shell_composes_application_shell() {
        let application_shell =
            BigApplicationWindowShellSpec::new("br.com.biglinux.BigTerminal", "BigTerminal")
                .default_size(1280, 720)
                .hosted_module("big-terminal");

        let resolved = BigWorkspaceWindowShellSpec::new(application_shell)
            .capability(BigWorkspaceCapability::Sidebar)
            .capability(BigWorkspaceCapability::Dock)
            .resolve()
            .expect("valid workspace shell");

        assert_eq!(resolved.application_shell.default_width, 1280);
        assert_eq!(
            resolved.application_shell.mount_mode,
            BigWindowMountMode::HostedModule {
                module_id: "big-terminal".to_owned(),
            }
        );
        assert_eq!(
            resolved.workspace_regions,
            vec![
                BigWorkspaceWindowRegion::WorkspaceTabs,
                BigWorkspaceWindowRegion::SplitPaneBody,
                BigWorkspaceWindowRegion::Sidebar,
                BigWorkspaceWindowRegion::Dock,
            ]
        );
    }

    #[test]
    fn workspace_shell_requires_tabs_and_split_panes() {
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Workspace", "Workspace");
        let error = BigWorkspaceWindowShellSpec::new(base.clone())
            .capabilities([])
            .resolve()
            .expect_err("workspace without tabs must fail first");

        assert_eq!(
            error,
            BigWindowShellSpecError::MissingWorkspaceTabsCapability
        );

        let error = BigWorkspaceWindowShellSpec::new(base)
            .capabilities([BigWorkspaceCapability::WorkspaceTabs])
            .resolve()
            .expect_err("workspace without split panes must fail");

        assert_eq!(error, BigWindowShellSpecError::MissingSplitPanesCapability);
    }

    #[test]
    fn workspace_overlay_requires_base_overlay_root() {
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Overlay", "Overlay");
        let error = BigWorkspaceWindowShellSpec::new(base)
            .capability(BigWorkspaceCapability::WorkspaceOverlay)
            .resolve()
            .expect_err("workspace overlay without base overlay root must fail");

        assert_eq!(
            error,
            BigWindowShellSpecError::WorkspaceOverlayRequiresBaseOverlay
        );
    }

    #[test]
    fn workspace_overlay_resolves_when_base_overlay_root_exists() {
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Overlay", "Overlay")
            .overlay_policy(BigWindowOverlayPolicy::SharedOverlayRoot);

        let resolved = BigWorkspaceWindowShellSpec::new(base)
            .capability(BigWorkspaceCapability::WorkspaceOverlay)
            .resolve()
            .expect("workspace overlay with base overlay root is valid");

        assert!(
            resolved
                .application_shell
                .regions
                .contains(&BigApplicationWindowRegion::Overlay)
        );
        assert!(
            resolved
                .workspace_regions
                .contains(&BigWorkspaceWindowRegion::WorkspaceOverlay)
        );
    }

    #[test]
    fn workspace_shell_resolves_toolbar_context_and_link_contracts() {
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Files", "File Manager");

        let resolved = BigWorkspaceWindowShellSpec::new(base)
            .toolbar_policy(BigWorkspaceToolbarPolicy::default())
            .contextual_surface_policy(
                BigWorkspaceContextualSurfacePolicy::new(BigWorkspacePaneRole::FileManager)
                    .companion_role(BigWorkspacePaneRole::Terminal)
                    .companion_role(BigWorkspacePaneRole::TextEditor),
            )
            .pane_link(BigWorkspacePaneLinkSpec::file_manager_with_terminal(
                "files-tab-1",
            ))
            .remote_file_access_policy(BigRemoteFileAccessPolicy::desktop_remote_uri_protocols())
            .resolve()
            .expect("valid configurable file-manager workspace");

        assert!(
            resolved
                .capabilities
                .contains(&BigWorkspaceCapability::ConfigurableToolbars)
        );
        assert!(
            resolved
                .capabilities
                .contains(&BigWorkspaceCapability::ContextualSurfaces)
        );
        assert!(
            resolved
                .capabilities
                .contains(&BigWorkspaceCapability::LinkedPanes)
        );
        assert!(
            resolved
                .capabilities
                .contains(&BigWorkspaceCapability::RemoteFileSessions)
        );
        assert_eq!(
            resolved
                .toolbar_policy
                .as_ref()
                .expect("toolbar policy")
                .default_placement,
            BigWorkspaceToolbarPlacement::Top
        );
        assert_eq!(
            resolved.pane_link_specs[0].directory_policy,
            BigLinkedDirectoryPolicy::FileManagerDrivesTerminal
        );
        let remote_policy = resolved
            .remote_file_access_policy
            .as_ref()
            .expect("remote file access policy");
        assert_eq!(
            remote_policy.preferred_backend,
            BigRemoteFileAccessBackend::Auto
        );
        assert!(remote_policy.requires_direct_uri_validation);
        assert!(
            remote_policy
                .supported_protocols
                .contains(&BigRemoteFileProtocol::Smb)
        );
        assert!(
            remote_policy
                .supported_protocols
                .contains(&BigRemoteFileProtocol::Ftp)
        );
        assert!(
            remote_policy
                .supported_protocols
                .contains(&BigRemoteFileProtocol::Ssh)
        );
        assert!(
            remote_policy
                .supported_protocols
                .contains(&BigRemoteFileProtocol::Sftp)
        );
    }

    #[test]
    fn product_workspace_presets_declare_companions_links_and_remote_sessions() {
        let terminal_resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.BigTerminal", "BigTerminal")
                .terminal_first_workspace("terminal-files")
                .resolve()
                .expect("valid terminal-first workspace");
        let terminal_context = terminal_resolved
            .contextual_surface_policy
            .as_ref()
            .expect("terminal context policy");
        assert_eq!(
            terminal_context.primary_role,
            BigWorkspacePaneRole::Terminal
        );
        assert!(
            terminal_context
                .companion_roles
                .contains(&BigWorkspacePaneRole::FileManager)
        );
        assert!(
            terminal_context
                .companion_roles
                .contains(&BigWorkspacePaneRole::AiAssistant)
        );
        assert_eq!(
            terminal_resolved.pane_link_specs[0].directory_policy,
            BigLinkedDirectoryPolicy::TerminalDrivesFileManager
        );
        assert!(terminal_resolved.remote_file_access_policy.is_none());

        let file_manager_resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.Files", "File Manager")
                .file_manager_first_workspace("files-terminal")
                .resolve()
                .expect("valid file-manager-first workspace");
        let file_manager_context = file_manager_resolved
            .contextual_surface_policy
            .as_ref()
            .expect("file manager context policy");
        assert_eq!(
            file_manager_context.primary_role,
            BigWorkspacePaneRole::FileManager
        );
        assert!(
            file_manager_context
                .companion_roles
                .contains(&BigWorkspacePaneRole::Terminal)
        );
        assert_eq!(
            file_manager_resolved.pane_link_specs[0].directory_policy,
            BigLinkedDirectoryPolicy::FileManagerDrivesTerminal
        );
        assert_desktop_remote_file_protocols(&file_manager_resolved);

        let editor_resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.Editor", "Text Editor")
                .text_editor_first_workspace("editor-files-terminal")
                .resolve()
                .expect("valid editor-first workspace");
        let editor_context = editor_resolved
            .contextual_surface_policy
            .as_ref()
            .expect("editor context policy");
        assert_eq!(
            editor_context.primary_role,
            BigWorkspacePaneRole::TextEditor
        );
        assert!(
            editor_context
                .companion_roles
                .contains(&BigWorkspacePaneRole::AiAssistant)
        );
        assert_eq!(
            editor_resolved.pane_link_specs[0].primary_role,
            BigWorkspacePaneRole::FileManager
        );
        assert_desktop_remote_file_protocols(&editor_resolved);
    }

    #[test]
    fn product_workspace_presets_remain_composable() {
        let resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.Files", "File Manager")
                .file_manager_first_workspace("files-terminal")
                .contextual_pane_roles(
                    BigWorkspacePaneRole::TextEditor,
                    [BigWorkspacePaneRole::FileManager],
                )
                .remote_file_access_policy(BigRemoteFileAccessPolicy::new([
                    BigRemoteFileProtocol::Sftp,
                ]))
                .resolve()
                .expect("valid customized file-manager workspace");

        assert_eq!(
            resolved
                .contextual_surface_policy
                .as_ref()
                .expect("custom context policy")
                .primary_role,
            BigWorkspacePaneRole::TextEditor
        );
        assert_eq!(
            resolved
                .remote_file_access_policy
                .as_ref()
                .expect("custom remote policy")
                .supported_protocols
                .len(),
            1
        );
    }

    #[test]
    fn standard_workspace_shortcuts_enable_sidebar_dock_overlay_with_a11y_names() {
        let side_opacity = BigWindowOpacityPercent::new(74).expect("valid side opacity");
        let body_opacity = BigWindowOpacityPercent::new(92).expect("valid body opacity");

        let resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.Files", "File Manager")
                .side_panel_region(
                    "File locations",
                    BigWorkspaceSidePanelChromePolicy::new(280)
                        .side_panel_opacity(side_opacity)
                        .body_opacity(body_opacity),
                )
                .dock_region("Linked terminal")
                .workspace_overlay_region()
                .resolve()
                .expect("valid standard workspace shell");

        assert!(
            resolved
                .capabilities
                .contains(&BigWorkspaceCapability::Sidebar)
        );
        assert!(
            resolved
                .capabilities
                .contains(&BigWorkspaceCapability::Dock)
        );
        assert!(
            resolved
                .capabilities
                .contains(&BigWorkspaceCapability::WorkspaceOverlay)
        );
        assert_eq!(
            resolved
                .workspace_accessibility_spec
                .sidebar_accessible_name
                .as_deref(),
            Some("File locations")
        );
        assert_eq!(
            resolved
                .workspace_accessibility_spec
                .dock_accessible_name
                .as_deref(),
            Some("Linked terminal")
        );
        assert!(
            resolved
                .application_shell
                .regions
                .contains(&BigApplicationWindowRegion::Overlay)
        );
        assert!(
            resolved
                .application_shell
                .color_policy
                .resolved_roles
                .contains(&BigWindowColorRole::Sidebar)
        );
        assert!(
            resolved
                .application_shell
                .color_policy
                .resolved_roles
                .contains(&BigWindowColorRole::Dock)
        );
        let side_panel_chrome_policy = resolved
            .side_panel_chrome_policy
            .expect("side panel chrome policy");
        assert_eq!(side_panel_chrome_policy.width_logical_pixels, 280);
        assert_eq!(side_panel_chrome_policy.header_width_logical_pixels, None);
        assert_eq!(side_panel_chrome_policy.side_panel_opacity, side_opacity);
        assert_eq!(side_panel_chrome_policy.body_opacity, body_opacity);
    }

    #[test]
    fn standard_workspace_shortcuts_preserve_composed_application_policy() {
        let resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.Editor", "Text Editor")
                .default_size(900, 650)
                .toast_policy(BigWindowToastPolicy::Disabled)
                .color_policy(BigWindowColorPolicy::default().with_role(BigWindowColorRole::Card))
                .application_accessible_name("Editor window")
                .primary_action(BigWindowActionSpec::new(
                    "win.new-tab",
                    "New Tab",
                    "Open a new tab",
                    BigWindowActionSurface::Header,
                ))
                .workspace_accessible_names("Editor workspace", "Document tabs", "Document panes")
                .workspace_action(BigWindowActionSpec::new(
                    "win.new-tab",
                    "New Tab",
                    "Open a new tab",
                    BigWindowActionSurface::Workspace,
                ))
                .resolve()
                .expect("valid standard workspace shell");

        assert_eq!(resolved.application_shell.default_width, 900);
        assert_eq!(resolved.application_shell.default_height, 650);
        assert_eq!(
            resolved.application_shell.regions,
            vec![
                BigApplicationWindowRegion::Header,
                BigApplicationWindowRegion::Body
            ]
        );
        assert!(
            resolved
                .application_shell
                .color_policy
                .resolved_roles
                .contains(&BigWindowColorRole::Card)
        );
        assert_eq!(
            resolved
                .application_shell
                .accessibility_spec
                .primary_actions[0]
                .action_id,
            "win.new-tab"
        );
        assert_eq!(
            resolved
                .workspace_accessibility_spec
                .workspace_accessible_name,
            "Editor workspace"
        );
        assert_eq!(
            resolved.workspace_accessibility_spec.workspace_actions[0].surface,
            BigWindowActionSurface::Workspace
        );
    }

    #[test]
    fn workspace_accessible_names_preserve_region_names_and_actions() {
        let resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.Files", "File Manager")
                .sidebar_region("Places")
                .dock_region("Linked terminal")
                .workspace_action(BigWindowActionSpec::new(
                    "win.reload",
                    "Reload",
                    "Reload current folder",
                    BigWindowActionSurface::Workspace,
                ))
                .workspace_accessible_names("Files workspace", "Folder tabs", "Folder panes")
                .resolve()
                .expect("valid workspace accessibility shortcut");

        assert_eq!(
            resolved
                .workspace_accessibility_spec
                .sidebar_accessible_name
                .as_deref(),
            Some("Places")
        );
        assert_eq!(
            resolved
                .workspace_accessibility_spec
                .dock_accessible_name
                .as_deref(),
            Some("Linked terminal")
        );
        assert_eq!(
            resolved.workspace_accessibility_spec.workspace_actions[0].action_id,
            "win.reload"
        );
    }

    #[test]
    fn standard_workspace_shortcuts_cover_full_application_shell_policy() {
        let chrome_opacity = BigWindowOpacityPercent::new(82).expect("valid chrome opacity");
        let body_opacity = BigWindowOpacityPercent::new(91).expect("valid body opacity");

        let resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.BigTerminal", "BigTerminal")
                .hosted_module("big-terminal")
                .close_policy(BigWindowClosePolicy::DeferToConsumerAction {
                    close_action_id: "win.close-request".to_owned(),
                })
                .translucent_chrome_and_body(chrome_opacity, body_opacity)
                .persistence_spec(
                    BigWindowStatePersistenceSpec::new("big-terminal-window")
                        .restore_fullscreen(false),
                )
                .workspace_overlay_region()
                .header_action(
                    BigWindowActionSpec::new(
                        "win.new-local-tab",
                        "New tab",
                        "Open a new local terminal tab",
                        BigWindowActionSurface::Header,
                    )
                    .accelerator("<Control><Shift>t"),
                )
                .footer_status_action(BigWindowActionSpec::new(
                    "win.toggle-status",
                    "Status",
                    "Toggle status line",
                    BigWindowActionSurface::FooterStatus,
                ))
                .resolve()
                .expect("valid full application shell policy");

        assert_eq!(
            resolved.application_shell.mount_mode,
            BigWindowMountMode::HostedModule {
                module_id: "big-terminal".to_owned()
            }
        );
        assert_eq!(
            resolved.application_shell.close_policy,
            BigWindowClosePolicy::DeferToConsumerAction {
                close_action_id: "win.close-request".to_owned()
            }
        );
        assert_eq!(
            resolved.application_shell.transparency_policy,
            BigWindowTransparencyPolicy::TranslucentChromeAndBody {
                chrome_opacity,
                body_opacity,
            }
        );
        assert_eq!(
            resolved
                .application_shell
                .persistence_spec
                .as_ref()
                .expect("persistence spec")
                .persistence_id,
            "big-terminal-window"
        );
        assert!(
            resolved
                .application_shell
                .regions
                .contains(&BigApplicationWindowRegion::Overlay)
        );
        assert!(
            resolved
                .application_shell
                .regions
                .contains(&BigApplicationWindowRegion::FooterStatus)
        );
        assert_eq!(
            resolved.application_shell.header_actions[0]
                .accelerator
                .as_deref(),
            Some("<Control><Shift>t")
        );
        assert_eq!(
            resolved.application_shell.footer_status_actions[0].action_id,
            "win.toggle-status"
        );
    }

    #[test]
    fn side_panel_chrome_shortcut_keeps_workspace_sidebar_opt_in() {
        let resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.Files", "File Manager")
                .side_panel_chrome(BigWorkspaceSidePanelChromePolicy::new(272))
                .resolve()
                .expect("valid side-panel chrome without sidebar region");

        assert!(
            !resolved
                .capabilities
                .contains(&BigWorkspaceCapability::Sidebar)
        );
        assert!(
            !resolved
                .workspace_regions
                .contains(&BigWorkspaceWindowRegion::Sidebar)
        );
        assert!(
            resolved
                .application_shell
                .color_policy
                .resolved_roles
                .contains(&BigWindowColorRole::Sidebar)
        );
        assert_eq!(
            resolved
                .side_panel_chrome_policy
                .expect("side-panel chrome policy")
                .width_logical_pixels,
            272
        );
    }

    #[test]
    fn side_panel_chrome_policy_preserves_width_and_transparency() {
        let side_opacity = BigWindowOpacityPercent::new(82).expect("valid side opacity");
        let body_opacity = BigWindowOpacityPercent::new(94).expect("valid body opacity");
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Files", "File Manager")
            .color_policy(
                BigWindowColorPolicy::default()
                    .with_role(BigWindowColorRole::Sidebar)
                    .with_role(BigWindowColorRole::Body),
            );

        let resolved = BigWorkspaceWindowShellSpec::new(base)
            .side_panel_chrome_policy(
                BigWorkspaceSidePanelChromePolicy::new(224)
                    .side_panel_opacity(side_opacity)
                    .body_opacity(body_opacity),
            )
            .resolve()
            .expect("valid side-panel chrome policy");

        let policy = resolved
            .side_panel_chrome_policy
            .expect("side-panel chrome policy");
        assert_eq!(policy.width_logical_pixels, 224);
        assert_eq!(policy.header_width_logical_pixels, None);
        assert_eq!(policy.side_panel_opacity, side_opacity);
        assert_eq!(policy.body_opacity, body_opacity);
        assert!(policy.should_extend_into_header);
    }

    #[test]
    fn side_panel_chrome_policy_preserves_header_width_override() {
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Files", "File Manager");

        let resolved = BigWorkspaceWindowShellSpec::new(base)
            .side_panel_chrome_policy(BigWorkspaceSidePanelChromePolicy::new(224).header_width(237))
            .resolve()
            .expect("valid side-panel chrome policy");

        let policy = resolved
            .side_panel_chrome_policy
            .expect("side-panel chrome policy");
        assert_eq!(policy.width_logical_pixels, 224);
        assert_eq!(policy.header_width_logical_pixels, Some(237));
    }

    #[test]
    fn side_panel_chrome_policy_requires_positive_width() {
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Files", "File Manager");

        let error = BigWorkspaceWindowShellSpec::new(base)
            .side_panel_chrome_policy(BigWorkspaceSidePanelChromePolicy::new(0))
            .resolve()
            .expect_err("zero side-panel width must fail");

        assert_eq!(error, BigWindowShellSpecError::InvalidSidePanelWidth);
    }

    #[test]
    fn toolbar_default_must_be_allowed() {
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Toolbar", "Toolbar Test");
        let error = BigWorkspaceWindowShellSpec::new(base)
            .toolbar_policy(
                BigWorkspaceToolbarPolicy::new(BigWorkspaceToolbarPlacement::Top)
                    .allowed_placements([BigWorkspaceToolbarPlacement::Bottom]),
            )
            .resolve()
            .expect_err("toolbar default outside allowed set must fail");

        assert_eq!(
            error,
            BigWindowShellSpecError::ToolbarDefaultPlacementNotAllowed
        );
    }

    #[test]
    fn linked_pane_roles_must_be_distinct() {
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Link", "Link Test");
        let error = BigWorkspaceWindowShellSpec::new(base)
            .pane_link(BigWorkspacePaneLinkSpec::new(
                "bad-link",
                BigWorkspacePaneRole::Terminal,
                BigWorkspacePaneRole::Terminal,
                BigWorkspacePaneLinkDirection::PrimaryDrivesSecondary,
                BigLinkedDirectoryPolicy::Bidirectional,
            ))
            .resolve()
            .expect_err("same role link must fail");

        assert_eq!(error, BigWindowShellSpecError::DuplicateLinkedPaneRoles);
    }

    #[test]
    fn terminal_to_file_manager_directory_policy_matches_remote_rule() {
        let policy = BigLinkedDirectoryPolicy::TerminalDrivesFileManager;

        assert!(
            policy
                .should_file_manager_follow_terminal_report(BigFileManagerBackendKind::Local, true)
        );
        assert!(
            !policy.should_file_manager_follow_terminal_report(
                BigFileManagerBackendKind::Local,
                false
            )
        );
        assert!(
            policy.should_file_manager_follow_terminal_report(
                BigFileManagerBackendKind::Remote,
                false
            )
        );
        assert!(!policy.should_terminal_follow_file_manager_navigation());
    }

    #[test]
    fn file_manager_to_terminal_policy_is_inverse() {
        let policy = BigLinkedDirectoryPolicy::FileManagerDrivesTerminal;

        assert!(
            !policy
                .should_file_manager_follow_terminal_report(BigFileManagerBackendKind::Local, true)
        );
        assert!(
            !policy.should_file_manager_follow_terminal_report(
                BigFileManagerBackendKind::Remote,
                false
            )
        );
        assert!(policy.should_terminal_follow_file_manager_navigation());
    }

    #[test]
    fn remote_file_access_policy_requires_protocols() {
        let base = BigApplicationWindowShellSpec::new("br.com.biglinux.Remote", "Remote Test");
        let error = BigWorkspaceWindowShellSpec::new(base)
            .remote_file_access_policy(BigRemoteFileAccessPolicy::new([]))
            .resolve()
            .expect_err("empty remote protocols must fail");

        assert_eq!(error, BigWindowShellSpecError::EmptyRemoteFileProtocolSet);
    }

    fn assert_desktop_remote_file_protocols(resolved: &BigWorkspaceWindowShellResolved) {
        let remote_file_access_policy = resolved
            .remote_file_access_policy
            .as_ref()
            .expect("desktop remote file access policy");

        assert!(
            remote_file_access_policy
                .supported_protocols
                .contains(&BigRemoteFileProtocol::Smb)
        );
        assert!(
            remote_file_access_policy
                .supported_protocols
                .contains(&BigRemoteFileProtocol::Ftp)
        );
        assert!(
            remote_file_access_policy
                .supported_protocols
                .contains(&BigRemoteFileProtocol::Ssh)
        );
        assert!(
            remote_file_access_policy
                .supported_protocols
                .contains(&BigRemoteFileProtocol::Sftp)
        );
        assert!(remote_file_access_policy.requires_direct_uri_validation);
        assert!(remote_file_access_policy.exposes_backend_preference_when_available);
    }
}
