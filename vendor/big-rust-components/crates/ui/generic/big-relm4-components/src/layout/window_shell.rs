// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared GTK shell for standard BigLinux application windows.
//!
//! The shell consumes display-free contracts from `big-app-kit` and builds the
//! reusable chrome once: toolbar root, header, body, optional overlay root,
//! optional toast host, footer action row, and accessible shell names. Apps keep
//! ownership of domain widgets and settings adapters.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::window_shell::{
    BigApplicationWindowRegion, BigApplicationWindowShellResolved, BigApplicationWindowShellSpec,
    BigBandPlacement, BigChromeBand, BigChromeBandPolicy, BigWindowActionSurface,
    BigWindowOpacityPercent, BigWindowShellSpecError, BigWindowTransparencyPolicy,
    BigWorkspaceSidePanelChromePolicy, BigWorkspaceWindowRegion, BigWorkspaceWindowShellResolved,
    BigWorkspaceWindowShellSpec,
};
use relm4::gtk;

use crate::i18n::{gettext_noop, t};
use crate::layout::chrome_band_host::{BigChromeBandHost, BigChromeBandPlacementCallback};
use crate::layout::tab_prefs::{self, BigTabStripPosition};
use crate::layout::tab_strip_placement::BigTabStripPlacement;
pub use crate::layout::tab_strip_placement::build_side_dock_resize_grip;
use crate::pane::{BigWorkspaceShell, SplitPaneSurface};

#[path = "window_shell/actions.rs"]
mod actions;
#[path = "window_shell/side_panel_chrome.rs"]
mod side_panel_chrome;
#[path = "window_shell/workspace_header.rs"]
mod workspace_header;

use actions::{create_footer_status_bar, create_shell_action_button};
use side_panel_chrome::{
    SidePanelChromeRuntime, apply_side_panel_chrome_policy, refresh_side_panel_chrome_policy,
};
use workspace_header::{
    create_workspace_tab_bar_host, create_workspace_title_stack,
    refresh_workspace_title_stack_policy,
};

/// Initialization payload for [`BigApplicationWindowShell`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigApplicationWindowShellInit {
    /// Display-free application shell spec.
    pub shell_spec: BigApplicationWindowShellSpec,
}

impl BigApplicationWindowShellInit {
    /// Create a shell initialization payload.
    #[must_use]
    pub fn new(shell_spec: BigApplicationWindowShellSpec) -> Self {
        Self { shell_spec }
    }
}

/// Inputs understood by a future Relm4 component wrapper around the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigApplicationWindowShellInput {
    /// Apply a new transparency policy to the existing shell widgets.
    ApplyTransparencyPolicy(BigWindowTransparencyPolicy),
    /// Ask the owning application to close according to its close policy.
    RequestClose,
    /// Move focus to the application body region.
    FocusBody,
}

/// Outputs emitted by shell action surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigApplicationWindowShellOutput {
    /// The shell close path was requested.
    CloseRequested,
    /// A header action identified by action id was activated.
    HeaderActionActivated(String),
    /// A footer/status action identified by action id was activated.
    FooterStatusActionActivated(String),
}

/// Shared visual shell for a standard BigLinux GTK application window.
#[derive(Clone)]
pub struct BigApplicationWindowShell {
    resolved_spec: BigApplicationWindowShellResolved,
    root: adw::ToolbarView,
    header: adw::HeaderBar,
    body: gtk::Box,
    footer_status_bar: Option<gtk::Box>,
    overlay: Option<gtk::Overlay>,
    toast_overlay: Option<adw::ToastOverlay>,
    transparency_css_class: Rc<RefCell<Option<String>>>,
    transparency_css: Rc<RefCell<String>>,
    transparency_css_provider: Rc<RefCell<Option<gtk::CssProvider>>>,
}

impl BigApplicationWindowShell {
    /// Build the shell from a display-free spec.
    ///
    /// # Errors
    ///
    /// Returns [`BigWindowShellSpecError`] when the provided spec is invalid.
    pub fn new(spec: BigApplicationWindowShellSpec) -> Result<Self, BigWindowShellSpecError> {
        let resolved_spec = spec.resolve()?;
        Ok(Self::from_resolved(resolved_spec))
    }

    /// Build the shell from an already resolved spec.
    #[must_use]
    pub fn from_resolved(resolved_spec: BigApplicationWindowShellResolved) -> Self {
        let root = adw::ToolbarView::new();
        root.add_css_class("big-application-window-shell");
        root.update_property(&[gtk::accessible::Property::Label(
            &resolved_spec.accessibility_spec.window_accessible_name,
        )]);

        let window_title = adw::WindowTitle::new(&resolved_spec.window_title, "");
        let header = adw::HeaderBar::builder()
            .title_widget(&window_title)
            .show_start_title_buttons(true)
            .show_end_title_buttons(true)
            .build();
        header.add_css_class("big-application-window-header");
        root.add_top_bar(&header);

        for action in resolved_spec
            .header_actions
            .iter()
            .filter(|action| action.surface == BigWindowActionSurface::Header)
        {
            header.pack_end(&create_shell_action_button(action));
        }

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["big-application-window-body"])
            .build();
        body.set_accessible_role(gtk::AccessibleRole::Group);
        body.update_property(&[gtk::accessible::Property::Label("Application content")]);

        let body_or_overlay = if has_region(&resolved_spec, BigApplicationWindowRegion::Overlay) {
            let overlay = gtk::Overlay::builder()
                .child(&body)
                .hexpand(true)
                .vexpand(true)
                .css_classes(["big-application-window-overlay-root"])
                .build();
            overlay.set_accessible_role(gtk::AccessibleRole::Group);
            Some((overlay.clone().upcast::<gtk::Widget>(), overlay))
        } else {
            None
        };

        let content = body_or_overlay
            .as_ref()
            .map(|(content, _)| content.clone())
            .unwrap_or_else(|| body.clone().upcast::<gtk::Widget>());

        let (content, toast_overlay) =
            if has_region(&resolved_spec, BigApplicationWindowRegion::Toast) {
                let toast_overlay = adw::ToastOverlay::new();
                toast_overlay.set_child(Some(&content));
                toast_overlay.add_css_class("big-application-window-toast-host");
                (
                    toast_overlay.clone().upcast::<gtk::Widget>(),
                    Some(toast_overlay),
                )
            } else {
                (content, None)
            };

        root.set_content(Some(&content));

        let footer_status_bar = create_footer_status_bar(&resolved_spec.footer_status_actions);
        if let Some(footer_status_bar) = &footer_status_bar {
            root.add_bottom_bar(footer_status_bar);
        }

