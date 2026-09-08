// SPDX-License-Identifier: MIT

//! Error-state spec.
//!
//! [`BigErrorStateSpec`] frames a failure surface with an icon, title,
//! body, optional copyable diagnostic details, and a retry action.
//! Pair with [`crate::state::retry::BigRetryPolicy`] for transient
//! failures.

/// Enumeration of supported big error kind variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigErrorKind {
    /// Likely to succeed if retried (network blip, lock contention).
    Transient,
    /// Will not succeed without user intervention (permission denied,
    /// missing file, malformed input).
    Permanent,
}

/// Spec for an error surface: icon, title, body, optional details and retry.
///
/// Pair with [`crate::state::retry::BigRetryPolicy`] for transient failures.
///
/// # Examples
///
/// Build a transient (retryable) error spec and inspect it:
///
/// ```
/// use big_relm4_components::state::error::{BigErrorKind, BigErrorStateSpec};
///
/// let spec = BigErrorStateSpec::transient(
///     "Could not reach server",
///     "Check your connection and try again.",
/// )
/// .with_secondary("Open settings", "app.preferences");
///
/// assert_eq!(spec.kind, BigErrorKind::Transient);
/// assert!(spec.offers_retry());
/// assert_eq!(spec.secondary.unwrap().action_id, "app.preferences");
/// ```
///
/// In a Relm4 component the spec is the typed `Init`:
///
/// ```ignore
/// impl relm4::SimpleComponent for ErrorSurface {
///     type Init = BigErrorStateSpec;
///     type Input = ();
///     type Output = ();
///     // model = ErrorSurface { spec }
///     // view: adw::StatusPage + optional retry / secondary buttons
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigErrorStateSpec {
    /// `Transient` vs `Permanent`. Drives whether retry is offered.
    pub kind: BigErrorKind,
    /// Icon name (typically `"dialog-error-symbolic"` or `"network-error-symbolic"`).
    pub icon_name: String,
    /// Short title (e.g. "Could not reach server").
    pub title: String,
    /// User-facing body (1-3 sentences, no jargon).
    pub body: String,
    /// Optional copyable details (stack trace, full diagnostic).
    /// Rendered in a collapsed disclosure to avoid intimidating users.
    pub details: Option<String>,
    /// Optional retry action label. `None` disables the retry button.
    pub retry_label: Option<String>,
    /// Optional secondary action ("Open settings", "Check logs").
    pub secondary: Option<BigErrorSecondaryAction>,
}

/// Secondary action button on an error page (e.g. "Open settings",
/// "Check logs").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigErrorSecondaryAction {
    /// Label.
    pub label: String,
    /// Action id.
    pub action_id: String,
}

impl BigErrorStateSpec {
    /// Construct a [`BigErrorStateSpec`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn transient(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            kind: BigErrorKind::Transient,
            icon_name: "dialog-warning-symbolic".to_string(),
            title: title.into(),
            body: body.into(),
            details: None,
            retry_label: Some("Try again".to_string()),
            secondary: None,
        }
    }

    /// Construct a [`BigErrorStateSpec`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn permanent(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            kind: BigErrorKind::Permanent,
            icon_name: "dialog-error-symbolic".to_string(),
            title: title.into(),
            body: body.into(),
            details: None,
            retry_label: None,
            secondary: None,
        }
    }

    /// Builder: sets details.
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Builder: sets icon.
    #[must_use]
    pub fn with_icon(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = icon_name.into();
        self
    }

    /// Builder: sets secondary.
    #[must_use]
    pub fn with_secondary(
        mut self,
        label: impl Into<String>,
        action_id: impl Into<String>,
    ) -> Self {
        self.secondary = Some(BigErrorSecondaryAction {
            label: label.into(),
            action_id: action_id.into(),
        });
        self
    }

    /// Override or disable the retry label.
    #[must_use]
    pub fn with_retry(mut self, label: Option<&str>) -> Self {
        self.retry_label = label.map(ToString::to_string);
        self
    }

    /// Convenience: `true` when retry is enabled.
    #[must_use]
    pub fn offers_retry(&self) -> bool {
        self.retry_label.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_offers_retry_by_default() {
        let spec = BigErrorStateSpec::transient("Network down", "Try again later.");
        assert!(spec.offers_retry());
        assert_eq!(spec.kind, BigErrorKind::Transient);
    }

    #[test]
    fn permanent_does_not_offer_retry() {
        let spec = BigErrorStateSpec::permanent("Permission denied", "File is read-only.");
        assert!(!spec.offers_retry());
        assert_eq!(spec.kind, BigErrorKind::Permanent);
    }

    #[test]
    fn details_attach_to_disclosure() {
        let spec = BigErrorStateSpec::permanent("Decode failed", "Bad media file.")
            .with_details("ffprobe: Invalid data found when processing input\n  at 0x42");
        assert!(spec.details.unwrap().contains("ffprobe"));
    }

    #[test]
    fn secondary_action_optional() {
        let spec = BigErrorStateSpec::transient("Offline", "No network.")
            .with_secondary("Open settings", "app.preferences");
        assert_eq!(spec.secondary.unwrap().action_id, "app.preferences");
    }

    #[test]
    fn retry_can_be_disabled() {
        let spec = BigErrorStateSpec::transient("X", "Y").with_retry(None);
        assert!(!spec.offers_retry());
    }
}
