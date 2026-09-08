// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Typed Adwaita toggle group for segmented choices.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

/// One option inside a [`BigToggleGroupSpec`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigToggleOptionSpec {
    /// Stable option name used by app state and tests.
    pub name: String,
    /// Visible option label.
    pub label: String,
    /// Optional symbolic icon name.
    pub icon_name: Option<String>,
    /// Optional option description exposed by libadwaita.
    pub description: Option<String>,
    /// Optional option tooltip.
    pub tooltip: Option<String>,
    /// Whether this option is enabled.
    pub enabled: bool,
    /// Whether mnemonic underlines are used.
    pub use_underline: bool,
}

impl BigToggleOptionSpec {
    /// Create an enabled toggle option from a stable name and visible label.
    #[must_use]
    pub fn new(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            icon_name: None,
            description: None,
            tooltip: None,
            enabled: true,
            use_underline: false,
        }
    }

    /// Configure a symbolic icon name.
    #[must_use]
    pub fn icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    /// Configure the option description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Configure the option tooltip.
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Disable the option.
    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Enable mnemonic underlines in the visible label.
    #[must_use]
    pub fn use_underline(mut self) -> Self {
        self.use_underline = true;
        self
    }
}

/// Data used to build an Adwaita segmented toggle group.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigToggleGroupSpec {
    /// Toggle options in visual order.
    pub options: Vec<BigToggleOptionSpec>,
    /// Active index used when [`Self::active_name`] is unset or invalid.
    pub active: u32,
    /// Stable active name.
    pub active_name: Option<String>,
    /// Accessible label for the group.
    pub accessible_label: Option<String>,
    /// Accessible description for the group.
    pub accessible_description: Option<String>,
    /// BigLinux tooltip for the group.
    pub tooltip: Option<String>,
    /// CSS classes applied to the group.
    pub css_classes: Vec<String>,
    /// Whether all toggles use equal width.
    pub homogeneous: bool,
    /// Whether the group may shrink in tight layouts.
    pub can_shrink: bool,
}

impl BigToggleGroupSpec {
    /// Create a toggle group from ordered options.
    #[must_use]
    pub fn new(options: impl IntoIterator<Item = BigToggleOptionSpec>) -> Self {
        Self {
            options: options.into_iter().collect(),
            active: 0,
            active_name: None,
            accessible_label: None,
            accessible_description: None,
            tooltip: None,
            css_classes: Vec::new(),
            homogeneous: true,
            can_shrink: true,
        }
    }

    /// Configure the active index.
    #[must_use]
    pub fn active(mut self, active: u32) -> Self {
        self.active = active;
        self.active_name = None;
        self
    }

    /// Configure the active stable option name.
    #[must_use]
    pub fn active_name(mut self, active_name: impl Into<String>) -> Self {
        self.active_name = Some(active_name.into());
        self
    }

    /// Configure the accessible group label.
    #[must_use]
    pub fn accessible_label(mut self, label: impl Into<String>) -> Self {
        self.accessible_label = Some(label.into());
        self
    }

    /// Configure the accessible group description.
    #[must_use]
    pub fn accessible_description(mut self, description: impl Into<String>) -> Self {
        self.accessible_description = Some(description.into());
        self
    }

    /// Configure the BigLinux tooltip.
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Add one CSS class to the group.
    #[must_use]
    pub fn css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_classes.push(css_class.into());
        self
    }

    /// Make toggle widths reflect content width.
    #[must_use]
    pub fn non_homogeneous(mut self) -> Self {
        self.homogeneous = false;
        self
    }

    /// Prevent shrinking in tight layouts.
    #[must_use]
    pub fn fixed_width(mut self) -> Self {
        self.can_shrink = false;
        self
    }

    /// Resolve defaults into a display-free contract.
    #[must_use]
    pub fn resolved(&self) -> BigToggleGroupResolved {
        let max_index = self.options.len().saturating_sub(1) as u32;
        let matched_active_name = self
            .active_name
            .as_ref()
            .filter(|name| self.options.iter().any(|option| option.name == **name))
            .cloned();
        let active = matched_active_name
            .as_ref()
            .and_then(|name| self.options.iter().position(|option| option.name == *name))
            .map_or_else(|| self.active.min(max_index), |index| index as u32);

        BigToggleGroupResolved {
            options: self.options.clone(),
            active,
            active_name: matched_active_name,
            accessible_label: self.accessible_label.clone(),
            accessible_description: self.accessible_description.clone(),
            tooltip: self.tooltip.clone(),
            css_classes: self.css_classes.clone(),
            homogeneous: self.homogeneous,
            can_shrink: self.can_shrink,
        }
    }
}

/// Pure resolved toggle group contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigToggleGroupResolved {
    /// Toggle options in visual order.
    pub options: Vec<BigToggleOptionSpec>,
    /// Active index after clamping or name resolution.
    pub active: u32,
    /// Active stable name after validation.
    pub active_name: Option<String>,
    /// Accessible label for the group.
    pub accessible_label: Option<String>,
    /// Accessible description for the group.
    pub accessible_description: Option<String>,
    /// BigLinux tooltip for the group.
    pub tooltip: Option<String>,
    /// CSS classes applied to the group.
    pub css_classes: Vec<String>,
    /// Whether all toggles use equal width.
    pub homogeneous: bool,
    /// Whether the group may shrink in tight layouts.
    pub can_shrink: bool,
}

