// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Application-window shell contracts used by workspace shells.

use super::{
    BigWindowColorPolicy, BigWindowOpacityPercent, BigWindowShellSpecError,
    BigWindowTransparencyPolicy, require_non_empty, validate_actions,
};

/// How the shell is mounted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWindowMountMode {
    /// App owns a standalone top-level application window.
    #[default]
    StandaloneApplication,
    /// App is mounted as a module inside a host process.
    HostedModule {
        /// Stable module id used by the host registry.
        module_id: String,
    },
}

/// Window close behavior exposed to the GTK adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWindowClosePolicy {
    /// Close immediately when GTK emits close-request.
    #[default]
    CloseImmediately,
    /// Ask the consumer to confirm if more than one workspace tab is open.
    ConfirmWhenMultipleWorkspaceTabs {
        /// Action id or callback id that opens the confirmation surface.
        confirm_action_id: String,
    },
    /// Delegate every close request to an app-owned action.
    DeferToConsumerAction {
        /// Action id or callback id that classifies the close request.
        close_action_id: String,
    },
}

/// Toast host policy for a shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWindowToastPolicy {
    /// Do not create a toast host.
    Disabled,
    /// Wrap content in a toast host.
    #[default]
    Enabled,
}

/// Overlay-root policy for a shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWindowOverlayPolicy {
    /// No shared overlay root.
    #[default]
    None,
    /// Create one shared overlay root for floating shell surfaces.
    SharedOverlayRoot,
}

/// Closed set of base application-window regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigApplicationWindowRegion {
    /// Header or toolbar region.
    Header,
    /// Main application body region.
    Body,
    /// Shared overlay root.
    Overlay,
    /// Toast host.
    Toast,
    /// Footer/status region.
    FooterStatus,
}

/// Closed set of action surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigWindowActionSurface {
    /// Header action slot.
    Header,
    /// Workspace action surface.
    Workspace,
    /// Sidebar action surface.
    Sidebar,
    /// Dock action surface.
    Dock,
    /// Overlay action surface.
    Overlay,
    /// Footer/status action surface.
    FooterStatus,
}

/// One command exposed by the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWindowActionSpec {
    /// Stable action id, normally matching the app's `GAction` id.
    pub action_id: String,
    /// Visible label or menu label.
    pub label: String,
    /// Accessible name for icon-only controls or composite controls.
    pub accessible_name: String,
    /// Optional keyboard accelerator.
    pub accelerator: Option<String>,
    /// Surface where this action is expected to appear.
    pub surface: BigWindowActionSurface,
}

impl BigWindowActionSpec {
    /// Create a shell action spec.
    #[must_use]
    pub fn new(
        action_id: impl Into<String>,
        label: impl Into<String>,
        accessible_name: impl Into<String>,
        surface: BigWindowActionSurface,
    ) -> Self {
        Self {
            action_id: action_id.into(),
            label: label.into(),
            accessible_name: accessible_name.into(),
            accelerator: None,
            surface,
        }
    }

    /// Attach a keyboard accelerator.
    #[must_use]
    pub fn accelerator(mut self, accelerator: impl Into<String>) -> Self {
        self.accelerator = Some(accelerator.into());
        self
    }

    pub(super) fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        require_non_empty(&self.action_id, BigWindowShellSpecError::EmptyActionId)?;
        require_non_empty(&self.label, BigWindowShellSpecError::EmptyActionLabel)?;
        require_non_empty(
            &self.accessible_name,
            BigWindowShellSpecError::EmptyActionAccessibleName,
        )
    }
}

/// Geometry and durable window-state persistence policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWindowStatePersistenceSpec {
    /// Stable id used by the consumer settings adapter.
    pub persistence_id: String,
    /// Restore previous width and height.
    pub should_restore_geometry: bool,
    /// Restore maximized state.
    pub should_restore_maximized: bool,
    /// Restore fullscreen state.
    pub should_restore_fullscreen: bool,
}