        let transparency_css = Rc::new(RefCell::new(String::new()));
        let transparency_css_provider = Rc::new(RefCell::new(None));
        root.connect_realize({
            let transparency_css = transparency_css.clone();
            let transparency_css_provider = transparency_css_provider.clone();
            move |root| {
                install_transparency_policy_css(
                    &root.display(),
                    &transparency_css.borrow(),
                    &transparency_css_provider,
                );
            }
        });

        let shell = Self {
            resolved_spec,
            root,
            header,
            body,
            footer_status_bar,
            overlay: body_or_overlay.map(|(_, overlay)| overlay),
            toast_overlay,
            transparency_css_class: Rc::new(RefCell::new(None)),
            transparency_css,
            transparency_css_provider,
        };
        shell.apply_transparency_policy(shell.resolved_spec.transparency_policy);
        shell
    }

    /// Borrow the resolved display-free contract used to build the shell.
    #[must_use]
    pub fn resolved_spec(&self) -> &BigApplicationWindowShellResolved {
        &self.resolved_spec
    }

    /// Borrow the toolbar-view root.
    #[must_use]
    pub fn root(&self) -> &adw::ToolbarView {
        &self.root
    }

    /// Consume the shell and return the toolbar-view root.
    #[must_use]
    pub fn into_root(self) -> adw::ToolbarView {
        self.root
    }

    /// Borrow the standard header bar.
    #[must_use]
    pub fn header(&self) -> &adw::HeaderBar {
        &self.header
    }

    /// Append app-owned content to the start side of the standard header.
    pub fn append_header_start_child(&self, child: &impl IsA<gtk::Widget>) {
        self.header.pack_start(child);
    }

    /// Append app-owned content to the end side of the standard header.
    pub fn append_header_end_child(&self, child: &impl IsA<gtk::Widget>) {
        self.header.pack_end(child);
    }

    /// Apply this shell's window-level defaults and mount its root content in
    /// an `adw::Window`.
    ///
    /// Apps still own the concrete window type and application registration, but
    /// common title, default size, and content mounting stay in the shared shell.
    pub fn mount_into_adw_window(&self, window: &adw::Window) {
        window.set_title(Some(&self.resolved_spec.window_title));
        window.set_default_size(
            self.resolved_spec.default_width,
            self.resolved_spec.default_height,
        );
        window.set_content(Some(&self.root));
    }

    /// Apply this shell's window-level defaults and mount its root content in
    /// an `adw::ApplicationWindow`.
    pub fn mount_into_adw_application_window(&self, window: &adw::ApplicationWindow) {
        window.set_title(Some(&self.resolved_spec.window_title));
        window.set_default_size(
            self.resolved_spec.default_width,
            self.resolved_spec.default_height,
        );
        window.set_content(Some(&self.root));
    }

    /// Borrow the body container where app-owned content is mounted.
    #[must_use]
    pub fn body(&self) -> &gtk::Box {
        &self.body
    }

    /// Append app-owned body content.
    pub fn append_body_child(&self, child: &impl IsA<gtk::Widget>) {
        self.body.append(child);
    }

    /// Borrow the optional footer/status action row.
    #[must_use]
    pub fn footer_status_bar(&self) -> Option<&gtk::Box> {
        self.footer_status_bar.as_ref()
    }

    /// Borrow the optional shared overlay root.
    #[must_use]
    pub fn overlay(&self) -> Option<&gtk::Overlay> {
        self.overlay.as_ref()
    }

    /// Borrow the optional toast overlay.
    #[must_use]
    pub fn toast_overlay(&self) -> Option<&adw::ToastOverlay> {
        self.toast_overlay.as_ref()
    }

    /// Apply a new transparency policy to the existing shell widgets.
    pub fn apply_transparency_policy(&self, policy: BigWindowTransparencyPolicy) {
        let css_class = transparency_policy_css_class(policy);
        if let Some(previous_css_class) =
            self.transparency_css_class.replace(Some(css_class.clone()))
            && previous_css_class != css_class
        {
            self.root.remove_css_class(&previous_css_class);
            self.header.remove_css_class(&previous_css_class);
            self.body.remove_css_class(&previous_css_class);
        }
        self.root.add_css_class(&css_class);
        self.header.add_css_class(&css_class);
        self.body.add_css_class(&css_class);
        let css = transparency_policy_css(&css_class, policy);
        self.transparency_css.replace(css.clone());
        if let Some(display) = gtk::gdk::Display::default() {
            install_transparency_policy_css(&display, &css, &self.transparency_css_provider);
        }
    }
}

/// CSS classes used by [`BigApplicationOverlayChrome`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigApplicationOverlayChromeClasses {
    body_column_css_class: String,
    overlay_root_css_class: String,
}

impl BigApplicationOverlayChromeClasses {
    /// Create the CSS class mapping for an app-specific overlay chrome.
    #[must_use]
    pub fn new(
        body_column_css_class: impl Into<String>,
        overlay_root_css_class: impl Into<String>,
    ) -> Self {
        Self {
            body_column_css_class: body_column_css_class.into(),
            overlay_root_css_class: overlay_root_css_class.into(),
        }
    }

    /// CSS class applied to the vertical body column.
    #[must_use]
    pub fn body_column_css_class(&self) -> &str {
        &self.body_column_css_class
    }

    /// CSS class applied to the `GtkOverlay` root.
    #[must_use]
    pub fn overlay_root_css_class(&self) -> &str {
        &self.overlay_root_css_class
    }
}

/// Shared overlay chrome for apps that own their header/body widgets.
///
/// This covers the reusable GTK structure used by workspace-style apps:
/// vertical body column, measured header spacer, shared overlay root, header
/// revealer, command-toolbar drag handle, and toast mounting. App-specific
/// reveal behavior stays outside this helper and wires through the exposed
/// overlay/header-spacer accessors.
#[derive(Clone)]
pub struct BigApplicationOverlayChrome {
    body_column: gtk::Box,
    header_spacer: gtk::Box,
    overlay_root: gtk::Overlay,
    header_revealer: gtk::Revealer,
}

impl BigApplicationOverlayChrome {
    /// Assemble the shared overlay chrome around app-owned widgets.
    #[must_use]
    pub fn new(
        header: &impl IsA<gtk::Widget>,
        command_toolbar: &impl IsA<gtk::Widget>,
        body: &impl IsA<gtk::Widget>,
        classes: BigApplicationOverlayChromeClasses,
    ) -> Self {
        let body_column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body_column.add_css_class(classes.body_column_css_class());

        let header_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body_column.append(&header_spacer);
        body_column.append(body);

        let overlay_root = gtk::Overlay::new();
        overlay_root.add_css_class(classes.overlay_root_css_class());
        overlay_root.set_child(Some(&body_column));

        let header_stack = gtk::Box::new(gtk::Orientation::Vertical, 0);
        header_stack.append(header);

        let toolbar_drag_handle = gtk::WindowHandle::builder().child(command_toolbar).build();
        command_toolbar
            .bind_property("visible", &toolbar_drag_handle, "visible")
            .sync_create()
            .build();
        header_stack.append(&toolbar_drag_handle);

        let header_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .child(&header_stack)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Start)
            .build();
        overlay_root.add_overlay(&header_revealer);
        overlay_root.set_measure_overlay(&header_revealer, false);

