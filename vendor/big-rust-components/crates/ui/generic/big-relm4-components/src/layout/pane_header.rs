// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared split-pane header chrome.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

const DEFAULT_MOVE_ICON: &str = "select-rectangular-symbolic";
const DEFAULT_CLOSE_ICON: &str = "window-close-symbolic";

/// Display-free spec for [`BigPaneHeader`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPaneHeaderSpec {
    /// Current pane title.
    pub title: String,
    /// Optional symbolic icon name shown before the title.
    pub icon_name: Option<String>,
    /// Label and tooltip for the move button.
    pub move_label: String,
    /// Label and tooltip for the close button.
    pub close_label: String,
    /// Symbolic icon name for the move button.
    pub move_icon_name: String,
    /// Symbolic icon name for the close button.
    pub close_icon_name: String,
    /// CSS classes applied to the header row.
    pub header_css_classes: Vec<String>,
    /// CSS classes applied to move/close buttons.
    pub button_css_classes: Vec<String>,
    /// Initial `GtkWindowHandle` visibility.
    pub is_visible: bool,
}

impl BigPaneHeaderSpec {
    /// Create a pane header spec with required labels.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        move_label: impl Into<String>,
        close_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            icon_name: None,
            move_label: move_label.into(),
            close_label: close_label.into(),
            move_icon_name: DEFAULT_MOVE_ICON.to_owned(),
            close_icon_name: DEFAULT_CLOSE_ICON.to_owned(),
            header_css_classes: vec!["header-bar".to_owned(), "terminal-pane-header".to_owned()],
            button_css_classes: vec!["flat".to_owned()],
            is_visible: false,
        }
    }

    /// Set the leading symbolic icon.
    #[must_use]
    pub fn icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    /// Replace header CSS classes.
    #[must_use]
    pub fn header_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.header_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Replace button CSS classes.
    #[must_use]
    pub fn button_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.button_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Set initial header visibility.
    #[must_use]
    pub fn visible(mut self, is_visible: bool) -> Self {
        self.is_visible = is_visible;
        self
    }
}

/// Built pane header plus app-wired action buttons.
#[derive(Debug, Clone)]
pub struct BigPaneHeader {
    root: gtk::WindowHandle,
    header: gtk::Box,
    icon: Option<gtk::Image>,
    title_label: gtk::Label,
    move_button: gtk::Button,
    close_button: gtk::Button,
}

impl BigPaneHeader {
    /// Build pane header chrome from a semantic spec.
    #[must_use]
    pub fn new(spec: BigPaneHeaderSpec) -> Self {
        Self::build(spec, None)
    }

    /// Build pane header chrome with a required leading icon.
    #[must_use]
    pub fn new_with_icon(
        mut spec: BigPaneHeaderSpec,
        icon_name: impl Into<String>,
    ) -> (Self, gtk::Image) {
        let icon_name = icon_name.into();
        let icon = gtk::Image::from_icon_name(&icon_name);
        spec.icon_name = Some(icon_name);

        (Self::build(spec, Some(icon.clone())), icon)
    }

    fn build(spec: BigPaneHeaderSpec, leading_icon: Option<gtk::Image>) -> Self {
        let title_label = gtk::Label::builder()
            .label(&spec.title)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .xalign(0.0)
            .hexpand(true)
            .halign(gtk::Align::Start)
            .build();

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .hexpand(true)
            .valign(gtk::Align::Start)
            .build();
        for css_class in &spec.header_css_classes {
            header.add_css_class(css_class);
        }

        let icon =
            leading_icon.or_else(|| spec.icon_name.as_deref().map(gtk::Image::from_icon_name));
        if let Some(icon) = &icon {
            header.append(icon);
        }
        header.append(&title_label);

        let button_classes: Vec<&str> =
            spec.button_css_classes.iter().map(String::as_str).collect();
        let move_button =
            tooltip::icon_button(&spec.move_icon_name, &spec.move_label, &button_classes);
        let close_button =
            tooltip::icon_button(&spec.close_icon_name, &spec.close_label, &button_classes);
        header.append(&move_button);
        header.append(&close_button);

        let root = gtk::WindowHandle::builder()
            .visible(spec.is_visible)
            .child(&header)
            .build();

        Self {
            root,
            header,
            icon,
            title_label,
            move_button,
            close_button,
        }
    }

    /// Return the `GtkWindowHandle` root.
    #[must_use]
    pub fn root(&self) -> &gtk::WindowHandle {
        &self.root
    }

    /// Return the inner header row.
    #[must_use]
    pub fn header(&self) -> &gtk::Box {
        &self.header
    }

    /// Return the optional leading icon.
    #[must_use]
    pub fn icon(&self) -> Option<&gtk::Image> {
        self.icon.as_ref()
    }

    /// Return the title label.
    #[must_use]
    pub fn title_label(&self) -> &gtk::Label {
        &self.title_label
    }

    /// Return the move button.
    #[must_use]
    pub fn move_button(&self) -> &gtk::Button {
        &self.move_button
    }

    /// Return the close button.
    #[must_use]
    pub fn close_button(&self) -> &gtk::Button {
        &self.close_button
    }

    /// Consume `self` and return all app-wired parts.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        gtk::WindowHandle,
        gtk::Box,
        Option<gtk::Image>,
        gtk::Label,
        gtk::Button,
        gtk::Button,
    ) {
        (
            self.root,
            self.header,
            self.icon,
            self.title_label,
            self.move_button,
            self.close_button,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_header_spec_defaults_to_terminal_chrome() {
        let spec = BigPaneHeaderSpec::new("Terminal", "Move", "Close");

        assert_eq!(spec.title, "Terminal");
        assert_eq!(spec.move_icon_name, DEFAULT_MOVE_ICON);
        assert_eq!(spec.close_icon_name, DEFAULT_CLOSE_ICON);
        assert_eq!(
            spec.header_css_classes,
            ["header-bar", "terminal-pane-header"]
        );
        assert_eq!(spec.button_css_classes, ["flat"]);
        assert!(!spec.is_visible);
    }

    #[test]
    fn pane_header_spec_records_icon_classes_and_visibility() {
        let spec = BigPaneHeaderSpec::new("Editor", "Move", "Close")
            .icon_name("text-x-generic-symbolic")
            .header_css_classes(["pane-header"])
            .button_css_classes(["flat", "circular"])
            .visible(true);

        assert_eq!(spec.icon_name.as_deref(), Some("text-x-generic-symbolic"));
        assert_eq!(spec.header_css_classes, ["pane-header"]);
        assert_eq!(spec.button_css_classes, ["flat", "circular"]);
        assert!(spec.is_visible);
    }
}