impl BigWindowStatePersistenceSpec {
    /// Create a persistence spec with geometry and maximized-state restore.
    #[must_use]
    pub fn new(persistence_id: impl Into<String>) -> Self {
        Self {
            persistence_id: persistence_id.into(),
            should_restore_geometry: true,
            should_restore_maximized: true,
            should_restore_fullscreen: false,
        }
    }

    /// Include fullscreen restore.
    #[must_use]
    pub fn restore_fullscreen(mut self, should_restore_fullscreen: bool) -> Self {
        self.should_restore_fullscreen = should_restore_fullscreen;
        self
    }

    fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        require_non_empty(
            &self.persistence_id,
            BigWindowShellSpecError::EmptyPersistenceId,
        )
    }
}

/// Accessibility metadata for the base application-window shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigApplicationWindowAccessibilitySpec {
    /// Accessible name for the application window.
    pub window_accessible_name: String,
    /// Primary actions that must remain keyboard and assistive-tech reachable.
    pub primary_actions: Vec<BigWindowActionSpec>,
}

impl BigApplicationWindowAccessibilitySpec {
    /// Create window accessibility metadata.
    #[must_use]
    pub fn new(window_accessible_name: impl Into<String>) -> Self {
        Self {
            window_accessible_name: window_accessible_name.into(),
            primary_actions: Vec::new(),
        }
    }

    /// Add a primary action to the accessibility contract.
    #[must_use]
    pub fn primary_action(mut self, action: BigWindowActionSpec) -> Self {
        self.primary_actions.push(action);
        self
    }

    fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        require_non_empty(
            &self.window_accessible_name,
            BigWindowShellSpecError::EmptyWindowAccessibleName,
        )?;
        validate_actions(&self.primary_actions)
    }
}

/// Display-free base shell spec for a BigLinux GTK application window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigApplicationWindowShellSpec {
    /// Reverse-DNS application id.
    pub application_id: String,
    /// Localized window title.
    pub window_title: String,
    /// Initial window width in logical pixels.
    pub default_width: i32,
    /// Initial window height in logical pixels.
    pub default_height: i32,
    /// Standalone or hosted mounting mode.
    pub mount_mode: BigWindowMountMode,
    /// Close-request policy.
    pub close_policy: BigWindowClosePolicy,
    /// Window transparency policy.
    pub transparency_policy: BigWindowTransparencyPolicy,
    /// Window color policy.
    pub color_policy: BigWindowColorPolicy,
    /// Optional durable state persistence.
    pub persistence_spec: Option<BigWindowStatePersistenceSpec>,
    /// Toast host policy.
    pub toast_policy: BigWindowToastPolicy,
    /// Overlay-root policy.
    pub overlay_policy: BigWindowOverlayPolicy,
    /// Accessibility metadata.
    pub accessibility_spec: BigApplicationWindowAccessibilitySpec,
    /// Header actions.
    pub header_actions: Vec<BigWindowActionSpec>,
    /// Footer/status actions.
    pub footer_status_actions: Vec<BigWindowActionSpec>,
}

impl BigApplicationWindowShellSpec {
    /// Create a base application-window shell spec.
    #[must_use]
    pub fn new(application_id: impl Into<String>, window_title: impl Into<String>) -> Self {
        let window_title = window_title.into();
        Self {
            application_id: application_id.into(),
            accessibility_spec: BigApplicationWindowAccessibilitySpec::new(window_title.clone()),
            window_title,
            default_width: 960,
            default_height: 640,
            mount_mode: BigWindowMountMode::StandaloneApplication,
            close_policy: BigWindowClosePolicy::CloseImmediately,
            transparency_policy: BigWindowTransparencyPolicy::Opaque,
            color_policy: BigWindowColorPolicy::default(),
            persistence_spec: None,
            toast_policy: BigWindowToastPolicy::Enabled,
            overlay_policy: BigWindowOverlayPolicy::None,
            header_actions: Vec::new(),
            footer_status_actions: Vec::new(),
        }
    }