        Self {
            body_column,
            header_spacer,
            overlay_root,
            header_revealer,
        }
    }

    /// Insert content directly below the measured header spacer.
    pub fn insert_body_widget_after_header_spacer(&self, widget: &impl IsA<gtk::Widget>) {
        self.body_column
            .insert_child_after(widget, Some(&self.header_spacer));
    }

    /// Mount this chrome into the provided toast overlay.
    pub fn mount_toast_overlay(&self, toast_overlay: &adw::ToastOverlay) {
        toast_overlay.set_child(Some(&self.overlay_root));
    }

    /// Borrow the shared overlay root.
    #[must_use]
    pub fn overlay_root(&self) -> &gtk::Overlay {
        &self.overlay_root
    }

    /// Borrow the header revealer so apps can wire reveal policy.
    #[must_use]
    pub fn header_revealer(&self) -> &gtk::Revealer {
        &self.header_revealer
    }

    /// Borrow the measured spacer that reserves room for floating chrome.
    #[must_use]
    pub fn header_spacer(&self) -> &gtk::Box {
        &self.header_spacer
    }

    /// Borrow the vertical body column.
    #[must_use]
    pub fn body_column(&self) -> &gtk::Box {
        &self.body_column
    }
}

/// Initialization payload for [`BigWorkspaceWindowShell`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigWorkspaceWindowShellInit {
    /// Display-free workspace shell spec.
    pub shell_spec: BigWorkspaceWindowShellSpec,
}

impl BigWorkspaceWindowShellInit {
    /// Create a workspace shell initialization payload.
    #[must_use]
    pub fn new(shell_spec: BigWorkspaceWindowShellSpec) -> Self {
        Self { shell_spec }
    }
}

/// Inputs understood by a future Relm4 component wrapper around the workspace shell.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWorkspaceWindowShellInput {
    /// Forward an input to the base application-window shell.
    Application(BigApplicationWindowShellInput),
    /// Move focus to the workspace tab strip.
    FocusWorkspaceTabs,
    /// Move focus to the split-pane body.
    FocusSplitPaneBody,
    /// Toggle the sidebar visibility.
    SetSidebarVisible(bool),
    /// Toggle the dock visibility.
    SetDockVisible(bool),
}

/// Outputs emitted by workspace shell action surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigWorkspaceWindowShellOutput {
    /// Output forwarded from the base application-window shell.
    Application(BigApplicationWindowShellOutput),
    /// The selected workspace tab changed.
    WorkspaceTabSelectionChanged,
    /// The focused split-pane changed.
    FocusedPaneChanged,
    /// A workspace action identified by action id was activated.
    WorkspaceActionActivated(String),
}

/// Error returned when a consumer mounts content into a disabled workspace region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigWorkspaceWindowRegionUnavailable {
    region: BigWorkspaceWindowRegion,
}

impl BigWorkspaceWindowRegionUnavailable {
    const fn new(region: BigWorkspaceWindowRegion) -> Self {
        Self { region }
    }

    /// The workspace region that was not enabled by the shell spec.
    #[must_use]
    pub const fn region(self) -> BigWorkspaceWindowRegion {
        self.region
    }
}

impl fmt::Display for BigWorkspaceWindowRegionUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "workspace window region {:?} is not enabled",
            self.region
        )
    }
}

impl std::error::Error for BigWorkspaceWindowRegionUnavailable {}

/// Visual shell for BigTerminal-grade BigLinux workspace windows.
///
/// The generic [`BigWorkspaceShell`] still owns tab registry, split trees, and
/// focus fan-out. This wrapper mounts it into the standard application-window
/// chrome with optional sidebar, dock, and workspace overlay regions.
#[derive(Clone)]
pub struct BigWorkspaceWindowShell<P: SplitPaneSurface + 'static, M: 'static> {
    resolved_spec: BigWorkspaceWindowShellResolved,
    application_shell: BigApplicationWindowShell,
    workspace_shell: Rc<BigWorkspaceShell<P, M>>,
    workspace_region: gtk::Box,
    workspace_tab_bar_host: gtk::Box,
    tab_strip_placement: BigTabStripPlacement,
    center_region: gtk::Box,
    split_pane_body: gtk::Box,
    header_side_panel: Option<gtk::Box>,
    side_panel_chrome: Option<Rc<SidePanelChromeRuntime>>,
    sidebar: Option<gtk::Box>,
    dock: Option<gtk::Box>,
    workspace_overlay: Option<gtk::Overlay>,
    toolbar_band_policy: Option<BigChromeBandPolicy>,
    toolbar_band_host: Rc<RefCell<Option<BigChromeBandHost>>>,
}

impl<P: SplitPaneSurface + 'static, M: 'static> BigWorkspaceWindowShell<P, M> {
    /// Build the workspace window shell from a display-free spec and an existing
    /// workspace registry/split shell.
    ///
    /// # Errors
    ///
    /// Returns [`BigWindowShellSpecError`] when the provided spec is invalid.
    pub fn new(
        spec: BigWorkspaceWindowShellSpec,
        workspace_shell: Rc<BigWorkspaceShell<P, M>>,
    ) -> Result<Self, BigWindowShellSpecError> {
        let resolved_spec = spec.resolve()?;
        Ok(Self::from_resolved(resolved_spec, workspace_shell))
    }

    /// Build the workspace window shell from an already resolved spec.
    #[must_use]
    pub fn from_resolved(
        resolved_spec: BigWorkspaceWindowShellResolved,
        workspace_shell: Rc<BigWorkspaceShell<P, M>>,
    ) -> Self {
        let application_shell =
            BigApplicationWindowShell::from_resolved(resolved_spec.application_shell.clone());
        let (header_side_panel, side_panel_chrome) = match resolved_spec.side_panel_chrome_policy {
            Some(policy) => {
                let (header_side_panel, runtime) =
                    apply_side_panel_chrome_policy(&application_shell, policy);
                (header_side_panel, Some(runtime))
            }
            None => (None, None),
        };
        let workspace_tab_bar_host = create_workspace_tab_bar_host(&resolved_spec);
        let tab_strip_placement = BigTabStripPlacement::new(
            workspace_shell.strip(),
            &workspace_tab_bar_host,
            &resolved_spec
                .workspace_accessibility_spec
                .tab_list_accessible_name,
        );
        let tab_strip_in_header = Rc::new(Cell::new(true));
        let workspace_title_stack = create_workspace_title_stack(
            &resolved_spec,
            &workspace_shell,
            &workspace_tab_bar_host,
            &tab_strip_in_header,
        );
        application_shell
            .header()
            .set_title_widget(Some(&workspace_title_stack));

        {
            let workspace_title_stack = workspace_title_stack.clone();
            let strip = workspace_shell.strip().clone();
            let tab_strip_in_header = tab_strip_in_header.clone();
            tab_strip_placement.set_on_position_applied(Rc::new(move |in_header| {
                tab_strip_in_header.set(in_header);
                refresh_workspace_title_stack_policy(
                    &workspace_title_stack,
                    strip.n_pages(),
                    &tab_strip_in_header,
                );
            }));
        }

        let split_pane_body = create_split_pane_body(&resolved_spec);
        split_pane_body.append(workspace_shell.root_widget());

        let center_region = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["big-workspace-window-center"])
            .build();
        center_region.append(&required_tab_strip_dock_root(
            &tab_strip_placement,
            BigTabStripPosition::Top,
        ));
        center_region.append(&split_pane_body);
        center_region.append(&required_tab_strip_dock_root(
            &tab_strip_placement,
            BigTabStripPosition::Bottom,
        ));

