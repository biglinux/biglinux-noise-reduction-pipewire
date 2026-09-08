// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Dialog workflow contracts (display-free specs).
//!
//! The libadwaita `AlertDialog` builders that render these specs live in
//! `big_app_kit::dialogs`.

/// Dialog archetype picked when building a [`BigDialogSpec`]; controls
/// the severity, button set, and default close behaviour of the
/// rendered `AdwAlertDialog`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigDialogKind {
    /// Yes/no confirmation for a non-destructive action.
    Confirm,
    /// Error report with optional copyable diagnostic.
    Error,
    /// Info dialog whose body the user is expected to copy.
    CopyableInfo,
    /// Settings/preferences sheet.
    Preferences,
    /// Import or export flow with file picker affordances.
    ImportExport,
    /// "Discard changes?" prompt shown before closing dirty documents.
    UnsavedChanges,
    /// "Overwrite existing file?" prompt; defaults to destructive.
    Overwrite,
}

/// Typed dialog spec — Confirm, Error, CopyableInfo, Preferences, ImportExport,
/// UnsavedChanges, Overwrite. Drives libadwaita `AlertDialog`.
///
/// # Capabilities
///
/// `dialog`, `dialog+destructive`, `dialog+copyable`
///
/// # Archetypes
///
/// `terminal`, `editor`, `media-player`, `media-converter`, `file-manager`, `pkg-manager`, `control-center`
///
/// # Examples
///
/// ```
/// use big_app_kit_core::dialogs::{BigDialogKind, BigDialogSpec};
///
/// let confirm = BigDialogSpec::new(BigDialogKind::Confirm, "Delete file?")
///     .body("This cannot be undone.")
///     .primary_action("Delete")
///     .destructive(true);
/// assert!(confirm.destructive);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDialogSpec {
    /// Dialog archetype (info, confirm, error, etc.).
    pub kind: BigDialogKind,
    /// Headline text rendered as the dialog title.
    pub title: String,
    /// Optional secondary text under the title.
    pub body: Option<String>,
    /// Label of the primary action button (e.g. `Delete`, `Save`).
    pub primary_action: Option<String>,
    /// Flags the primary action as destructive so libadwaita renders it
    /// in the warning style; set `true` whenever a click discards data
    /// the user cannot recover from in-app.
    pub destructive: bool,
    /// Exposes a Cancel/Close button; set `false` only for
    /// acknowledge-only error dialogs where dismissal must be explicit.
    pub cancellable: bool,
    /// Optional copy-to-clipboard details block (e.g. stack traces).
    pub copyable_details: Option<String>,
}