    /// Override the initial window size.
    #[must_use]
    pub fn default_size(mut self, width: i32, height: i32) -> Self {
        self.default_width = width;
        self.default_height = height;
        self
    }

    /// Mark the window as hosted by `big-host` or another module host.
    #[must_use]
    pub fn hosted_module(mut self, module_id: impl Into<String>) -> Self {
        self.mount_mode = BigWindowMountMode::HostedModule {
            module_id: module_id.into(),
        };
        self
    }

    /// Set close-request policy.
    #[must_use]
    pub fn close_policy(mut self, close_policy: BigWindowClosePolicy) -> Self {
        self.close_policy = close_policy;
        self
    }

    /// Set transparency policy.
    #[must_use]
    pub fn transparency_policy(mut self, transparency_policy: BigWindowTransparencyPolicy) -> Self {
        self.transparency_policy = transparency_policy;
        self
    }

    /// Make only the main body translucent.
    #[must_use]
    pub fn translucent_body(mut self, body_opacity: BigWindowOpacityPercent) -> Self {
        self.transparency_policy = BigWindowTransparencyPolicy::TranslucentBody { body_opacity };
        self
    }

    /// Make chrome and main body translucent with separate opacity values.
    #[must_use]
    pub fn translucent_chrome_and_body(
        mut self,
        chrome_opacity: BigWindowOpacityPercent,
        body_opacity: BigWindowOpacityPercent,
    ) -> Self {
        self.transparency_policy = BigWindowTransparencyPolicy::TranslucentChromeAndBody {
            chrome_opacity,
            body_opacity,
        };
        self
    }

    /// Request a transparent root window.
    #[must_use]
    pub fn transparent_root(mut self) -> Self {
        self.transparency_policy = BigWindowTransparencyPolicy::TransparentRoot;
        self
    }

    /// Set color policy.
    #[must_use]
    pub fn color_policy(mut self, color_policy: BigWindowColorPolicy) -> Self {
        self.color_policy = color_policy;
        self
    }

    /// Set durable state persistence policy.
    #[must_use]
    pub fn persistence_spec(mut self, persistence_spec: BigWindowStatePersistenceSpec) -> Self {
        self.persistence_spec = Some(persistence_spec);
        self
    }

    /// Set toast host policy.
    #[must_use]
    pub fn toast_policy(mut self, toast_policy: BigWindowToastPolicy) -> Self {
        self.toast_policy = toast_policy;
        self
    }

    /// Set overlay-root policy.
    #[must_use]
    pub fn overlay_policy(mut self, overlay_policy: BigWindowOverlayPolicy) -> Self {
        self.overlay_policy = overlay_policy;
        self
    }

    /// Set accessibility metadata.
    #[must_use]
    pub fn accessibility_spec(
        mut self,
        accessibility_spec: BigApplicationWindowAccessibilitySpec,
    ) -> Self {
        self.accessibility_spec = accessibility_spec;
        self
    }

    /// Set only the application-window accessible name.
    #[must_use]
    pub fn accessible_name(mut self, accessible_name: impl Into<String>) -> Self {
        self.accessibility_spec.window_accessible_name = accessible_name.into();
        self
    }

    /// Add a primary action to the application accessibility contract.
    #[must_use]
    pub fn primary_action(mut self, action: BigWindowActionSpec) -> Self {
        self.accessibility_spec = self.accessibility_spec.primary_action(action);
        self
    }

    /// Add an action to the header surface.
    #[must_use]
    pub fn header_action(mut self, action: BigWindowActionSpec) -> Self {
        self.header_actions.push(action);
        self
    }

    /// Add an action to the footer/status surface.
    #[must_use]
    pub fn footer_status_action(mut self, action: BigWindowActionSpec) -> Self {
        self.footer_status_actions.push(action);
        self
    }