        let dock =
            has_workspace_region(&resolved_spec, BigWorkspaceWindowRegion::Dock).then(|| {
                let dock = gtk::Box::builder()
                    .orientation(gtk::Orientation::Horizontal)
                    .spacing(6)
                    .css_classes(["big-workspace-window-dock"])
                    .build();
                dock.set_accessible_role(gtk::AccessibleRole::Group);
                if let Some(accessible_name) = &resolved_spec
                    .workspace_accessibility_spec
                    .dock_accessible_name
                {
                    dock.update_property(&[gtk::accessible::Property::Label(accessible_name)]);
                }
                dock
            });
        if let Some(dock) = &dock {
            center_region.append(dock);
        }

        let workspace_region = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["big-workspace-window-region"])
            .build();
        workspace_region.set_accessible_role(gtk::AccessibleRole::Group);
        workspace_region.update_property(&[gtk::accessible::Property::Label(
            &resolved_spec
                .workspace_accessibility_spec
                .workspace_accessible_name,
        )]);

        let sidebar =
            has_workspace_region(&resolved_spec, BigWorkspaceWindowRegion::Sidebar).then(|| {
                let sidebar = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(6)
                    .css_classes(["big-workspace-window-sidebar"])
                    .build();
                sidebar.set_accessible_role(gtk::AccessibleRole::Group);
                if let Some(accessible_name) = &resolved_spec
                    .workspace_accessibility_spec
                    .sidebar_accessible_name
                {
                    sidebar.update_property(&[gtk::accessible::Property::Label(accessible_name)]);
                }
                if let Some(side_panel_chrome_policy) = resolved_spec.side_panel_chrome_policy {
                    sidebar.set_width_request(side_panel_chrome_policy.width_logical_pixels);
                }
                sidebar
            });
        workspace_region.append(&required_tab_strip_dock_root(
            &tab_strip_placement,
            BigTabStripPosition::Start,
        ));
        if let Some(sidebar) = &sidebar {
            workspace_region.append(sidebar);
        }
        workspace_region.append(&center_region);
        workspace_region.append(&required_tab_strip_dock_root(
            &tab_strip_placement,
            BigTabStripPosition::End,
        ));

        let workspace_overlay =
            has_workspace_region(&resolved_spec, BigWorkspaceWindowRegion::WorkspaceOverlay).then(
                || {
                    let overlay = gtk::Overlay::builder()
                        .child(&workspace_region)
                        .hexpand(true)
                        .vexpand(true)
                        .css_classes(["big-workspace-window-overlay"])
                        .build();
                    overlay.set_accessible_role(gtk::AccessibleRole::Group);
                    overlay
                },
            );

        if let Some(workspace_overlay) = &workspace_overlay {
            application_shell.append_body_child(workspace_overlay);
        } else {
            application_shell.append_body_child(&workspace_region);
        }

        let toolbar_band_policy = toolbar_band_policy(&resolved_spec);

        Self {
            resolved_spec,
            application_shell,
            workspace_shell,
            workspace_region,
            workspace_tab_bar_host,
            tab_strip_placement,
            center_region,
            split_pane_body,
            header_side_panel,
            side_panel_chrome,
            sidebar,
            dock,
            workspace_overlay,
            toolbar_band_policy,
            toolbar_band_host: Rc::new(RefCell::new(None)),
        }
    }

    /// Register how the side-strip width persists (called with the final
    /// width after a resize drag; store it under
    /// [`tab_prefs::KEY_TAB_STRIP_SIDE_WIDTH`]).
    pub fn set_tab_strip_side_width_persister(&self, persist: impl Fn(i32) + 'static) {
        self.tab_strip_placement.set_side_width_persister(persist);
    }

    /// Apply the persisted logical width used by start/end tab-strip docks.
    pub fn set_tab_strip_side_width(&self, width: i32) {
        self.tab_strip_placement.set_side_width(width);
    }

    /// Register the strip's new-tab affordance: `header_new_tab` is the app's
    /// widget in the header (hidden while the strip is docked elsewhere); the
    /// shell's stand-in button, activating `action_name`, follows the strip
    /// into the top/bottom/side docks.
    pub fn set_tab_strip_new_tab_action(
        &self,
        action_name: &str,
        accessible_label: &str,
        header_new_tab: &impl IsA<gtk::Widget>,
    ) {
        self.tab_strip_placement
            .set_new_tab_action(action_name, accessible_label, header_new_tab);
    }

    /// Like [`Self::set_tab_strip_new_tab_action`], but the app supplies the
    /// whole dock stand-in: `dock_root` mounts after the tabs (e.g. a
    /// new-tab + more-options group so no header capability is lost
    /// off-header) and `dock_button` is the primary button inside it that
    /// receives the dock styling.
    pub fn set_tab_strip_new_tab_widgets(
        &self,
        dock_root: &impl IsA<gtk::Widget>,
        dock_button: &gtk::Button,
        accessible_label: &str,
        header_new_tab: &impl IsA<gtk::Widget>,
    ) {
        self.tab_strip_placement.set_new_tab_widgets(
            dock_root,
            dock_button,
            accessible_label,
            header_new_tab,
        );
    }

    /// Mount the tab strip at `position`, re-parenting the tab bar between the
    /// header title area and the top/bottom/side docks. Side placements turn
    /// the strip vertical. Idempotent per position; safe to call live.
    pub fn set_tab_strip_position(&self, position: BigTabStripPosition) {
        self.tab_strip_placement
            .set_position(self.workspace_shell.strip(), position);
    }

    /// Adopt an app-owned toolbar band into the config-placed chrome band host.
    ///
    /// The shell is generic over `<P, M>` and cannot build the app's toolbar, so
    /// the app hands in its `band_widget` (the toolbar root) and its `home_host`
    /// (the box the band lives in at its default placement — kept app-local so
    /// app-specific chrome such as a reveal revealer stays outside this shell).
    /// The band host's non-home edge docks are interleaved next to the tab-strip
    /// docks. No-op-with-home-fallback when the spec carries no
    /// [`BigChromeBand::Toolbar`] policy (the band stays visible in `home_host`).
    pub fn mount_toolbar_band(&self, band_widget: &impl IsA<gtk::Widget>, home_host: &gtk::Box) {
        let Some(policy) = &self.toolbar_band_policy else {
            reparent_into_home_host(band_widget, home_host);
            return;
        };
        let names = toolbar_band_dock_accessible_names();
        let host = BigChromeBandHost::new(
            band_widget,
            home_host,
            policy.default_placement,
            &policy.allowed_placements,
            [
                names[0].as_str(),
                names[1].as_str(),
                names[2].as_str(),
                names[3].as_str(),
            ],
        );
        self.interleave_toolbar_band_docks(&host);
        *self.toolbar_band_host.borrow_mut() = Some(host);
    }

    /// Mount the toolbar band at `placement`, reparenting it between the home
    /// host, an edge dock, or hidden. No-op when no band was mounted or the
    /// shell carries no toolbar band policy. Idempotent; safe to call live.
    pub fn set_toolbar_band_placement(&self, placement: BigBandPlacement) {
        if let Some(host) = self.toolbar_band_host.borrow().as_ref() {
            host.set_placement(placement);
        }
    }

    /// Register a callback invoked after each toolbar band placement change
    /// (e.g. to reorient the toolbar's own inner layout). No-op when no band was
    /// mounted.
    pub fn set_toolbar_band_on_placement_applied(&self, callback: BigChromeBandPlacementCallback) {
        if let Some(host) = self.toolbar_band_host.borrow().as_ref() {
            host.set_on_placement_applied(callback);
        }
    }

    /// Borrow a handle to the mounted toolbar band host, if any.
    ///
    /// Returns a cheap clone (shared widget/state handles) rather than a
    /// borrow, because the host is created post-construction behind interior
    /// mutability.
    #[must_use]
    pub fn toolbar_band_host(&self) -> Option<BigChromeBandHost> {
        self.toolbar_band_host.borrow().clone()
    }

    fn interleave_toolbar_band_docks(&self, host: &BigChromeBandHost) {
        if let Some(dock) = host.dock_root(BigBandPlacement::Top) {
            let anchor =
                required_tab_strip_dock_root(&self.tab_strip_placement, BigTabStripPosition::Top);
            self.center_region.insert_child_after(&dock, Some(&anchor));
        }
        if let Some(dock) = host.dock_root(BigBandPlacement::Bottom) {
            self.center_region
                .insert_child_after(&dock, Some(&self.split_pane_body));
        }
        if let Some(dock) = host.dock_root(BigBandPlacement::Start) {
            let anchor =
                required_tab_strip_dock_root(&self.tab_strip_placement, BigTabStripPosition::Start);
            self.workspace_region
                .insert_child_after(&dock, Some(&anchor));
        }
        if let Some(dock) = host.dock_root(BigBandPlacement::End) {
            self.workspace_region
                .insert_child_after(&dock, Some(&self.center_region));
        }
    }

    /// Apply the full suite tab personalization at window level: strip
    /// position (mount + orientation) plus the strip-level settings
    /// (alignment, expand-evenly, close mode, theme CSS). Call on boot and on
    /// any [`tab_prefs::TAB_SETTING_KEYS`] change.
    pub fn apply_tab_personalization(
        &self,
        store: &dyn crate::input::preference_rows::BigPreferenceStore,
        theme_provider_slot: &mut Option<gtk::CssProvider>,
    ) {
        let position = tab_prefs::tab_strip_position_setting(store);
        let side_width = tab_prefs::tab_strip_side_width_setting(store);
        self.tab_strip_placement.set_side_width(side_width);
        self.set_tab_strip_position(position);
        self.workspace_shell
            .apply_tab_personalization(store, theme_provider_slot);
        if position.is_vertical() {
            // Alignment is a horizontal-strip concept; a vertical strip fills
            // its dock's width and anchors to the top.
            let tab_bar = self.workspace_shell.strip().tab_bar();
            tab_bar.set_halign(gtk::Align::Fill);
            tab_bar.set_valign(gtk::Align::Start);
        }
    }

    /// Apply a new application-window transparency policy to this workspace shell.
    pub fn apply_transparency_policy(&self, policy: BigWindowTransparencyPolicy) {
        self.application_shell.apply_transparency_policy(policy);
    }

    /// Refresh side-panel/body chrome opacity without rebuilding the shell.
    /// No-op for shells built without a side-panel chrome policy.
    pub fn refresh_side_panel_chrome_policy(&self, policy: BigWorkspaceSidePanelChromePolicy) {
        let Some(side_panel_chrome) = &self.side_panel_chrome else {
            return;
        };
        refresh_side_panel_chrome_policy(
            side_panel_chrome,
            &self.application_shell,
            self.header_side_panel.as_ref(),
            self.sidebar.as_ref(),
            policy,
        );
    }

    /// Borrow the resolved display-free workspace contract.
    #[must_use]
    pub fn resolved_spec(&self) -> &BigWorkspaceWindowShellResolved {
        &self.resolved_spec
    }

    /// Borrow the composed base application shell.
    #[must_use]
    pub fn application_shell(&self) -> &BigApplicationWindowShell {
        &self.application_shell
    }

    /// Borrow the underlying workspace registry/split shell.
    #[must_use]
    pub fn workspace_shell(&self) -> &Rc<BigWorkspaceShell<P, M>> {
        &self.workspace_shell
    }

    /// Borrow the top-level root widget from the composed application shell.
    #[must_use]
    pub fn root(&self) -> &adw::ToolbarView {
        self.application_shell.root()
    }

    /// Apply this workspace shell's window-level defaults and mount its root in
    /// an `adw::Window`.
    ///
    /// This is the ergonomic path for product windows: the host creates the
    /// concrete `adw::Window` or `adw::ApplicationWindow`, while the shared shell
    /// owns the common title, size, and content contract.
    pub fn mount_into_adw_window(&self, window: &adw::Window) {
        self.application_shell.mount_into_adw_window(window);
    }

    /// Apply this workspace shell's window-level defaults and mount its root in
    /// an `adw::ApplicationWindow`.
    pub fn mount_into_adw_application_window(&self, window: &adw::ApplicationWindow) {
        self.application_shell
            .mount_into_adw_application_window(window);
    }

    /// Borrow the horizontal workspace region.
    #[must_use]
    pub fn workspace_region(&self) -> &gtk::Box {
        &self.workspace_region
    }

    /// Borrow the mounted workspace tab-bar host.
    #[must_use]
    pub fn workspace_tab_bar_host(&self) -> &gtk::Box {
        &self.workspace_tab_bar_host
    }

    /// Borrow the split-pane body container.
    #[must_use]
    pub fn split_pane_body(&self) -> &gtk::Box {
        &self.split_pane_body
    }

    /// Borrow the optional header side-panel slot.
    ///
    /// This region aligns app-owned leading controls with the body sidebar,
    /// keeping navigation buttons inside the sidebar-colored chrome band.
    #[must_use]
    pub fn header_side_panel(&self) -> Option<&gtk::Box> {
        self.header_side_panel.as_ref()
    }

    /// Append app-owned leading header content.
    ///
    /// When side-panel chrome extends into the header, this mounts the child
    /// inside that sidebar-colored header band. Otherwise it falls back to the
    /// standard header start slot.
    pub fn append_header_start_child(&self, child: &impl IsA<gtk::Widget>) {
        if let Some(header_side_panel) = &self.header_side_panel {
            header_side_panel.append(child);
        } else {
            self.application_shell.append_header_start_child(child);
        }
    }

    /// Append app-owned trailing header content.
    pub fn append_header_end_child(&self, child: &impl IsA<gtk::Widget>) {
        self.application_shell.append_header_end_child(child);
    }

    /// Borrow the optional sidebar mount.
    #[must_use]
    pub fn sidebar(&self) -> Option<&gtk::Box> {
        self.sidebar.as_ref()
    }

    /// Append app-owned content to the enabled sidebar region.
    ///
    /// # Errors
    ///
    /// Returns [`BigWorkspaceWindowRegionUnavailable`] when the shell spec did
    /// not enable [`BigWorkspaceWindowRegion::Sidebar`].
    pub fn append_sidebar_child(
        &self,
        child: &impl IsA<gtk::Widget>,
    ) -> Result<(), BigWorkspaceWindowRegionUnavailable> {
        let Some(sidebar) = &self.sidebar else {
            return Err(BigWorkspaceWindowRegionUnavailable::new(
                BigWorkspaceWindowRegion::Sidebar,
            ));
        };
        sidebar.append(child);
        Ok(())
    }

    /// Borrow the optional dock mount.
    #[must_use]
    pub fn dock(&self) -> Option<&gtk::Box> {
        self.dock.as_ref()
    }

    /// Append app-owned content to the enabled dock region.
    ///
    /// # Errors
    ///
    /// Returns [`BigWorkspaceWindowRegionUnavailable`] when the shell spec did
    /// not enable [`BigWorkspaceWindowRegion::Dock`].
    pub fn append_dock_child(
        &self,
        child: &impl IsA<gtk::Widget>,
    ) -> Result<(), BigWorkspaceWindowRegionUnavailable> {
        let Some(dock) = &self.dock else {
            return Err(BigWorkspaceWindowRegionUnavailable::new(
                BigWorkspaceWindowRegion::Dock,
            ));
        };
        dock.append(child);
        Ok(())
    }

    /// Borrow the optional workspace overlay.
    #[must_use]
    pub fn workspace_overlay(&self) -> Option<&gtk::Overlay> {
        self.workspace_overlay.as_ref()
    }

    /// Add app-owned content to the enabled workspace overlay.
    ///
    /// # Errors
    ///
    /// Returns [`BigWorkspaceWindowRegionUnavailable`] when the shell spec did
    /// not enable [`BigWorkspaceWindowRegion::WorkspaceOverlay`].
    pub fn add_workspace_overlay_child(
        &self,
        overlay_child: &impl IsA<gtk::Widget>,
        should_measure_overlay: bool,
    ) -> Result<(), BigWorkspaceWindowRegionUnavailable> {
        let Some(workspace_overlay) = &self.workspace_overlay else {
            return Err(BigWorkspaceWindowRegionUnavailable::new(
                BigWorkspaceWindowRegion::WorkspaceOverlay,
            ));
        };
        workspace_overlay.add_overlay(overlay_child);
        workspace_overlay.set_measure_overlay(overlay_child, should_measure_overlay);
        Ok(())
    }
}