impl BigDialogSpec {
    /// Build a spec for `kind` with default flags (cancellable,
    /// non-destructive, no body, no copyable detail).
    #[must_use]
    pub fn new(kind: BigDialogKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            body: None,
            primary_action: None,
            destructive: false,
            cancellable: true,
            copyable_details: None,
        }
    }

    /// Shortcut for a confirm dialog with a pre-set primary button.
    #[must_use]
    pub fn confirm(title: impl Into<String>, primary_action: impl Into<String>) -> Self {
        Self::new(BigDialogKind::Confirm, title).primary_action(primary_action)
    }

    /// Shortcut for a plain error dialog with no copyable diagnostic.
    #[must_use]
    pub fn error(title: impl Into<String>) -> Self {
        Self::new(BigDialogKind::Error, title)
    }

    /// Error dialog with an attached copyable diagnostic block
    /// (typically stderr or a backtrace).
    #[must_use]
    pub fn error_with_details(title: impl Into<String>, details: impl Into<String>) -> Self {
        Self::error(title).copyable_details(details)
    }

    /// "Discard unsaved changes?" spec; pre-marked destructive so the
    /// primary button renders in red.
    #[must_use]
    pub fn unsaved_changes(title: impl Into<String>) -> Self {
        Self::new(BigDialogKind::UnsavedChanges, title).destructive(true)
    }

    /// "Overwrite existing file?" spec; pre-marked destructive.
    #[must_use]
    pub fn overwrite(title: impl Into<String>) -> Self {
        Self::new(BigDialogKind::Overwrite, title).destructive(true)
    }

    /// Attach a secondary line of body text under the headline.
    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set the label of the primary action button (`"Delete"`,
    /// `"Save"`).
    #[must_use]
    pub fn primary_action(mut self, label: impl Into<String>) -> Self {
        self.primary_action = Some(label.into());
        self
    }

    /// Mark the primary action as destructive (red styling, no
    /// default focus).
    #[must_use]
    pub fn destructive(mut self, enabled: bool) -> Self {
        self.destructive = enabled;
        self
    }

    /// Attach a diagnostic block the dialog should expose via a "Copy"
    /// button. Implies `requires_copy_button` in the resolved form.
    #[must_use]
    pub fn copyable_details(mut self, details: impl Into<String>) -> Self {
        self.copyable_details = Some(details.into());
        self
    }

    /// Project the spec into a [`BigDialogResolved`] that an
    /// `AdwAlertDialog` adapter consumes directly.
    #[must_use]
    pub fn resolved(&self) -> BigDialogResolved {
        BigDialogResolved {
            requires_copy_button: self.copyable_details.is_some()
                || self.kind == BigDialogKind::CopyableInfo,
            requires_cancel: self.cancellable,
            severity: match self.kind {
                BigDialogKind::Error => "error",
                BigDialogKind::UnsavedChanges | BigDialogKind::Overwrite if self.destructive => {
                    "warning"
                }
                BigDialogKind::Confirm
                | BigDialogKind::Overwrite
                | BigDialogKind::UnsavedChanges => "confirm",
                BigDialogKind::CopyableInfo
                | BigDialogKind::Preferences
                | BigDialogKind::ImportExport => "info",
            },
        }
    }
}

/// Persist a positive dialog size through a caller-owned storage closure.
///
/// Returns `false` when GTK has not reported a usable size yet. The helper is
/// deliberately storage-agnostic: callers own settings keys, debounce policy,
/// and error handling.
pub fn persist_dialog_size(width: i32, height: i32, persist: impl FnOnce(i32, i32)) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    persist(width, height);
    true
}

/// Pre-computed flags telling an `AdwAlertDialog` adapter how to
/// render a [`BigDialogSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDialogResolved {
    /// `true` when the dialog must expose a "Copy" button for its
    /// diagnostic block.
    pub requires_copy_button: bool,
    /// `true` when the dialog must expose a Cancel/Close button.
    pub requires_cancel: bool,
    /// One of `"error"`, `"warning"`, `"confirm"`, `"info"`; used
    /// for CSS class selection.
    pub severity: &'static str,
}

/// Visual tone for a [`BigEntryValidation`] message in a validated entry dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigEntryValidationTone {
    /// No success/error emphasis.
    Neutral,
    /// Positive confirmation; typically styled with the app's `success` class.
    Success,
    /// Blocking validation issue; typically styled with the app's `error` class.
    Error,
}

/// Validation state for a single-entry action dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEntryValidation {
    /// Whether the dialog action response should be enabled.
    pub is_valid: bool,
    /// Message shown below the entry. Empty messages keep the label neutral.
    pub message: String,
    /// Visual tone applied to the validation message.
    pub tone: BigEntryValidationTone,
}

impl BigEntryValidation {
    /// Block submission without showing a message.
    #[must_use]
    pub fn pending() -> Self {
        Self {
            is_valid: false,
            message: String::new(),
            tone: BigEntryValidationTone::Neutral,
        }
    }

    /// Allow submission and show a success message.
    #[must_use]
    pub fn valid(message: impl Into<String>) -> Self {
        Self {
            is_valid: true,
            message: message.into(),
            tone: BigEntryValidationTone::Success,
        }
    }

    /// Block submission and show an error message.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            is_valid: false,
            message: message.into(),
            tone: BigEntryValidationTone::Error,
        }
    }
}

/// Text contract for a single-entry dialog with live validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigValidatedEntryDialogSpec {
    /// Dialog heading.
    pub heading: String,
    /// Dialog body text.
    pub body: String,
    /// `AdwEntryRow` title.
    pub entry_title: String,
    /// Label for the cancel response.
    pub cancel_label: String,
    /// Stable response id for the action button.
    pub action_id: String,
    /// Label for the action button.
    pub action_label: String,
    /// Whether the action button should use destructive styling.
    pub destructive: bool,
}

