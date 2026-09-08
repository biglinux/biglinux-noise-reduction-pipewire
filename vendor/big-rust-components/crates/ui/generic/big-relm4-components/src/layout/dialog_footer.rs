// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Standard dialog action footer.

use adw::prelude::*;
use relm4::gtk;

const DEFAULT_SPACING: i32 = 6;
const DEFAULT_MARGIN: i32 = 12;

/// Labels and style for a two-action dialog footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDialogFooterSpec {
    /// Cancel label.
    pub cancel_label: String,
    /// Accept label.
    pub accept_label: String,
    /// Accept css classes.
    pub accept_css_classes: Vec<String>,
}

impl BigDialogFooterSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(cancel_label: impl Into<String>, accept_label: impl Into<String>) -> Self {
        Self {
            cancel_label: cancel_label.into(),
            accept_label: accept_label.into(),
            accept_css_classes: vec!["suggested-action".into()],
        }
    }

    /// Builder: sets accept css classes.
    #[must_use]
    pub fn with_accept_css_classes(mut self, css_classes: &[&str]) -> Self {
        self.accept_css_classes = css_classes
            .iter()
            .map(|class| (*class).to_owned())
            .collect();
        self
    }
}

/// Built footer and action buttons.
#[derive(Debug, Clone)]
pub struct BigDialogFooter {
    root: gtk::Box,
    cancel: gtk::Button,
    accept: gtk::Button,
}

impl BigDialogFooter {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: &BigDialogFooterSpec) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, DEFAULT_SPACING);
        root.set_halign(gtk::Align::End);
        root.set_margin_start(DEFAULT_MARGIN);
        root.set_margin_end(DEFAULT_MARGIN);
        root.set_margin_top(DEFAULT_MARGIN);
        root.set_margin_bottom(DEFAULT_MARGIN);

        let cancel = gtk::Button::with_label(&spec.cancel_label);
        let accept = gtk::Button::builder()
            .label(&spec.accept_label)
            .css_classes(spec.accept_css_classes.clone())
            .build();

        root.append(&cancel);
        root.append(&accept);

        Self {
            root,
            cancel,
            accept,
        }
    }

    /// Return a reference to the `root` exposed by this [`BigDialogFooter`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `cancel button` exposed by this [`BigDialogFooter`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn cancel_button(&self) -> &gtk::Button {
        &self.cancel
    }

    /// Return a reference to the `accept button` exposed by this [`BigDialogFooter`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn accept_button(&self) -> &gtk::Button {
        &self.accept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_to_suggested_accept_action() {
        let spec = BigDialogFooterSpec::new("Cancel", "Save");
        assert_eq!(spec.accept_css_classes, ["suggested-action"]);
    }

    #[test]
    fn spec_can_override_accept_classes() {
        let spec = BigDialogFooterSpec::new("Cancel", "Delete")
            .with_accept_css_classes(&["destructive-action"]);
        assert_eq!(spec.accept_css_classes, ["destructive-action"]);
    }
}
