// SPDX-License-Identifier: MIT

//! Inline status row with spinner, text, and optional cancel action.

use relm4::gtk;
use relm4::gtk::prelude::*;

const DEFAULT_SPACING: i32 = 8;
const DEFAULT_VERTICAL_MARGIN: i32 = 8;

/// Data used to build an inline loading/status row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigInlineStatusRowSpec {
    /// Initial status text.
    pub status_text: String,
    /// Optional cancel action label.
    pub cancel_label: Option<String>,
    /// Space between controls.
    pub spacing: i32,
    /// Top and bottom margin.
    pub vertical_margin: i32,
    /// Whether the row starts visible.
    pub visible: bool,
}

impl BigInlineStatusRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(status_text: impl Into<String>) -> Self {
        Self {
            status_text: status_text.into(),
            cancel_label: None,
            spacing: DEFAULT_SPACING,
            vertical_margin: DEFAULT_VERTICAL_MARGIN,
            visible: true,
        }
    }

    /// Add a cancel button.
    #[must_use]
    pub fn cancel_label(mut self, cancel_label: impl Into<String>) -> Self {
        self.cancel_label = Some(cancel_label.into());
        self
    }

    /// Configure spacing between controls.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Configure top and bottom margin.
    #[must_use]
    pub fn vertical_margin(mut self, vertical_margin: i32) -> Self {
        self.vertical_margin = vertical_margin;
        self
    }

    /// Configure whether the row starts visible.
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigInlineStatusRowResolved {
        BigInlineStatusRowResolved {
            status_text: self.status_text.clone(),
            cancel_label: self.cancel_label.clone(),
            spacing: self.spacing.max(0),
            vertical_margin: self.vertical_margin.max(0),
            visible: self.visible,
        }
    }
}

/// Pure resolved inline status row contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigInlineStatusRowResolved {
    /// Initial status text.
    pub status_text: String,
    /// Optional cancel action label.
    pub cancel_label: Option<String>,
    /// Space between controls.
    pub spacing: i32,
    /// Top and bottom margin.
    pub vertical_margin: i32,
    /// Whether the row starts visible.
    pub visible: bool,
}

/// Built inline status row.
#[derive(Debug, Clone)]
pub struct BigInlineStatusRow {
    root: gtk::Box,
    spinner: gtk::Spinner,
    status_label: gtk::Label,
    cancel_button: Option<gtk::Button>,
}

impl BigInlineStatusRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigInlineStatusRowSpec) -> Self {
        let resolved = spec.resolved();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(resolved.spacing)
            .halign(gtk::Align::Center)
            .margin_top(resolved.vertical_margin)
            .margin_bottom(resolved.vertical_margin)
            .visible(resolved.visible)
            .build();
        let spinner = gtk::Spinner::new();
        let status_label = gtk::Label::new(Some(resolved.status_text.as_str()));
        root.append(&spinner);
        root.append(&status_label);

        let cancel_button = resolved.cancel_label.map(|label| {
            let button = gtk::Button::builder()
                .label(label.as_str())
                .css_classes(["flat"])
                .build();
            root.append(&button);
            button
        });

        Self {
            root,
            spinner,
            status_label,
            cancel_button,
        }
    }

    /// Return the root row.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return the spinner.
    #[must_use]
    pub fn spinner(&self) -> &gtk::Spinner {
        &self.spinner
    }

    /// Return the status label.
    #[must_use]
    pub fn status_label(&self) -> &gtk::Label {
        &self.status_label
    }

    /// Return the optional cancel button.
    #[must_use]
    pub fn cancel_button(&self) -> Option<&gtk::Button> {
        self.cancel_button.as_ref()
    }

    /// Consume `self` and yield the root row.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_status_row_clamps_layout_values() {
        let resolved = BigInlineStatusRowSpec::new("Loading")
            .cancel_label("Cancel")
            .spacing(-4)
            .vertical_margin(-2)
            .visible(false)
            .resolved();

        assert_eq!(resolved.status_text, "Loading");
        assert_eq!(resolved.cancel_label.as_deref(), Some("Cancel"));
        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.vertical_margin, 0);
        assert!(!resolved.visible);
    }
}
