// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Reusable `adw::NavigationSplitView` scaffold — the framework base for every
//! sidebar layout (the same shape as GNOME Settings / Files).
//!
//! It owns the otherwise-repetitive, GTK-direct assembly (split + a
//! `NavigationPage` / `ToolbarView` / `HeaderBar` per pane) so callers never
//! hand-build it. Both panes take an arbitrary body widget (the caller wraps it
//! in a `ScrolledWindow` if it needs to scroll — the scaffold never force-wraps,
//! so an already-scrolling body like an `adw::ViewStack` of scrollers is not
//! double-wrapped).
//!
//! The sidebar pane's header is always *flat and seamless*: the split auto-tints
//! the sidebar background, and a flat header is transparent so it shows that
//! same tint, blending into the body below (no bottom border, no colour step —
//! this is what removes the "1px line under the sidebar header" and keeps the
//! header from showing the lighter `headerbar` background). It carries no title
//! by default; drop a title widget — plain title text, a primary action button,
//! or a title/search stack — via [`BigSidebarDialog::set_sidebar_header`], and
//! pack extra controls (e.g. a search toggle) into
//! [`BigSidebarDialog::sidebar_header`].
//!
//! Two entry points: [`BigSidebarDialog::new`] builds the split for embedding
//! (exposed via [`BigSidebarDialog::root`]); [`BigSidebarDialog::install`] is the
//! convenience that also sets it as a window's content + title.
//!
//! ```ignore
//! let layout = BigSidebarDialog::install(&window, &BigSidebarDialogSpec::new("Title"));
//! layout.set_sidebar(&my_sidebar_box);
//! layout.set_content(&my_content_box);
//! ```

use adw::prelude::*;
use relm4::gtk;

/// Build-time configuration for [`BigSidebarDialog`].
#[derive(Debug, Clone, PartialEq)]
pub struct BigSidebarDialogSpec {
    /// Sidebar page title (and window title).
    pub title: String,
    /// Content page title.
    pub content_title: String,
    /// Default sidebar width as a fraction of the window. `None` leaves the
    /// split view's own default (only min/max applied).
    pub sidebar_width_fraction: Option<f64>,
    /// Minimum sidebar width in px.
    pub min_sidebar_width: f64,
    /// Maximum sidebar width in px.
    pub max_sidebar_width: f64,
}

impl BigSidebarDialogSpec {
    /// Construct a spec with sensible defaults; `title` labels the sidebar page.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            content_title: String::new(),
            sidebar_width_fraction: Some(0.42),
            min_sidebar_width: 280.0,
            max_sidebar_width: 360.0,
        }
    }

    /// Set the content page title.
    #[must_use]
    pub fn content_title(mut self, title: impl Into<String>) -> Self {
        self.content_title = title.into();
        self
    }

    /// Override the sidebar width policy (fraction, min px, max px).
    #[must_use]
    pub fn sidebar_widths(mut self, fraction: f64, min: f64, max: f64) -> Self {
        self.sidebar_width_fraction = Some(fraction);
        self.min_sidebar_width = min;
        self.max_sidebar_width = max;
        self
    }

    /// Set only the min/max sidebar width, leaving the split view's default
    /// width fraction untouched.
    #[must_use]
    pub fn sidebar_width_range(mut self, min: f64, max: f64) -> Self {
        self.sidebar_width_fraction = None;
        self.min_sidebar_width = min;
        self.max_sidebar_width = max;
        self
    }
}

/// A sidebar split assembled from a [`BigSidebarDialogSpec`]. Holds the pane
/// toolbars (body slots), the sidebar header (for an optional action widget /
/// extra controls), and the content header title.
#[derive(Debug, Clone)]
pub struct BigSidebarDialog {
    root: adw::NavigationSplitView,
    sidebar_toolbar: adw::ToolbarView,
    sidebar_header: adw::HeaderBar,
    content_toolbar: adw::ToolbarView,
    content_header: adw::HeaderBar,
    content_title: adw::WindowTitle,
}

