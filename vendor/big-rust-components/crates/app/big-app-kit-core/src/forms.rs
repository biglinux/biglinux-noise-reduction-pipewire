// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Form and settings page contracts.

/// Widget archetype to render for a [`BigFieldSpec`]; the adapter maps
/// each value to the libadwaita row that fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigFieldKind {
    /// Single-line text entry rendered as `AdwEntryRow`.
    Text,
    /// Masked entry that also flips `must_redact` on in the resolved
    /// form.
    Password,
    /// Filesystem path entry with a "Browse" button.
    Path,
    /// Integer/float spin button row.
    Spin,
    /// On/off switch row.
    Switch,
    /// Drop-down list bound to a fixed set of choices.
    Combo,
    /// Horizontal slider bound to a numeric range.
    Slider,
    /// Color picker row.
    Color,
    /// Font picker row.
    Font,
}

/// Severity tag attached to a validation result; drives the badge
/// colour in the validation banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigValidationSeverity {
    /// Hint, neutral styling.
    Info,
    /// Yellow styling; the form can still be submitted.
    Warning,
    /// Red styling; submission is blocked.
    Error,
}

/// Display-free description of a single form field.
///
/// Apps build a list of these per settings page and feed them into the
/// adapter, which then realises each row and wires it to a GSettings
/// key named `settings.<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFieldSpec {
    /// Stable identifier used in [`BigFieldResolved::action_name`] and
    /// as the GSettings key suffix.
    pub id: String,
    /// Localized label shown next to the row.
    pub label: String,
    /// Widget archetype to render.
    pub kind: BigFieldKind,
    /// `true` when the field must have a non-empty value before the
    /// form can be submitted.
    pub required: bool,
    /// `true` for fields containing secrets; implies redacted display
    /// and a redacted log surface.
    pub sensitive: bool,
    /// Optional tooltip shown on hover.
    pub tooltip: Option<String>,
}

impl BigFieldSpec {
    /// Build a generic field spec without flags. Prefer the
    /// kind-specific helpers (`text_row`, `password_row`, ...).
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>, kind: BigFieldKind) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind,
            required: false,
            sensitive: false,
            tooltip: None,
        }
    }

    /// Shortcut: text entry row.
    #[must_use]
    pub fn text_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, BigFieldKind::Text)
    }

    /// Shortcut: password row; pre-flags the spec as sensitive so logs
    /// and exports redact it.
    #[must_use]
    pub fn password_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, BigFieldKind::Password).sensitive(true)
    }

    /// Shortcut: filesystem path row with picker button.
    #[must_use]
    pub fn path_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, BigFieldKind::Path)
    }

    /// Shortcut: numeric spin button row.
    #[must_use]
    pub fn spin_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, BigFieldKind::Spin)
    }

    /// Shortcut: on/off switch row.
    #[must_use]
    pub fn switch_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, BigFieldKind::Switch)
    }

    /// Shortcut: drop-down row.
    #[must_use]
    pub fn combo_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, BigFieldKind::Combo)
    }

    /// Shortcut: slider row.
    #[must_use]
    pub fn slider_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, BigFieldKind::Slider)
    }

    /// Shortcut: color picker row.
    #[must_use]
    pub fn color_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, BigFieldKind::Color)
    }

    /// Shortcut: font picker row.
    #[must_use]
    pub fn font_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, BigFieldKind::Font)
    }

    /// Mark the field as required. Empty values fail submission.
    #[must_use]
    pub fn required(mut self, enabled: bool) -> Self {
        self.required = enabled;
        self
    }

    /// Mark the field as holding secret data so logs, exports, and
    /// diagnostics know to redact it.
    #[must_use]
    pub fn sensitive(mut self, enabled: bool) -> Self {
        self.sensitive = enabled;
        self
    }

    /// Attach a localized tooltip shown on hover.
    #[must_use]
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Project the spec into the adapter-facing form: action name,
    /// validation requirement, picker requirement, redaction
    /// requirement.
    #[must_use]
    pub fn resolved(&self) -> BigFieldResolved {
        BigFieldResolved {
            action_name: format!("settings.{}", self.id),
            needs_validation: self.required
                || self.sensitive
                || self.kind == BigFieldKind::Password,
            needs_picker: self.kind == BigFieldKind::Path,
            must_redact: self.sensitive || self.kind == BigFieldKind::Password,
        }
    }
}

