// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Typed app shell contracts.

/// High-level window layout the app wants. Each kind selects a
/// different default header policy and set of regions; the actual
/// widget tree is built by the adapter that consumes a
/// [`BigShellResolved`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigShellKind {
    /// Plain window with header + content; the default for tools that
    /// show one screen.
    SingleWindow,
    /// Master/detail layout backed by a sidebar; selecting a row in
    /// the sidebar replaces the content pane.
    SidebarDetail,
    /// Settings-style shell with a navigation sidebar and a stack of
    /// preference pages.
    ControlCenter,
    /// Document-style shell with tabs across the top.
    Tabbed,
    /// File processing shell: queue sidebar plus footer for batch
    /// controls (start, pause, clear).
    FileQueue,
    /// Step-by-step wizard with a footer holding Back/Next/Finish.
    Wizard,
    /// Pre-libadwaita toolbar look kept for legacy apps.
    Traditional,
}

/// Where the chrome (title, primary actions) lives relative to the
/// content pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigHeaderPolicy {
    /// One header bar above the entire window (the libadwaita
    /// default).
    Unified,
    /// Split header: sidebar gets its own bar, content gets another.
    SidebarAndContent,
    /// Traditional menu bar + toolbar instead of a header bar.
    TraditionalToolbar,
}

/// Display-free description of an app window the toolkit can realise.
///
/// Apps build a spec, optionally chain setter methods, then call
/// [`BigShellSpec::resolved`] to obtain the runtime data the widget
/// adapter consumes. `apply_kind_defaults` ensures every kind starts
/// from a sensible header/region configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigShellSpec {
    /// Reverse-DNS application identifier
    /// (`"br.com.biglinux.Settings"`); also the GTK app ID and the
    /// D-Bus name.
    pub app_id: String,
    /// Localized window title.
    pub title: String,
    /// Selected layout kind; the new-constructor uses this to apply
    /// region defaults.
    pub kind: BigShellKind,
    /// Where the chrome lives; defaulted by the kind but
    /// overridable via [`BigShellSpec::header_policy`].
    pub header_policy: BigHeaderPolicy,
    /// Initial window width in logical pixels.
    pub default_width: i32,
    /// Initial window height in logical pixels.
    pub default_height: i32,
    /// `true` when the window needs an `AdwToastOverlay` wrapping the
    /// content for ephemeral notifications.
    pub uses_toasts: bool,
    /// `true` when the layout includes a sidebar region.
    pub uses_sidebar: bool,
    /// `true` when the layout includes a tab strip.
    pub uses_tabs: bool,
    /// `true` when the layout includes a footer (queue controls,
    /// wizard buttons).
    pub uses_footer: bool,
}

impl BigShellSpec {
    /// Build a spec for `kind` with the default size, unified header,
    /// toast overlay enabled, and the kind-specific region defaults
    /// applied.
    #[must_use]
    pub fn new(app_id: impl Into<String>, title: impl Into<String>, kind: BigShellKind) -> Self {
        let mut spec = Self {
            app_id: app_id.into(),
            title: title.into(),
            kind,
            header_policy: BigHeaderPolicy::Unified,
            default_width: 960,
            default_height: 640,
            uses_toasts: true,
            uses_sidebar: false,
            uses_tabs: false,
            uses_footer: false,
        };
        spec.apply_kind_defaults();
        spec
    }

    /// Shortcut for `new(.., .., BigShellKind::SingleWindow)`.
    #[must_use]
    pub fn single_window(app_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(app_id, title, BigShellKind::SingleWindow)
    }

    /// Shortcut for `new(.., .., BigShellKind::SidebarDetail)`; also
    /// enables the sidebar region and split header.
    #[must_use]
    pub fn sidebar_detail(app_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(app_id, title, BigShellKind::SidebarDetail)
    }

    /// Shortcut for `new(.., .., BigShellKind::ControlCenter)`; shares
    /// the sidebar defaults with `sidebar_detail`.
    #[must_use]
    pub fn control_center(app_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(app_id, title, BigShellKind::ControlCenter)
    }

    /// Shortcut for `new(.., .., BigShellKind::Tabbed)`; enables the
    /// tab strip region.
    #[must_use]
    pub fn tabbed(app_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(app_id, title, BigShellKind::Tabbed)
    }

    /// Shortcut for `new(.., .., BigShellKind::FileQueue)`; enables
    /// sidebar plus footer.
    #[must_use]
    pub fn file_queue(app_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(app_id, title, BigShellKind::FileQueue)
    }

    /// Shortcut for `new(.., .., BigShellKind::Wizard)`; enables a
    /// footer holding Back/Next/Finish.
    #[must_use]
    pub fn wizard(app_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(app_id, title, BigShellKind::Wizard)
    }

    /// Shortcut for `new(.., .., BigShellKind::Traditional)`; selects
    /// the legacy toolbar look and adds a footer.
    #[must_use]
    pub fn traditional(app_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(app_id, title, BigShellKind::Traditional)
    }

