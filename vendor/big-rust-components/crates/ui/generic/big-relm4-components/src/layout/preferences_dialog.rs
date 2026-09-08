// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Modal preferences-style dialog scaffold.
//!
//! Composes `adw::Window` + `adw::ToolbarView` + `adw::HeaderBar` +
//! `adw::PreferencesPage` inside a `gtk::ScrolledWindow`, plus a bottom
//! `gtk::ActionBar` for explicit Cancel/Save buttons.
//!
//! This is the established BigLinux form-dialog archetype: a modal
//! libadwaita window that gathers user input across one or more
//! `PreferencesGroup`s and commits on an explicit primary action. Use
//! `BigDialogFooter` instead when the footer should hug the right edge
//! (compact dialogs without a full ActionBar).

use adw::prelude::*;
use relm4::gtk;

const DEFAULT_WIDTH: i32 = 600;
const DEFAULT_HEIGHT: i32 = 440;

/// Build-time configuration for [`BigPreferencesDialog`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPreferencesDialogSpec {
    /// Title.
    pub title: String,
    /// Default width.
    pub default_width: i32,
    /// Default height.
    pub default_height: i32,
    /// Modal.
    pub modal: bool,
}

impl BigPreferencesDialogSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            default_width: DEFAULT_WIDTH,
            default_height: DEFAULT_HEIGHT,
            modal: true,
        }
    }

    /// Configure the `size` setting and return the updated builder.
    ///
    /// The supplied `width` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigPreferencesDialogSpec`].
    #[must_use]
    pub fn size(mut self, width: i32, height: i32) -> Self {
        self.default_width = width;
        self.default_height = height;
        self
    }

    /// Configure the `non_modal` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigPreferencesDialogSpec`].
    #[must_use]
    pub fn non_modal(mut self) -> Self {
        self.modal = false;
        self
    }
}

/// Built scaffold with handles to the parts callers commonly need.
#[derive(Debug, Clone)]
pub struct BigPreferencesDialog {
    // bigagents: app-local-window-owner — this handle IS the window's owner
    // (builds, presents, and exposes it via `window()`); it holds no signal
    // handlers that capture itself, so there is no window ⇄ handler cycle.
    // Consumers must capture the window weakly in *their* handlers.
    window: adw::Window,
    header: adw::HeaderBar,
    page: adw::PreferencesPage,
    action_bar: gtk::ActionBar,
}

impl BigPreferencesDialog {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: &BigPreferencesDialogSpec) -> Self {
        let window = adw::Window::builder()
            .title(spec.title.as_str())
            .modal(spec.modal)
            .default_width(spec.default_width)
            .default_height(spec.default_height)
            .build();

        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        toolbar.add_top_bar(&header);

        let page = adw::PreferencesPage::new();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&page)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        let action_bar = gtk::ActionBar::new();

        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.append(&scroller);
        body.append(&action_bar);
        toolbar.set_content(Some(&body));
        window.set_content(Some(&toolbar));

        Self {
            window,
            header,
            page,
            action_bar,
        }
    }

    /// Return a reference to the `window` exposed by this [`BigPreferencesDialog`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn window(&self) -> &adw::Window {
        &self.window
    }

    /// Return a reference to the `header` exposed by this [`BigPreferencesDialog`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn header(&self) -> &adw::HeaderBar {
        &self.header
    }

    /// Return a reference to the `page` exposed by this [`BigPreferencesDialog`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn page(&self) -> &adw::PreferencesPage {
        &self.page
    }

    /// Return a reference to the `action bar` exposed by this [`BigPreferencesDialog`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn action_bar(&self) -> &gtk::ActionBar {
        &self.action_bar
    }

    /// Append `group` to the dialog's preferences page.
    pub fn add_group(&self, group: &adw::PreferencesGroup) {
        self.page.add(group);
    }

    /// Append a Cancel/Save pair to the ActionBar.
    ///
    /// Cancel is packed at the start; Save is packed at the end and
    /// gets the `suggested-action` CSS class. Callers wire signals on
    /// the returned buttons.
    pub fn add_footer_cancel_save(
        &self,
        cancel_label: impl Into<String>,
        save_label: impl Into<String>,
    ) -> (gtk::Button, gtk::Button) {
        let cancel = gtk::Button::builder()
            .label(cancel_label.into())
            .valign(gtk::Align::Center)
            .build();
        let save = gtk::Button::builder()
            .label(save_label.into())
            .valign(gtk::Align::Center)
            .css_classes(["suggested-action"])
            .build();
        self.action_bar.pack_start(&cancel);
        self.action_bar.pack_end(&save);
        (cancel, save)
    }

    /// Set `transient_for` and `present` the window in one call.
    pub fn present(&self, parent: &impl gtk::prelude::IsA<gtk::Window>) {
        self.window.set_transient_for(Some(parent));
        self.window.present();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_match_biglinux_archetype() {
        let spec = BigPreferencesDialogSpec::new("Preferences");
        assert_eq!(spec.title, "Preferences");
        assert_eq!(spec.default_width, DEFAULT_WIDTH);
        assert_eq!(spec.default_height, DEFAULT_HEIGHT);
        assert!(spec.modal);
    }

    #[test]
    fn spec_size_overrides_defaults() {
        let spec = BigPreferencesDialogSpec::new("Edit").size(800, 600);
        assert_eq!(spec.default_width, 800);
        assert_eq!(spec.default_height, 600);
    }

    #[test]
    fn spec_non_modal_clears_flag() {
        let spec = BigPreferencesDialogSpec::new("Edit").non_modal();
        assert!(!spec.modal);
    }
}