impl BigValidatedEntryDialogSpec {
    /// Build a non-destructive validated entry dialog spec.
    #[must_use]
    pub fn new(
        heading: impl Into<String>,
        body: impl Into<String>,
        entry_title: impl Into<String>,
        cancel_label: impl Into<String>,
        action_id: impl Into<String>,
        action_label: impl Into<String>,
    ) -> Self {
        Self {
            heading: heading.into(),
            body: body.into(),
            entry_title: entry_title.into(),
            cancel_label: cancel_label.into(),
            action_id: action_id.into(),
            action_label: action_label.into(),
            destructive: false,
        }
    }

    /// Mark the action button as destructive.
    #[must_use]
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_dialog_with_details_requires_copy_button() {
        let resolved = BigDialogSpec::error_with_details("Failed", "stderr").resolved();
        assert!(resolved.requires_copy_button);
        assert_eq!(resolved.severity, "error");
    }

    #[test]
    fn destructive_overwrite_is_warning() {
        let resolved = BigDialogSpec::new(BigDialogKind::Overwrite, "Overwrite?")
            .destructive(true)
            .resolved();
        assert_eq!(resolved.severity, "warning");
    }

    #[test]
    fn copyable_message_dialog_requires_copy_button_without_details() {
        let resolved = BigDialogSpec::new(BigDialogKind::CopyableInfo, "Command output").resolved();

        assert!(resolved.requires_copy_button);
        assert_eq!(resolved.severity, "info");
    }

    #[test]
    fn non_destructive_overwrite_and_unsaved_changes_are_confirmations() {
        let overwrite = BigDialogSpec::new(BigDialogKind::Overwrite, "Overwrite?").resolved();
        let unsaved = BigDialogSpec::new(BigDialogKind::UnsavedChanges, "Discard?").resolved();

        assert_eq!(overwrite.severity, "confirm");
        assert_eq!(unsaved.severity, "confirm");
    }

    #[test]
    fn persist_dialog_size_requires_positive_dimensions() {
        let mut persisted_size = None;

        assert!(!persist_dialog_size(0, 480, |width, height| {
            persisted_size = Some((width, height));
        }));
        assert_eq!(persisted_size, None);

        assert!(!persist_dialog_size(640, -1, |width, height| {
            persisted_size = Some((width, height));
        }));
        assert_eq!(persisted_size, None);

        assert!(persist_dialog_size(640, 480, |width, height| {
            persisted_size = Some((width, height));
        }));
        assert_eq!(persisted_size, Some((640, 480)));
    }

    #[test]
    fn validated_entry_spec_records_action_contract() {
        let spec = BigValidatedEntryDialogSpec::new(
            "New command",
            "Enter a command",
            "Command",
            "Cancel",
            "create",
            "Create",
        )
        .destructive(true);

        assert_eq!(spec.heading, "New command");
        assert_eq!(spec.body, "Enter a command");
        assert_eq!(spec.entry_title, "Command");
        assert_eq!(spec.cancel_label, "Cancel");
        assert_eq!(spec.action_id, "create");
        assert_eq!(spec.action_label, "Create");
        assert!(spec.destructive);
    }

    #[test]
    fn entry_validation_constructors_encode_submit_state() {
        let pending = BigEntryValidation::pending();
        assert!(!pending.is_valid);
        assert_eq!(pending.message, "");
        assert_eq!(pending.tone, BigEntryValidationTone::Neutral);

        let valid = BigEntryValidation::valid("Valid name");
        assert!(valid.is_valid);
        assert_eq!(valid.message, "Valid name");
        assert_eq!(valid.tone, BigEntryValidationTone::Success);

        let invalid = BigEntryValidation::invalid("Already exists");
        assert!(!invalid.is_valid);
        assert_eq!(invalid.message, "Already exists");
        assert_eq!(invalid.tone, BigEntryValidationTone::Error);
    }
}