    /// Override the initial window size.
    #[must_use]
    pub fn default_size(mut self, width: i32, height: i32) -> Self {
        self.default_width = width;
        self.default_height = height;
        self
    }

    /// Override the header policy; useful when an app wants
    /// `SidebarAndContent` even without a sidebar kind.
    #[must_use]
    pub fn header_policy(mut self, policy: BigHeaderPolicy) -> Self {
        self.header_policy = policy;
        self
    }

    /// Toggle the footer region. Required for wizards and file
    /// queues; opt-in everywhere else.
    #[must_use]
    pub fn footer(mut self, enabled: bool) -> Self {
        self.uses_footer = enabled;
        self
    }

    /// Compute the resolved layout description: ordered regions,
    /// navigation requirement, toast overlay requirement, and final
    /// header policy.
    #[must_use]
    pub fn resolved(&self) -> BigShellResolved {
        BigShellResolved {
            regions: self.regions(),
            requires_navigation: self.uses_sidebar
                || self.uses_tabs
                || self.kind == BigShellKind::Wizard,
            requires_toast_overlay: self.uses_toasts,
            header_policy: self.header_policy,
        }
    }

    fn apply_kind_defaults(&mut self) {
        match self.kind {
            BigShellKind::SingleWindow => {}
            BigShellKind::SidebarDetail | BigShellKind::ControlCenter => {
                self.uses_sidebar = true;
                self.header_policy = BigHeaderPolicy::SidebarAndContent;
            }
            BigShellKind::Tabbed => self.uses_tabs = true,
            BigShellKind::FileQueue => {
                self.uses_sidebar = true;
                self.uses_footer = true;
            }
            BigShellKind::Wizard => self.uses_footer = true,
            BigShellKind::Traditional => {
                self.header_policy = BigHeaderPolicy::TraditionalToolbar;
                self.uses_footer = true;
            }
        }
    }

    fn regions(&self) -> Vec<&'static str> {
        let mut regions = vec!["header", "content"];
        if self.uses_sidebar {
            regions.insert(1, "sidebar");
        }
        if self.uses_tabs {
            regions.insert(1, "tabs");
        }
        if self.uses_footer {
            regions.push("footer");
        }
        regions
    }
}

/// Output of [`BigShellSpec::resolved`]; ready for a widget adapter to
/// consume without re-reading the spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigShellResolved {
    /// Ordered region identifiers (`"header"`, `"sidebar"`, ...)
    /// from top-left to bottom-right of the layout.
    pub regions: Vec<&'static str>,
    /// `true` when the layout exposes a navigation surface (sidebar,
    /// tabs, or wizard footer) that the host should fill.
    pub requires_navigation: bool,
    /// `true` when the host must wrap the content in an
    /// `AdwToastOverlay` for transient toasts.
    pub requires_toast_overlay: bool,
    /// Final header policy after `apply_kind_defaults` and any user
    /// override.
    pub header_policy: BigHeaderPolicy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_center_shell_has_sidebar_navigation() {
        let resolved = BigShellSpec::new(
            "br.com.biglinux.Settings",
            "Settings",
            BigShellKind::ControlCenter,
        )
        .resolved();
        assert!(resolved.requires_navigation);
        assert!(resolved.regions.contains(&"sidebar"));
        assert_eq!(resolved.header_policy, BigHeaderPolicy::SidebarAndContent);
    }

    #[test]
    fn traditional_shell_gets_toolbar_and_footer() {
        let spec = BigShellSpec::new(
            "br.com.biglinux.Legacy",
            "Legacy",
            BigShellKind::Traditional,
        );
        assert_eq!(spec.header_policy, BigHeaderPolicy::TraditionalToolbar);
        assert!(spec.uses_footer);
    }

    #[test]
    fn single_window_shell_does_not_require_navigation() {
        let resolved = BigShellSpec::single_window("br.com.biglinux.Single", "Single").resolved();

        assert!(!resolved.requires_navigation);
        assert_eq!(resolved.regions, vec!["header", "content"]);
    }

    #[test]
    fn tabbed_shell_requires_navigation_without_sidebar() {
        let resolved = BigShellSpec::tabbed("br.com.biglinux.Tabs", "Tabs").resolved();

        assert!(resolved.requires_navigation);
        assert_eq!(resolved.regions, vec!["header", "tabs", "content"]);
    }

    #[test]
    fn wizard_shell_requires_navigation_without_sidebar_or_tabs() {
        let spec = BigShellSpec::wizard("br.com.biglinux.Wizard", "Wizard");
        let resolved = spec.resolved();

        assert!(!spec.uses_sidebar);
        assert!(!spec.uses_tabs);
        assert!(resolved.requires_navigation);
        assert_eq!(resolved.regions, vec!["header", "content", "footer"]);
    }
}