impl BigSidebarDialog {
    /// Build the split for embedding. Use [`Self::root`] to mount it.
    #[must_use]
    pub fn new(spec: &BigSidebarDialogSpec) -> Self {
        // Sidebar pane: flat + transparent header → shows the split's sidebar
        // tint → seamless (no border, no headerbar-coloured band). Title slot is
        // empty until a caller sets one via `set_sidebar_header`.
        let sidebar_toolbar = adw::ToolbarView::new();
        sidebar_toolbar.set_top_bar_style(adw::ToolbarStyle::Flat);
        let sidebar_header = adw::HeaderBar::builder()
            .show_title(false)
            .css_classes(["flat"])
            .build();
        sidebar_toolbar.add_top_bar(&sidebar_header);
        let sidebar_page = adw::NavigationPage::new(&sidebar_toolbar, &spec.title);

        // Content pane: standard header with a live title.
        let content_title = adw::WindowTitle::new(&spec.content_title, "");
        let content_header = adw::HeaderBar::builder()
            .title_widget(&content_title)
            .build();
        let content_toolbar = adw::ToolbarView::new();
        content_toolbar.add_top_bar(&content_header);
        // AdwNavigationPage requires a non-empty title (the collapsed-mode
        // back-button label); fall back to the window title when the caller
        // leaves `content_title` empty — the live header title is the separate
        // `content_title` WindowTitle, updated via `set_content_title`. Without
        // this fallback Adwaita logs "AdwNavigationPage … is missing a title".
        let content_page_title = if spec.content_title.is_empty() {
            spec.title.as_str()
        } else {
            spec.content_title.as_str()
        };
        let content_page = adw::NavigationPage::new(&content_toolbar, content_page_title);

        let mut builder = adw::NavigationSplitView::builder()
            .min_sidebar_width(spec.min_sidebar_width)
            .max_sidebar_width(spec.max_sidebar_width)
            .sidebar(&sidebar_page)
            .content(&content_page);
        if let Some(fraction) = spec.sidebar_width_fraction {
            builder = builder.sidebar_width_fraction(fraction);
        }
        let root = builder.build();

        Self {
            root,
            sidebar_toolbar,
            sidebar_header,
            content_toolbar,
            content_header,
            content_title,
        }
    }

    /// Build the split and set it as `window`'s content (and title).
    #[must_use]
    pub fn install(window: &adw::Window, spec: &BigSidebarDialogSpec) -> Self {
        window.set_title(Some(&spec.title));
        let dialog = Self::new(spec);
        window.set_content(Some(&dialog.root));
        dialog
    }

    /// The assembled split view, for embedding into a larger surface.
    #[must_use]
    pub fn root(&self) -> &adw::NavigationSplitView {
        &self.root
    }

    /// The content header's live title widget, for callers that update it from
    /// a signal without strong-capturing the whole dialog (a `dialog.clone()`
    /// inside a widget-owned closure forms a `root ⇄ content child` ref cycle
    /// that leaks the split view + its content). Downgrade this and upgrade in
    /// the closure instead.
    #[must_use]
    pub fn content_title_widget(&self) -> &adw::WindowTitle {
        &self.content_title
    }

    /// Set the sidebar pane's body widget.
    pub fn set_sidebar(&self, child: &impl IsA<gtk::Widget>) {
        self.sidebar_toolbar.set_content(Some(child));
    }

    /// Place a widget (a title, a primary action button, or a title/search
    /// stack) as the sidebar header's title widget, and reveal the title slot.
    pub fn set_sidebar_header(&self, widget: &impl IsA<gtk::Widget>) {
        self.sidebar_header.set_title_widget(Some(widget));
        self.sidebar_header.set_show_title(true);
    }

    /// The sidebar header, for packing extra controls (search toggle, etc.).
    #[must_use]
    pub fn sidebar_header(&self) -> &adw::HeaderBar {
        &self.sidebar_header
    }

    /// Set the content pane's body widget.
    pub fn set_content(&self, child: &impl IsA<gtk::Widget>) {
        self.content_toolbar.set_content(Some(child));
    }

    /// The content header, for packing extra controls.
    #[must_use]
    pub fn content_header(&self) -> &adw::HeaderBar {
        &self.content_header
    }

    /// Update the content header title (e.g. to append a live count).
    pub fn set_content_title(&self, title: &str) {
        self.content_title.set_title(title);
    }
}