/// Adapter-facing flags computed from a [`BigFieldSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFieldResolved {
    /// Scoped action name in the form `"settings.<id>"`.
    pub action_name: String,
    /// `true` when the field must be passed through the validator
    /// before the form is allowed to submit.
    pub needs_validation: bool,
    /// `true` when the field needs a file/folder picker affordance.
    pub needs_picker: bool,
    /// `true` when display, logs, and exports must redact this
    /// field's value.
    pub must_redact: bool,
}

/// Group of related fields rendered as one `AdwPreferencesGroup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSettingsGroupSpec {
    /// Localized group header.
    pub title: String,
    /// Fields in display order.
    pub fields: Vec<BigFieldSpec>,
}

impl BigSettingsGroupSpec {
    /// Build a group with a title and the initial list of fields.
    #[must_use]
    pub fn new(title: impl Into<String>, fields: Vec<BigFieldSpec>) -> Self {
        Self {
            title: title.into(),
            fields,
        }
    }
}

/// Settings page composed of one or more [`BigSettingsGroupSpec`]
/// groups; rendered as an `AdwPreferencesPage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSettingsPageSpec {
    /// Localized page title used as the navigation row label.
    pub title: String,
    /// Groups in display order.
    pub groups: Vec<BigSettingsGroupSpec>,
}

impl BigSettingsPageSpec {
    /// Build an empty page; chain [`BigSettingsPageSpec::group`] to add
    /// content.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            groups: Vec::new(),
        }
    }

    /// Append a group to the page.
    #[must_use]
    pub fn group(mut self, group: BigSettingsGroupSpec) -> Self {
        self.groups.push(group);
        self
    }

    /// Sum the field counts of every group. Used by tests and audits
    /// to verify a page has the expected number of inputs.
    #[must_use]
    pub fn field_count(&self) -> usize {
        self.groups.iter().map(|group| group.fields.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_field_is_redacted_and_validated() {
        let resolved = BigFieldSpec::password_row("token", "Token").resolved();
        assert!(resolved.must_redact);
        assert!(resolved.needs_validation);
    }

    #[test]
    fn required_text_field_requires_validation_without_redaction() {
        let resolved = BigFieldSpec::text_row("name", "Name")
            .required(true)
            .resolved();

        assert_eq!(resolved.action_name, "settings.name");
        assert!(resolved.needs_validation);
        assert!(!resolved.needs_picker);
        assert!(!resolved.must_redact);
    }

    #[test]
    fn sensitive_text_field_requires_validation_and_redaction() {
        let resolved = BigFieldSpec::text_row("api-key", "API key")
            .sensitive(true)
            .resolved();

        assert!(resolved.needs_validation);
        assert!(!resolved.needs_picker);
        assert!(resolved.must_redact);
    }

    #[test]
    fn password_kind_requires_validation_and_redaction_without_sensitive_flag() {
        let resolved = BigFieldSpec::new("password", "Password", BigFieldKind::Password).resolved();

        assert!(resolved.needs_validation);
        assert!(!resolved.needs_picker);
        assert!(resolved.must_redact);
    }

    #[test]
    fn path_field_requires_picker_without_validation() {
        let resolved = BigFieldSpec::path_row("output", "Output").resolved();

        assert!(!resolved.needs_validation);
        assert!(resolved.needs_picker);
        assert!(!resolved.must_redact);
    }

    #[test]
    fn plain_text_field_has_action_without_validation_or_picker() {
        let resolved = BigFieldSpec::text_row("nickname", "Nickname").resolved();

        assert_eq!(resolved.action_name, "settings.nickname");
        assert!(!resolved.needs_validation);
        assert!(!resolved.needs_picker);
        assert!(!resolved.must_redact);
    }

    #[test]
    fn settings_page_counts_group_fields() {
        let page = BigSettingsPageSpec::new("General").group(BigSettingsGroupSpec::new(
            "Paths",
            vec![BigFieldSpec::new("output", "Output", BigFieldKind::Path)],
        ));
        assert_eq!(page.field_count(), 1);
    }

    #[test]
    fn settings_page_counts_fields_across_groups() {
        let page = BigSettingsPageSpec::new("General")
            .group(BigSettingsGroupSpec::new(
                "Paths",
                vec![
                    BigFieldSpec::path_row("output", "Output"),
                    BigFieldSpec::path_row("backup", "Backup"),
                ],
            ))
            .group(BigSettingsGroupSpec::new(
                "Network",
                vec![BigFieldSpec::text_row("server", "Server")],
            ));

        assert_eq!(page.field_count(), 3);
    }
}