fn has_region(
    resolved_spec: &BigApplicationWindowShellResolved,
    region: BigApplicationWindowRegion,
) -> bool {
    resolved_spec.regions.contains(&region)
}

fn create_split_pane_body(resolved_spec: &BigWorkspaceWindowShellResolved) -> gtk::Box {
    let split_pane_body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["big-workspace-window-split-pane-body"])
        .build();
    split_pane_body.set_accessible_role(gtk::AccessibleRole::Group);
    split_pane_body.update_property(&[gtk::accessible::Property::Label(
        &resolved_spec
            .workspace_accessibility_spec
            .split_area_accessible_name,
    )]);
    split_pane_body
}

fn required_tab_strip_dock_root(
    placement: &BigTabStripPlacement,
    position: BigTabStripPosition,
) -> gtk::Widget {
    placement
        .dock_root(position)
        .expect("non-header tab strip position has a dock root")
}

/// The [`BigChromeBand::Toolbar`] band policy from the resolved chrome-placement
/// policy, if the spec configures a movable toolbar band.
fn toolbar_band_policy(
    resolved_spec: &BigWorkspaceWindowShellResolved,
) -> Option<BigChromeBandPolicy> {
    resolved_spec.chrome_placement.as_ref().and_then(|policy| {
        policy
            .bands
            .iter()
            .find(|band| band.band == BigChromeBand::Toolbar)
            .cloned()
    })
}

