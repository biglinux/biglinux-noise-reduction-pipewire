// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared toolbar pane and header shells for app pages, sidebars, and utility
//! panes.
//!
//! Apps should use these types instead of constructing `AdwToolbarView` and
//! `AdwHeaderBar` directly. Domain widgets still stay app-owned; the framework
//! owns the common chrome, title-button policy, CSS hooks, and sizing knobs.

use adw::prelude::*;
use relm4::gtk;

/// Display-free spec for [`BigToolbarPane`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BigToolbarPaneSpec {
    /// CSS classes applied to the toolbar view root.
    pub css_classes: Vec<String>,
    /// Optional fixed width request for sidebar or utility panes.
    pub width_request: Option<i32>,
}

impl BigToolbarPaneSpec {
    /// Create a default pane spec with no app-specific styling.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one CSS class to the toolbar view root.
    #[must_use]
    pub fn css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_classes.push(css_class.into());
        self
    }

    /// Set a width request while leaving height natural.
    #[must_use]
    pub fn width_request(mut self, width_request: i32) -> Self {
        self.width_request = Some(width_request);
        self
    }
}

/// Shared `AdwToolbarView` wrapper for panes that need top or bottom bars.
#[derive(Debug, Clone)]
pub struct BigToolbarPane {
    root: adw::ToolbarView,
}

impl BigToolbarPane {
    /// Build a toolbar pane from a semantic spec.
    #[must_use]
    pub fn new(spec: BigToolbarPaneSpec) -> Self {
        let root = adw::ToolbarView::new();
        for css_class in spec.css_classes {
            root.add_css_class(&css_class);
        }
        if let Some(width_request) = spec.width_request {
            root.set_size_request(width_request, -1);
        }
        Self { root }
    }

    /// Borrow the root toolbar view.
    #[must_use]
    pub fn root(&self) -> &adw::ToolbarView {
        &self.root
    }

    /// Consume the wrapper and return the root toolbar view.
    #[must_use]
    pub fn into_root(self) -> adw::ToolbarView {
        self.root
    }

    /// Set the pane body.
    pub fn set_content(&self, child: &impl IsA<gtk::Widget>) {
        self.root.set_content(Some(child));
    }

    /// Add a top bar, normally a [`BigToolbarHeader`].
    pub fn add_top_bar(&self, child: &impl IsA<gtk::Widget>) {
        self.root.add_top_bar(child);
    }

    /// Add a bottom bar such as transport controls or dialog actions.
    pub fn add_bottom_bar(&self, child: &impl IsA<gtk::Widget>) {
        self.root.add_bottom_bar(child);
    }
}

/// Display-free spec for [`BigToolbarHeader`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigToolbarHeaderSpec {
    /// Optional simple title label. Use [`BigToolbarHeader::with_title_widget`]
    /// when the title is a custom widget such as tabs or a live status label.
    pub title: Option<String>,
    /// Whether libadwaita exposes the title slot.
    pub show_title: bool,
    /// Whether the header shows start-side window title buttons.
    pub show_start_title_buttons: bool,
    /// Whether the header shows end-side window title buttons.
    pub show_end_title_buttons: bool,
    /// CSS classes applied to the header bar.
    pub css_classes: Vec<String>,
    /// CSS classes applied to the simple title label.
    pub title_css_classes: Vec<String>,
}

impl Default for BigToolbarHeaderSpec {
    fn default() -> Self {
        Self {
            title: None,
            show_title: true,
            show_start_title_buttons: true,
            show_end_title_buttons: true,
            css_classes: Vec::new(),
            title_css_classes: Vec::new(),
        }
    }
}

impl BigToolbarHeaderSpec {
    /// Create an empty header spec for callers that provide a custom title
    /// widget or only need action slots.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a header spec with a simple title label.
    #[must_use]
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::default()
        }
    }

    /// Set whether libadwaita exposes the title slot.
    #[must_use]
    pub fn show_title(mut self, show_title: bool) -> Self {
        self.show_title = show_title;
        self
    }

    /// Set whether start-side window title buttons are visible.
    #[must_use]
    pub fn show_start_title_buttons(mut self, show_start_title_buttons: bool) -> Self {
        self.show_start_title_buttons = show_start_title_buttons;
        self
    }

    /// Set whether end-side window title buttons are visible.
    #[must_use]
    pub fn show_end_title_buttons(mut self, show_end_title_buttons: bool) -> Self {
        self.show_end_title_buttons = show_end_title_buttons;
        self
    }

    /// Add one CSS class to the header bar.
    #[must_use]
    pub fn css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_classes.push(css_class.into());
        self
    }

    /// Add one CSS class to the simple title label.
    #[must_use]
    pub fn title_css_class(mut self, css_class: impl Into<String>) -> Self {
        self.title_css_classes.push(css_class.into());
        self
    }
}

