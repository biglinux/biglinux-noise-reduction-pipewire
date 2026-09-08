// SPDX-License-Identifier: MIT

//! Empty state spec.
//!
//! Use when the user has reached a primary surface with no data: empty
//! library, empty playlist, empty inbox, empty file queue, no podcast
//! subscriptions, no Wi-Fi networks. Pair with one primary call-to-action
//! (CTA) so the next step is obvious.

/// Spec for an empty-state surface (icon, title, optional body and CTA).
///
/// Consumers feed this into a widget builder (typically wrapping
/// `adw::StatusPage`) as the `Init` payload of a Relm4 component.
///
/// # Examples
///
/// Building the spec and wiring it into a Relm4 component's `init`:
///
/// ```
/// use big_relm4_components::state::empty::BigEmptyStateSpec;
///
/// let spec = BigEmptyStateSpec::new("folder-music-symbolic", "No Audio Files")
///     .with_body("Drag a file here to begin.")
///     .with_action("Open folder", "win.open-folder");
///
/// assert_eq!(spec.title, "No Audio Files");
/// assert!(spec.action.is_some());
/// ```
///
/// In a Relm4 component, the spec is the typed `Init`:
///
/// ```ignore
/// impl relm4::SimpleComponent for LibraryEmpty {
///     type Init = BigEmptyStateSpec;
///     type Input = ();
///     type Output = ();
///     // model = LibraryEmpty { spec }
///     // view: adw::StatusPage built from spec.icon_name / spec.title / spec.body
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEmptyStateSpec {
    /// Icon name from the GTK icon theme (e.g. `"folder-symbolic"`).
    pub icon_name: String,
    /// Primary heading.
    pub title: String,
    /// Optional explanatory body. Keep under 140 characters.
    pub body: Option<String>,
    /// Optional primary action label, paired with `action_id`.
    pub action: Option<BigEmptyStateAction>,
}

/// Primary call-to-action button shown on an empty-state page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEmptyStateAction {
    /// Visible button label.
    pub label: String,
    /// `GAction` name to activate.
    pub action_id: String,
    /// Treat the button as `suggested-action` style.
    pub suggested: bool,
}

/// One numbered step in a guided empty state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigGuidedEmptyStateStep {
    /// Visible step label.
    pub label: String,
}

impl BigGuidedEmptyStateStep {
    /// Creates a new instance.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

/// A richer empty-state surface with short guidance and multiple actions.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigGuidedEmptyStateSpec {
    /// Icon name from the GTK icon theme.
    pub icon_name: String,
    /// Primary heading.
    pub title: String,
    /// Optional explanatory body.
    pub body: Option<String>,
    /// Ordered guidance steps.
    pub steps: Vec<BigGuidedEmptyStateStep>,
    /// Ordered action buttons.
    pub actions: Vec<BigEmptyStateAction>,
    /// Extra CSS classes applied to the root container.
    pub root_css_classes: Vec<String>,
    /// Extra CSS classes applied to the icon.
    pub icon_css_classes: Vec<String>,
    /// Extra CSS classes applied to the steps container.
    pub steps_css_classes: Vec<String>,
    /// Extra CSS classes applied to each step row.
    pub step_css_classes: Vec<String>,
    /// Extra CSS classes applied to each step number badge.
    pub step_number_css_classes: Vec<String>,
}

impl BigEmptyStateSpec {
    /// Title-only empty state.
    #[must_use]
    pub fn new(icon_name: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            icon_name: icon_name.into(),
            title: title.into(),
            body: None,
            action: None,
        }
    }

    /// Builder: sets body.
    #[must_use]
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Builder: sets action.
    #[must_use]
    pub fn with_action(mut self, label: impl Into<String>, action_id: impl Into<String>) -> Self {
        self.action = Some(BigEmptyStateAction {
            label: label.into(),
            action_id: action_id.into(),
            suggested: true,
        });
        self
    }
}

impl BigGuidedEmptyStateSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(icon_name: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            icon_name: icon_name.into(),
            title: title.into(),
            body: None,
            steps: Vec::new(),
            actions: Vec::new(),
            root_css_classes: Vec::new(),
            icon_css_classes: Vec::new(),
            steps_css_classes: Vec::new(),
            step_css_classes: Vec::new(),
            step_number_css_classes: Vec::new(),
        }
    }

    /// Builder: sets body.
    #[must_use]
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Builder: appends one guidance step.
    #[must_use]
    pub fn with_step(mut self, label: impl Into<String>) -> Self {
        self.steps.push(BigGuidedEmptyStateStep::new(label));
        self
    }

    /// Builder: appends one action button.
    #[must_use]
    pub fn with_action(
        mut self,
        label: impl Into<String>,
        action_id: impl Into<String>,
        suggested: bool,
    ) -> Self {
        self.actions.push(BigEmptyStateAction {
            label: label.into(),
            action_id: action_id.into(),
            suggested,
        });
        self
    }

    /// Builder: appends extra root CSS classes.
    #[must_use]
    pub fn root_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.root_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: appends extra icon CSS classes.
    #[must_use]
    pub fn icon_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.icon_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: appends extra steps container CSS classes.
    #[must_use]
    pub fn steps_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.steps_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: appends extra step row CSS classes.
    #[must_use]
    pub fn step_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.step_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: appends extra step number CSS classes.
    #[must_use]
    pub fn step_number_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.step_number_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_only_state_has_no_action() {
        let spec = BigEmptyStateSpec::new("folder-symbolic", "No files yet");
        assert!(spec.action.is_none());
        assert!(spec.body.is_none());
    }

    #[test]
    fn body_attaches_to_spec() {
        let spec = BigEmptyStateSpec::new("folder-symbolic", "No files")
            .with_body("Drag a file to start.");
        assert_eq!(spec.body.as_deref(), Some("Drag a file to start."));
    }

    #[test]
    fn action_attaches_and_is_suggested_by_default() {
        let spec = BigEmptyStateSpec::new("folder-symbolic", "No files")
            .with_action("Open folder", "win.open-folder");
        let action = spec.action.unwrap();
        assert_eq!(action.label, "Open folder");
        assert_eq!(action.action_id, "win.open-folder");
        assert!(action.suggested);
    }

    #[test]
    fn guided_empty_state_records_steps_actions_and_classes() {
        let spec = BigGuidedEmptyStateSpec::new("utilities-terminal-symbolic", "Teach")
            .with_body("Open a lesson.")
            .with_step("Create or open")
            .with_action("New", "win.lesson-new", true)
            .root_css_classes(["lesson-empty-state"])
            .icon_css_classes(["lesson-empty-icon"])
            .steps_css_classes(["lesson-empty-steps"])
            .step_css_classes(["lesson-empty-step"])
            .step_number_css_classes(["lesson-empty-step-number"]);

        assert_eq!(spec.body.as_deref(), Some("Open a lesson."));
        assert_eq!(spec.steps[0].label, "Create or open");
        assert_eq!(spec.actions[0].action_id, "win.lesson-new");
        assert_eq!(spec.root_css_classes, ["lesson-empty-state"]);
        assert_eq!(spec.icon_css_classes, ["lesson-empty-icon"]);
        assert_eq!(spec.steps_css_classes, ["lesson-empty-steps"]);
        assert_eq!(spec.step_css_classes, ["lesson-empty-step"]);
        assert_eq!(spec.step_number_css_classes, ["lesson-empty-step-number"]);
    }
}
