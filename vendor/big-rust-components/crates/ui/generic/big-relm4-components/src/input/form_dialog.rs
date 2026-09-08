// SPDX-License-Identifier: MIT

//! Form-dialog builder spec.
//!
//! Every BigLinux app has at least one "header + scrollable form +
//! Save/Cancel footer" dialog. [`BigFormDialogSpec`] captures the
//! pattern as data so widget builders, AT-SPI smoke harnesses, and
//! tests can reason about it without rendering.
//!
//! Validation is intentionally minimal: field validators return
//! `Result<(), String>` so any callable can be wired. The spec only
//! tracks declarative metadata (id, label, kind) and dirty/state
//! resolution.

/// Widget kind the form-dialog builder renders for a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigFormFieldKind {
    /// Free-form single-line entry.
    Text,
    /// Masked single-line entry; passes through redaction in logs.
    Password,
    /// Filesystem path entry with picker button.
    Path,
    /// Integer spin button.
    Integer,
    /// Floating-point spin button.
    Decimal,
    /// On/off switch row.
    Boolean,
    /// Drop-down picking one of a fixed set.
    Choice,
}

/// Display-free specification describing big form field behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFormFieldSpec {
    /// Stable id used by both AT-SPI and persistence.
    pub id: String,
    /// Visible label.
    pub label: String,
    /// Optional help text under the field.
    pub help: Option<String>,
    /// Kind drives the rendered widget.
    pub kind: BigFormFieldKind,
    /// `false` keeps the field disabled (read-only) in the dialog.
    pub editable: bool,
    /// `true` makes the field a required input — the resolved spec
    /// reports `has_required_unsatisfied` until the value is non-empty.
    pub required: bool,
}

impl BigFormFieldSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>, kind: BigFormFieldKind) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            help: None,
            kind,
            editable: true,
            required: false,
        }
    }

    /// Configure the `required` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigFormFieldSpec`].
    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Builder: sets help.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Configure the `read_only` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigFormFieldSpec`].
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.editable = false;
        self
    }
}

/// Form-dialog spec: header + scrollable field list + footer with save/cancel.
///
/// # Capabilities
///
/// `form-dialog`, `form-dialog+validation`
///
/// # Archetypes
///
/// `terminal`, `editor`, `file-manager`, `control-center`, `pkg-manager`, `network-manager`, `container-manager`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFormDialogSpec {
    /// Dialog title.
    pub title: String,
    /// Optional subtitle / description.
    pub subtitle: Option<String>,
    /// Save-button label (default `"Save"`).
    pub save_label: String,
    /// Cancel-button label (default `"Cancel"`).
    pub cancel_label: String,
    /// `true` styles Save as `suggested-action` (default).
    pub save_suggested: bool,
    /// `true` styles Save as `destructive-action`. Mutually exclusive
    /// with `save_suggested`.
    pub save_destructive: bool,
    /// Field declarations in the order they appear.
    pub fields: Vec<BigFormFieldSpec>,
}

impl BigFormDialogSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            save_label: "Save".to_string(),
            cancel_label: "Cancel".to_string(),
            save_suggested: true,
            save_destructive: false,
            fields: Vec::new(),
        }
    }

    /// Builder: sets subtitle.
    #[must_use]
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Builder: sets save label.
    #[must_use]
    pub fn with_save_label(mut self, label: impl Into<String>) -> Self {
        self.save_label = label.into();
        self
    }

    /// Builder: sets cancel label.
    #[must_use]
    pub fn with_cancel_label(mut self, label: impl Into<String>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// Make Save destructive (red); also clears `save_suggested`.
    #[must_use]
    pub fn destructive(mut self) -> Self {
        self.save_destructive = true;
        self.save_suggested = false;
        self
    }

    /// Builder: sets field.
    #[must_use]
    pub fn with_field(mut self, field: BigFormFieldSpec) -> Self {
        self.fields.push(field);
        self
    }

    /// Validate field uniqueness and exclusive Save-button styling.
    ///
    /// # Errors
    /// Returns a human-readable description of the first integrity
    /// violation found.
    pub fn validate(&self) -> Result<(), String> {
        if self.save_suggested && self.save_destructive {
            return Err("Save cannot be both suggested and destructive".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for f in &self.fields {
            if f.id.is_empty() {
                return Err("field has empty id".to_string());
            }
            if !seen.insert(f.id.clone()) {
                return Err(format!("duplicate field id: {}", f.id));
            }
        }
        Ok(())
    }

    /// Resolve the spec against a value map. Returns required-but-empty
    /// field ids; callers use this to disable the Save button until the
    /// form is complete.
    #[must_use]
    pub fn required_missing<'a, F>(&'a self, value_for: F) -> Vec<&'a str>
    where
        F: Fn(&str) -> Option<&'a str>,
    {
        self.fields
            .iter()
            .filter(|f| f.required && value_for(&f.id).is_none_or(str::is_empty))
            .map(|f| f.id.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_spec_validates() {
        let spec = BigFormDialogSpec::new("Add server");
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn duplicate_field_id_rejected() {
        let spec = BigFormDialogSpec::new("Add")
            .with_field(BigFormFieldSpec::new(
                "host",
                "Host",
                BigFormFieldKind::Text,
            ))
            .with_field(BigFormFieldSpec::new(
                "host",
                "Host2",
                BigFormFieldKind::Text,
            ));
        assert!(spec.validate().is_err());
    }

    #[test]
    fn destructive_clears_suggested() {
        let spec = BigFormDialogSpec::new("Delete profile").destructive();
        assert!(spec.save_destructive);
        assert!(!spec.save_suggested);
        spec.validate().unwrap();
    }

    #[test]
    fn required_missing_reports_blanks() {
        let spec = BigFormDialogSpec::new("Add")
            .with_field(BigFormFieldSpec::new("host", "Host", BigFormFieldKind::Text).required())
            .with_field(
                BigFormFieldSpec::new("port", "Port", BigFormFieldKind::Integer).required(),
            );
        let missing = spec.required_missing(|id| match id {
            "host" => Some("ssh.example.com"),
            _ => None,
        });
        assert_eq!(missing, vec!["port"]);
    }

    #[test]
    fn read_only_field_carries_state() {
        let f = BigFormFieldSpec::new("id", "ID", BigFormFieldKind::Text).read_only();
        assert!(!f.editable);
    }

    #[test]
    fn empty_field_id_rejected() {
        let spec = BigFormDialogSpec::new("Add").with_field(BigFormFieldSpec::new(
            "",
            "x",
            BigFormFieldKind::Text,
        ));
        assert!(spec.validate().is_err());
    }
}
