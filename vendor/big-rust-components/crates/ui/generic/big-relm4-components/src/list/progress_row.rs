// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Compact progress rows for queues and background tasks.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

const DEFAULT_WIDTH_CHARS: i32 = 4;
const DEFAULT_BAR_WIDTH: i32 = 160;
const ROW_SPACING: i32 = 8;

/// Optional trailing action for a progress row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigProgressRowActionSpec {
    /// Icon name.
    pub icon_name: String,
    /// Label.
    pub label: String,
}

impl BigProgressRowActionSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(icon_name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            icon_name: icon_name.into(),
            label: label.into(),
        }
    }
}

/// Current progress-row state.
#[derive(Debug, Clone, PartialEq)]
pub struct BigProgressRowSpec {
    /// Title.
    pub title: String,
    /// Fraction.
    pub fraction: Option<f64>,
    /// Status text.
    pub status_text: String,
    /// Action.
    pub action: Option<BigProgressRowActionSpec>,
}

impl BigProgressRowSpec {
    /// Construct a [`BigProgressRowSpec`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn determinate(
        title: impl Into<String>,
        fraction: f64,
        status_text: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            fraction: Some(fraction.clamp(0.0, 1.0)),
            status_text: status_text.into(),
            action: None,
        }
    }

    /// Construct a [`BigProgressRowSpec`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn indeterminate(title: impl Into<String>, status_text: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            fraction: None,
            status_text: status_text.into(),
            action: None,
        }
    }

    /// Builder: sets action.
    #[must_use]
    pub fn with_action(mut self, action: BigProgressRowActionSpec) -> Self {
        self.action = Some(action);
        self
    }
}

/// Built row widgets. Apps wire action callbacks.
#[derive(Debug, Clone)]
pub struct BigProgressRow {
    root: gtk::Box,
    title: gtk::Label,
    bar: gtk::ProgressBar,
    status: gtk::Label,
    action_button: Option<gtk::Button>,
}

impl BigProgressRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: &BigProgressRowSpec) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, ROW_SPACING);

        let title = gtk::Label::new(Some(&spec.title));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        title.add_css_class("caption-heading");

        let bar = gtk::ProgressBar::new();
        bar.set_hexpand(true);
        bar.set_valign(gtk::Align::Center);
        bar.set_width_request(DEFAULT_BAR_WIDTH);

        let status = gtk::Label::new(Some(&spec.status_text));
        status.add_css_class("dim-label");
        status.set_width_chars(DEFAULT_WIDTH_CHARS);

        root.append(&title);
        root.append(&bar);
        root.append(&status);

        let action_button = spec.action.as_ref().map(|action| {
            let button = tooltip::icon_button(&action.icon_name, &action.label, &["flat"]);
            root.append(&button);
            button
        });

        let row = Self {
            root,
            title,
            bar,
            status,
            action_button,
        };
        row.update(spec);
        row
    }

    /// Return a reference to the `root` exposed by this [`BigProgressRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return the current `action button` value held by this [`BigProgressRow`].
    #[must_use]
    pub fn action_button(&self) -> Option<&gtk::Button> {
        self.action_button.as_ref()
    }

    /// Re-render the row from `spec`: title, progress fraction, and
    /// status text. Indeterminate fractions trigger pulsing.
    pub fn update(&self, spec: &BigProgressRowSpec) {
        if self.title.text() != spec.title.as_str() {
            self.title.set_text(&spec.title);
        }
        match spec.fraction {
            Some(fraction) => self.bar.set_fraction(fraction.clamp(0.0, 1.0)),
            None => {
                self.bar.set_fraction(0.0);
                self.bar.pulse();
            }
        }
        if self.status.text() != spec.status_text.as_str() {
            self.status.set_text(&spec.status_text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determinate_spec_clamps_fraction() {
        assert_eq!(
            BigProgressRowSpec::determinate("Copy", 2.5, "100%").fraction,
            Some(1.0)
        );
        assert_eq!(
            BigProgressRowSpec::determinate("Copy", -1.0, "0%").fraction,
            Some(0.0)
        );
    }

    #[test]
    fn action_spec_keeps_label_and_icon() {
        let spec = BigProgressRowSpec::indeterminate("Copy", "...").with_action(
            BigProgressRowActionSpec::new("process-stop-symbolic", "Cancel"),
        );
        assert_eq!(
            spec.action.as_ref().map(|action| action.label.as_str()),
            Some("Cancel")
        );
    }
}