/// Translated `[top, bottom, start, end]` accessible names for the toolbar band
/// edge docks (component textdomain).
fn toolbar_band_dock_accessible_names() -> [String; 4] {
    [
        t(gettext_noop("Top toolbar")),
        t(gettext_noop("Bottom toolbar")),
        t(gettext_noop("Start toolbar")),
        t(gettext_noop("End toolbar")),
    ]
}

/// Leak-safe home mount used when a toolbar band is handed in without a
/// configured band policy: keep the band visible at its home host.
fn reparent_into_home_host(band_widget: &impl IsA<gtk::Widget>, home_host: &gtk::Box) {
    let band: gtk::Widget = band_widget.clone().upcast();
    if band.parent().as_ref() == Some(home_host.upcast_ref::<gtk::Widget>()) {
        return;
    }
    if band.parent().is_some() {
        band.unparent();
    }
    home_host.append(&band);
}

fn has_workspace_region(
    resolved_spec: &BigWorkspaceWindowShellResolved,
    region: BigWorkspaceWindowRegion,
) -> bool {
    resolved_spec.workspace_regions.contains(&region)
}

fn transparency_policy_css_class(policy: BigWindowTransparencyPolicy) -> String {
    match policy {
        BigWindowTransparencyPolicy::Opaque => "big-window-transparency-opaque".to_owned(),
        BigWindowTransparencyPolicy::TranslucentBody { body_opacity } => {
            format!("big-window-transparency-body-{}", body_opacity.value())
        }
        BigWindowTransparencyPolicy::TranslucentChromeAndBody {
            chrome_opacity,
            body_opacity,
        } => format!(
            "big-window-transparency-chrome-{}-body-{}",
            chrome_opacity.value(),
            body_opacity.value()
        ),
        BigWindowTransparencyPolicy::TransparentRoot => "big-window-transparency-root".to_owned(),
        _ => "big-window-transparency-opaque".to_owned(),
    }
}