/// Shared `AdwHeaderBar` wrapper for pane title/action rows.
#[derive(Debug, Clone)]
pub struct BigToolbarHeader {
    root: adw::HeaderBar,
    title_label: Option<gtk::Label>,
}

impl BigToolbarHeader {
    /// Build a header from a spec, creating a title label when
    /// [`BigToolbarHeaderSpec::title`] is set.
    #[must_use]
    pub fn new(spec: BigToolbarHeaderSpec) -> Self {
        let title_label = spec.title.as_ref().map(|title| {
            let label = gtk::Label::builder()
                .label(title)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            for css_class in &spec.title_css_classes {
                label.add_css_class(css_class);
            }
            label
        });
        let root = build_header_root(&spec, title_label.as_ref().map(|label| label.upcast_ref()));
        Self { root, title_label }
    }

    /// Build a header around an app-owned title widget.
    #[must_use]
    pub fn with_title_widget(
        spec: BigToolbarHeaderSpec,
        title_widget: &impl IsA<gtk::Widget>,
    ) -> Self {
        let root = build_header_root(&spec, Some(title_widget.upcast_ref()));
        Self {
            root,
            title_label: None,
        }
    }

    /// Borrow the root header bar.
    #[must_use]
    pub fn root(&self) -> &adw::HeaderBar {
        &self.root
    }

    /// Borrow the simple title label when this header created one.
    #[must_use]
    pub fn title_label(&self) -> Option<&gtk::Label> {
        self.title_label.as_ref()
    }

    /// Consume the wrapper and return the root header bar.
    #[must_use]
    pub fn into_root(self) -> adw::HeaderBar {
        self.root
    }

    /// Add a widget to the start slot.
    pub fn pack_start(&self, widget: &impl IsA<gtk::Widget>) {
        self.root.pack_start(widget);
    }

    /// Add a widget to the end slot.
    pub fn pack_end(&self, widget: &impl IsA<gtk::Widget>) {
        self.root.pack_end(widget);
    }
}

fn build_header_root(
    spec: &BigToolbarHeaderSpec,
    title_widget: Option<&gtk::Widget>,
) -> adw::HeaderBar {
    let root = adw::HeaderBar::builder()
        .show_title(spec.show_title)
        .show_start_title_buttons(spec.show_start_title_buttons)
        .show_end_title_buttons(spec.show_end_title_buttons)
        .build();
    if let Some(title_widget) = title_widget {
        root.set_title_widget(Some(title_widget));
    }
    for css_class in &spec.css_classes {
        root.add_css_class(css_class);
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_spec_records_css_and_width() {
        let spec = BigToolbarPaneSpec::new()
            .css_class("sidebar-pane")
            .width_request(360);

        assert_eq!(spec.css_classes, ["sidebar-pane"]);
        assert_eq!(spec.width_request, Some(360));
    }

    #[test]
    fn header_spec_defaults_to_standard_window_buttons() {
        let spec = BigToolbarHeaderSpec::new();

        assert_eq!(spec.title, None);
        assert!(spec.show_title);
        assert!(spec.show_start_title_buttons);
        assert!(spec.show_end_title_buttons);
        assert!(spec.css_classes.is_empty());
    }

    #[test]
    fn titled_header_spec_records_title_policy_and_css() {
        let spec = BigToolbarHeaderSpec::titled("Progress")
            .show_start_title_buttons(false)
            .show_end_title_buttons(true)
            .css_class("flat")
            .title_css_class("heading");

        assert_eq!(spec.title.as_deref(), Some("Progress"));
        assert!(!spec.show_start_title_buttons);
        assert!(spec.show_end_title_buttons);
        assert_eq!(spec.css_classes, ["flat"]);
        assert_eq!(spec.title_css_classes, ["heading"]);
    }
}