/// Built toggle group plus the stable option names used to map outputs.
#[derive(Debug, Clone)]
pub struct BigToggleGroup {
    root: adw::ToggleGroup,
    option_names: Vec<String>,
}

impl BigToggleGroup {
    /// Build an Adwaita toggle group from a semantic BigLinux spec.
    #[must_use]
    pub fn new(spec: BigToggleGroupSpec) -> Self {
        let resolved = spec.resolved();
        let root = adw::ToggleGroup::new();
        root.set_active(resolved.active);
        root.set_homogeneous(resolved.homogeneous);
        root.set_can_shrink(resolved.can_shrink);

        if let Some(tooltip_text) = resolved.tooltip.as_deref() {
            tooltip::set(&root, tooltip_text);
        }
        let accessible_description = resolved
            .accessible_description
            .as_deref()
            .or(resolved.tooltip.as_deref());
        let mut properties = Vec::new();
        if let Some(label) = resolved.accessible_label.as_deref() {
            properties.push(gtk::accessible::Property::Label(label));
        }
        if let Some(description) = accessible_description {
            properties.push(gtk::accessible::Property::Description(description));
        }
        if !properties.is_empty() {
            root.update_property(&properties);
        }
        for css_class in &resolved.css_classes {
            root.add_css_class(css_class);
        }

        let option_names = resolved
            .options
            .iter()
            .map(|option| option.name.clone())
            .collect::<Vec<_>>();

        for option in resolved.options {
            root.add(build_toggle(option));
        }

        if let Some(active_name) = resolved.active_name.as_deref() {
            root.set_active_name(Some(active_name));
        }

        Self { root, option_names }
    }

    /// Return the root [`adw::ToggleGroup`].
    #[must_use]
    pub fn root(&self) -> &adw::ToggleGroup {
        &self.root
    }

    /// Return the stable names in visual order.
    #[must_use]
    pub fn option_names(&self) -> &[String] {
        &self.option_names
    }

    /// Return the stable name for the current active index, when it is known.
    #[must_use]
    pub fn active_name(&self) -> Option<&str> {
        self.option_names
            .get(self.root.active() as usize)
            .map(String::as_str)
    }

    /// Consume this wrapper and yield the underlying group.
    #[must_use]
    pub fn into_root(self) -> adw::ToggleGroup {
        self.root
    }
}

fn build_toggle(option: BigToggleOptionSpec) -> adw::Toggle {
    let mut builder = adw::Toggle::builder()
        .name(option.name)
        .label(option.label)
        .enabled(option.enabled)
        .use_underline(option.use_underline);

    if let Some(icon_name) = option.icon_name {
        builder = builder.icon_name(icon_name);
    }
    if let Some(description) = option.description {
        builder = builder.description(description);
    }
    if let Some(tooltip_text) = option.tooltip {
        builder = builder.tooltip(tooltip_text);
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export_options() -> [BigToggleOptionSpec; 2] {
        [
            BigToggleOptionSpec::new("fast", "Keep format"),
            BigToggleOptionSpec::new("precise", "Re-encode"),
        ]
    }

    #[test]
    fn resolved_defaults_to_first_option() {
        let resolved = BigToggleGroupSpec::new(export_options()).resolved();

        assert_eq!(resolved.active, 0);
        assert_eq!(resolved.active_name, None);
        assert!(resolved.homogeneous);
        assert!(resolved.can_shrink);
        assert_eq!(resolved.options[0].name, "fast");
    }

    #[test]
    fn active_index_is_clamped_to_available_options() {
        let resolved = BigToggleGroupSpec::new(export_options())
            .active(99)
            .resolved();

        assert_eq!(resolved.active, 1);
    }

    #[test]
    fn active_name_wins_when_it_matches_an_option() {
        let resolved = BigToggleGroupSpec::new(export_options())
            .active(0)
            .active_name("precise")
            .resolved();

        assert_eq!(resolved.active, 1);
        assert_eq!(resolved.active_name.as_deref(), Some("precise"));
    }

    #[test]
    fn invalid_active_name_falls_back_to_index() {
        let resolved = BigToggleGroupSpec::new(export_options())
            .active(1)
            .active_name("missing")
            .resolved();

        assert_eq!(resolved.active, 1);
        assert_eq!(resolved.active_name, None);
    }

    #[test]
    fn option_builder_records_metadata() {
        let option = BigToggleOptionSpec::new("compact", "Compact")
            .icon_name("view-list-compact-symbolic")
            .description("Uses less space")
            .tooltip("Compact view")
            .disabled()
            .use_underline();

        assert_eq!(
            option.icon_name.as_deref(),
            Some("view-list-compact-symbolic")
        );
        assert_eq!(option.description.as_deref(), Some("Uses less space"));
        assert_eq!(option.tooltip.as_deref(), Some("Compact view"));
        assert!(!option.enabled);
        assert!(option.use_underline);
    }
}