fn transparency_policy_css(css_class: &str, policy: BigWindowTransparencyPolicy) -> String {
    match policy {
        BigWindowTransparencyPolicy::Opaque => format!(
            ".{css_class}.big-application-window-header,
            .{css_class} .big-application-window-header,
            .{css_class} .big-application-window-header windowhandle,
            .{css_class}.big-application-window-body,
            .{css_class} .big-application-window-body {{
                background-color: alpha(@window_bg_color, 1.00);
                background-image: none;
            }}
            {}",
            transparency_policy_reset_css()
        ),
        BigWindowTransparencyPolicy::TranslucentBody { body_opacity } => {
            let body_alpha = opacity_alpha(body_opacity);
            format!(
                ".{css_class}.big-application-window-body,
                .{css_class} .big-application-window-body {{
                    background-color: alpha(@window_bg_color, {body_alpha});
                    background-image: none;
                }}
                {}",
                big_appearance::chrome::shell_layers_transparent_css(&[])
            )
        }
        BigWindowTransparencyPolicy::TranslucentChromeAndBody {
            chrome_opacity,
            body_opacity,
        } => {
            let chrome_alpha = opacity_alpha(chrome_opacity);
            let body_alpha = opacity_alpha(body_opacity);
            format!(
                ".{css_class}.big-application-window-header,
                .{css_class} .big-application-window-header,
                .{css_class} .big-application-window-header windowhandle {{
                    background-color: alpha(@window_bg_color, {chrome_alpha});
                    background-image: none;
                }}
                .{css_class}.big-application-window-body,
                .{css_class} .big-application-window-body {{
                    background-color: alpha(@window_bg_color, {body_alpha});
                    background-image: none;
                }}
                {}",
                big_appearance::chrome::shell_layers_transparent_css(&[])
            )
        }
        BigWindowTransparencyPolicy::TransparentRoot => format!(
            ".{css_class}.big-application-window-shell,
            .{css_class} .big-application-window-body {{
                background-color: transparent;
                background-image: none;
            }}
            {}",
            big_appearance::chrome::shell_layers_transparent_css(&[])
        ),
        _ => String::new(),
    }
}

fn transparency_policy_reset_css() -> &'static str {
    r"
            window.big-main-window.background {
                background-color: @window_bg_color;
                background-image: none;
            }
            .big-main-window .big-window-toast-root,
            .big-main-window .big-window-overlay,
            .big-main-window .big-window-body,
            .big-main-window .big-dock-root,
            .big-main-window .big-dock-paned,
            .big-main-window .big-workspace-shell,
            .big-main-window .big-tab-content-stack,
            .big-main-window .big-split-tree {
                background-color: @window_bg_color;
                background-image: none;
            }"
}

fn opacity_alpha(opacity: BigWindowOpacityPercent) -> String {
    format!("{:.2}", f64::from(opacity.value()) / 100.0)
}