    /// Resolve and validate the base application-window shell.
    ///
    /// # Errors
    ///
    /// Returns [`BigWindowShellSpecError`] when required names, action metadata,
    /// hosted identity, persistence identity, or dimensions are invalid.
    pub fn resolve(&self) -> Result<BigApplicationWindowShellResolved, BigWindowShellSpecError> {
        self.validate()?;
        let mut regions = vec![
            BigApplicationWindowRegion::Header,
            BigApplicationWindowRegion::Body,
        ];
        if self.overlay_policy == BigWindowOverlayPolicy::SharedOverlayRoot {
            regions.push(BigApplicationWindowRegion::Overlay);
        }
        if self.toast_policy == BigWindowToastPolicy::Enabled {
            regions.push(BigApplicationWindowRegion::Toast);
        }
        if !self.footer_status_actions.is_empty() {
            regions.push(BigApplicationWindowRegion::FooterStatus);
        }
        Ok(BigApplicationWindowShellResolved {
            application_id: self.application_id.clone(),
            window_title: self.window_title.clone(),
            default_width: self.default_width,
            default_height: self.default_height,
            mount_mode: self.mount_mode.clone(),
            close_policy: self.close_policy.clone(),
            transparency_policy: self.transparency_policy,
            color_policy: self.color_policy.clone(),
            persistence_spec: self.persistence_spec.clone(),
            regions,
            accessibility_spec: self.accessibility_spec.clone(),
            header_actions: self.header_actions.clone(),
            footer_status_actions: self.footer_status_actions.clone(),
        })
    }

    fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        require_non_empty(
            &self.application_id,
            BigWindowShellSpecError::EmptyApplicationId,
        )?;
        require_non_empty(
            &self.window_title,
            BigWindowShellSpecError::EmptyWindowTitle,
        )?;
        if self.default_width <= 0 || self.default_height <= 0 {
            return Err(BigWindowShellSpecError::InvalidDefaultSize);
        }
        if let BigWindowMountMode::HostedModule { module_id } = &self.mount_mode {
            require_non_empty(module_id, BigWindowShellSpecError::EmptyHostedModuleId)?;
        }
        match &self.close_policy {
            BigWindowClosePolicy::CloseImmediately => {}
            BigWindowClosePolicy::ConfirmWhenMultipleWorkspaceTabs { confirm_action_id } => {
                require_non_empty(confirm_action_id, BigWindowShellSpecError::EmptyActionId)?;
            }
            BigWindowClosePolicy::DeferToConsumerAction { close_action_id } => {
                require_non_empty(close_action_id, BigWindowShellSpecError::EmptyActionId)?;
            }
        }
        if let Some(persistence_spec) = &self.persistence_spec {
            persistence_spec.validate()?;
        }
        self.accessibility_spec.validate()?;
        validate_actions(&self.header_actions)?;
        validate_actions(&self.footer_status_actions)
    }
}

/// Resolved base application-window shell contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigApplicationWindowShellResolved {
    /// Reverse-DNS application id.
    pub application_id: String,
    /// Localized window title.
    pub window_title: String,
    /// Initial window width in logical pixels.
    pub default_width: i32,
    /// Initial window height in logical pixels.
    pub default_height: i32,
    /// Standalone or hosted mounting mode.
    pub mount_mode: BigWindowMountMode,
    /// Close-request policy.
    pub close_policy: BigWindowClosePolicy,
    /// Window transparency policy.
    pub transparency_policy: BigWindowTransparencyPolicy,
    /// Window color policy.
    pub color_policy: BigWindowColorPolicy,
    /// Optional durable state persistence.
    pub persistence_spec: Option<BigWindowStatePersistenceSpec>,
    /// Closed set of resolved base regions.
    pub regions: Vec<BigApplicationWindowRegion>,
    /// Accessibility metadata.
    pub accessibility_spec: BigApplicationWindowAccessibilitySpec,
    /// Header actions.
    pub header_actions: Vec<BigWindowActionSpec>,
    /// Footer/status actions.
    pub footer_status_actions: Vec<BigWindowActionSpec>,
}
