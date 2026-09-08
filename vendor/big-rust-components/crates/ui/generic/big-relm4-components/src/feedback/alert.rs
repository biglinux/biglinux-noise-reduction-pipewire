// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared libadwaita alert-dialog builders.

use adw::prelude::*;

/// Tone applied to a confirmation response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigConfirmAlertTone {
    /// Ordinary confirmation action.
    Suggested,
    /// Destructive confirmation action.
    Destructive,
}

/// Specification for a cancel/confirm alert dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigConfirmAlertSpec {
    /// Dialog heading.
    pub heading: String,
    /// Dialog body text.
    pub body: String,
    /// Response id for the safe cancel/keep action.
    pub cancel_id: String,
    /// Visible cancel/keep label.
    pub cancel_label: String,
    /// Response id for the confirm action.
    pub confirm_id: String,
    /// Visible confirm label.
    pub confirm_label: String,
    /// Visual tone for the confirm action.
    pub confirm_tone: BigConfirmAlertTone,
}

impl BigConfirmAlertSpec {
    /// Create a non-destructive cancel/confirm alert spec.
    #[must_use]
    pub fn new(
        heading: impl Into<String>,
        body: impl Into<String>,
        cancel_label: impl Into<String>,
        confirm_id: impl Into<String>,
        confirm_label: impl Into<String>,
    ) -> Self {
        Self {
            heading: heading.into(),
            body: body.into(),
            cancel_id: "cancel".to_owned(),
            cancel_label: cancel_label.into(),
            confirm_id: confirm_id.into(),
            confirm_label: confirm_label.into(),
            confirm_tone: BigConfirmAlertTone::Suggested,
        }
    }

    /// Override the safe cancel/keep response id.
    #[must_use]
    pub fn cancel_id(mut self, cancel_id: impl Into<String>) -> Self {
        self.cancel_id = cancel_id.into();
        self
    }

    /// Mark or unmark the confirm response as destructive.
    #[must_use]
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.confirm_tone = if destructive {
            BigConfirmAlertTone::Destructive
        } else {
            BigConfirmAlertTone::Suggested
        };
        self
    }
}

/// Build a libadwaita alert dialog from a shared confirmation spec.
#[must_use]
pub fn build_confirm_alert_dialog(spec: &BigConfirmAlertSpec) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::builder()
        .heading(&spec.heading)
        .body(&spec.body)
        .close_response(&spec.cancel_id)
        .default_response(&spec.cancel_id)
        .build();
    dialog.add_response(&spec.cancel_id, &spec.cancel_label);
    dialog.add_response(&spec.confirm_id, &spec.confirm_label);
    dialog.set_response_appearance(
        &spec.confirm_id,
        match spec.confirm_tone {
            BigConfirmAlertTone::Suggested => adw::ResponseAppearance::Suggested,
            BigConfirmAlertTone::Destructive => adw::ResponseAppearance::Destructive,
        },
    );
    dialog
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_confirm_uses_safe_cancel_id() {
        let spec = BigConfirmAlertSpec::new(
            "Cancel all?",
            "Stop every job",
            "Continue",
            "cancel_all",
            "Cancel all",
        )
        .cancel_id("keep")
        .destructive(true);

        assert_eq!(spec.cancel_id, "keep");
        assert_eq!(spec.confirm_id, "cancel_all");
        assert_eq!(spec.confirm_tone, BigConfirmAlertTone::Destructive);
    }

    #[test]
    fn confirm_alert_defaults_to_suggested_action() {
        let spec = BigConfirmAlertSpec::new("Run?", "Start task", "Cancel", "run", "Run");

        assert_eq!(spec.cancel_id, "cancel");
        assert_eq!(spec.confirm_tone, BigConfirmAlertTone::Suggested);
    }
}