fn install_transparency_policy_css(
    display: &gtk::gdk::Display,
    css: &str,
    provider_slot: &RefCell<Option<gtk::CssProvider>>,
) {
    if let Some(provider) = provider_slot.borrow_mut().take() {
        gtk::style_context_remove_provider_for_display(display, &provider);
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(css);
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    provider_slot.replace(Some(provider));
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_app_kit::window_shell::{
        BigChromePlacementPolicy, BigWindowOverlayPolicy, BigWindowToastPolicy,
        BigWorkspaceCapability,
    };

    use crate::layout::tab_strip_host::BigTabStrip;

    /// Minimal split-pane surface so the workspace shell can be built in a GTK
    /// smoke test without any real pane content.
    struct StubPane {
        root: gtk::Box,
    }

    impl StubPane {
        fn new() -> Rc<Self> {
            Rc::new(Self {
                root: gtk::Box::new(gtk::Orientation::Vertical, 0),
            })
        }
    }

    impl SplitPaneSurface for StubPane {
        fn pane_root(&self) -> gtk::Widget {
            self.root.clone().upcast()
        }
        fn set_header_visible(&self, _visible: bool) {}
        fn grab_focus(&self) {}
    }

    fn toolbar_band_workspace_shell() -> Option<BigWorkspaceWindowShell<StubPane, String>> {
        if gtk::init().is_err() || adw::init().is_err() {
            return None;
        }
        let policy = BigChromePlacementPolicy::new().band(
            BigChromeBandPolicy::new(BigChromeBand::Toolbar, BigBandPlacement::Top)
                .allowed_placements([
                    BigBandPlacement::Top,
                    BigBandPlacement::Bottom,
                    BigBandPlacement::Start,
                    BigBandPlacement::End,
                    BigBandPlacement::Hidden,
                ])
                .can_hide(true),
        );
        let resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.BandTest", "Band Test")
                .chrome_placement(policy)
                .resolve()
                .expect("valid toolbar band workspace spec");
        let _pane = StubPane::new();
        let workspace_shell = BigWorkspaceShell::<StubPane, String>::new(BigTabStrip::default());
        Some(BigWorkspaceWindowShell::from_resolved(
            resolved,
            workspace_shell,
        ))
    }

    #[test]
    fn toolbar_band_builds_non_home_edge_docks_and_reparents() {
        let Some(shell) = toolbar_band_workspace_shell() else {
            return;
        };
        // Opt-in: nothing mounted before the app hands in its toolbar.
        assert!(shell.toolbar_band_host().is_none());

        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let home_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shell.mount_toolbar_band(&toolbar, &home_host);

        let host = shell.toolbar_band_host().expect("host present after mount");
        // Home is Top → no Top dock; the other allowed edges get docks.
        assert!(host.dock_root(BigBandPlacement::Top).is_none());
        assert!(host.dock_root(BigBandPlacement::Bottom).is_some());
        assert!(host.dock_root(BigBandPlacement::Start).is_some());
        assert!(host.dock_root(BigBandPlacement::End).is_some());
        // The band starts in the app-owned home host.
        assert_eq!(home_host.first_child(), Some(toolbar.clone().upcast()));

        // The bottom dock is interleaved into the center region.
        let bottom_dock = host.dock_root(BigBandPlacement::Bottom).unwrap();
        assert_eq!(
            bottom_dock.parent().as_ref(),
            Some(shell.center_region.upcast_ref())
        );
        // The end dock is interleaved into the workspace region.
        let end_dock = host.dock_root(BigBandPlacement::End).unwrap();
        assert_eq!(
            end_dock.parent().as_ref(),
            Some(shell.workspace_region.upcast_ref())
        );

        // Move to the bottom edge: band leaves home and the dock shows.
        shell.set_toolbar_band_placement(BigBandPlacement::Bottom);
        assert_eq!(host.placement(), BigBandPlacement::Bottom);
        assert!(home_host.first_child().is_none());
        assert!(bottom_dock.is_visible());
    }

    #[test]
    fn no_toolbar_band_policy_keeps_band_in_home_host() {
        if gtk::init().is_err() || adw::init().is_err() {
            return;
        }
        // Standard spec, no chrome-placement policy → no band host.
        let resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.NoBand", "No Band Test")
                .resolve()
                .expect("valid workspace spec");
        let workspace_shell = BigWorkspaceShell::<StubPane, String>::new(BigTabStrip::default());
        let shell = BigWorkspaceWindowShell::from_resolved(resolved, workspace_shell);

        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let home_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shell.mount_toolbar_band(&toolbar, &home_host);

        assert!(shell.toolbar_band_host().is_none());
        // Graceful fallback: the band still shows at its home host.
        assert_eq!(home_host.first_child(), Some(toolbar.clone().upcast()));
    }

    #[test]
    fn shell_init_preserves_display_free_spec() {
        let spec = BigApplicationWindowShellSpec::new("br.com.biglinux.Editor", "Editor");
        let init = BigApplicationWindowShellInit::new(spec.clone());

        assert_eq!(init.shell_spec, spec);
    }

    #[test]
    fn resolved_spec_selects_overlay_and_toast_regions() {
        let resolved_spec = BigApplicationWindowShellSpec::new("br.com.biglinux.Editor", "Editor")
            .overlay_policy(BigWindowOverlayPolicy::SharedOverlayRoot)
            .toast_policy(BigWindowToastPolicy::Enabled)
            .resolve()
            .expect("valid shell spec");

        assert!(has_region(
            &resolved_spec,
            BigApplicationWindowRegion::Overlay
        ));
        assert!(has_region(
            &resolved_spec,
            BigApplicationWindowRegion::Toast
        ));
    }

    #[test]
    fn overlay_chrome_classes_preserve_app_specific_names() {
        let classes =
            BigApplicationOverlayChromeClasses::new("big-window-body", "big-window-overlay");

        assert_eq!(classes.body_column_css_class(), "big-window-body");
        assert_eq!(classes.overlay_root_css_class(), "big-window-overlay");
    }

    #[test]
    fn workspace_init_preserves_display_free_spec() {
        let application_shell =
            BigApplicationWindowShellSpec::new("br.com.biglinux.Terminal", "Terminal");
        let spec = BigWorkspaceWindowShellSpec::new(application_shell);
        let init = BigWorkspaceWindowShellInit::new(spec.clone());

        assert_eq!(init.shell_spec, spec);
    }

    #[test]
    fn resolved_workspace_spec_selects_sidebar_dock_and_overlay_regions() {
        let application_shell =
            BigApplicationWindowShellSpec::new("br.com.biglinux.Terminal", "Terminal")
                .overlay_policy(BigWindowOverlayPolicy::SharedOverlayRoot);
        let resolved_spec = BigWorkspaceWindowShellSpec::new(application_shell)
            .capability(BigWorkspaceCapability::Sidebar)
            .capability(BigWorkspaceCapability::Dock)
            .capability(BigWorkspaceCapability::WorkspaceOverlay)
            .resolve()
            .expect("valid workspace shell spec");

        assert!(has_workspace_region(
            &resolved_spec,
            BigWorkspaceWindowRegion::Sidebar
        ));
        assert!(has_workspace_region(
            &resolved_spec,
            BigWorkspaceWindowRegion::Dock
        ));
        assert!(has_workspace_region(
            &resolved_spec,
            BigWorkspaceWindowRegion::WorkspaceOverlay
        ));
    }

    #[test]
    fn unavailable_region_error_reports_region() {
        let error = BigWorkspaceWindowRegionUnavailable::new(BigWorkspaceWindowRegion::Dock);

        assert_eq!(error.region(), BigWorkspaceWindowRegion::Dock);
        assert!(error.to_string().contains("Dock"));
    }

    #[test]
    fn apply_transparency_policy_swaps_previous_policy_css_class() {
        if gtk::init().is_err() || adw::init().is_err() {
            return;
        }
        let shell = BigApplicationWindowShell::new(BigApplicationWindowShellSpec::new(
            "br.com.biglinux.Editor",
            "Editor",
        ))
        .expect("valid shell spec");
        let translucent_policy = BigWindowTransparencyPolicy::TranslucentBody {
            body_opacity: BigWindowOpacityPercent::new(60).expect("valid opacity"),
        };
        let translucent_class = transparency_policy_css_class(translucent_policy);
        let opaque_class = transparency_policy_css_class(BigWindowTransparencyPolicy::Opaque);

        shell.apply_transparency_policy(translucent_policy);
        assert!(shell.root.has_css_class(&translucent_class));

        shell.apply_transparency_policy(BigWindowTransparencyPolicy::Opaque);
        assert!(shell.root.has_css_class(&opaque_class));
        for widget in [
            shell.root.clone().upcast::<gtk::Widget>(),
            shell.header.clone().upcast(),
            shell.body.clone().upcast(),
        ] {
            assert!(
                !widget.has_css_class(&translucent_class),
                "stale transparency class must be removed on policy change"
            );
        }
    }

    #[test]
    fn translucent_policy_css_clears_window_root_and_shared_shell_layers() {
        let css = transparency_policy_css(
            "big-window-transparency-body-60",
            BigWindowTransparencyPolicy::TranslucentBody {
                body_opacity: BigWindowOpacityPercent::new(60).expect("valid opacity"),
            },
        );

        assert!(css.contains("window.big-main-window.background"));
        assert!(css.contains("background-color: transparent;"));
        assert!(css.contains(".big-main-window .big-workspace-shell"));
        assert!(css.contains(".big-main-window .big-tab-content-stack"));
    }

    #[test]
    fn opaque_policy_css_resets_window_root_and_shared_shell_layers() {
        let css = transparency_policy_css(
            "big-window-transparency-opaque",
            BigWindowTransparencyPolicy::Opaque,
        );

        assert!(css.contains("window.big-main-window.background"));
        assert!(css.contains("background-color: @window_bg_color;"));
        assert!(css.contains(".big-main-window .big-workspace-shell"));
        assert!(css.contains(".big-main-window .big-tab-content-stack"));
    }
}
