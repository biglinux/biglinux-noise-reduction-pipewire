// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux check/radio action row helper.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

use super::row_core::row_text;

const DEFAULT_BADGE_CLASSES: &[&str] = &["success", "caption", "pill"];

/// How the row behaves when activated.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigCheckRowActivation {
    /// Row activation toggles the check button.
    ToggleCheck,
    /// Row activation opens another surface; caller owns the action handler.
    OpenDetail,
}

/// Data used to build a check/radio action row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigCheckActionRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Badge.
    pub badge: Option<String>,
    /// Badge css classes.
    pub badge_css_classes: Vec<String>,
    /// Trailing icon name.
    pub trailing_icon_name: Option<String>,
    /// Active.
    pub active: bool,
    /// Activation.
    pub activation: BigCheckRowActivation,
    /// Allow markup.
    pub allow_markup: bool,
}

impl BigCheckActionRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            badge: None,
            badge_css_classes: DEFAULT_BADGE_CLASSES
                .iter()
                .map(ToString::to_string)
                .collect(),
            trailing_icon_name: None,
            active: false,
            activation: BigCheckRowActivation::ToggleCheck,
            allow_markup: false,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigCheckActionRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `badge` setting and return the updated builder.
    ///
    /// The supplied `badge` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigCheckActionRowSpec`].
    #[must_use]
    pub fn badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// Configure the `badge_css_classes` setting and return the updated builder.
    ///
    /// The supplied `classes` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigCheckActionRowSpec`].
    #[must_use]
    pub fn badge_css_classes(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.badge_css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Configure the `trailing_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigCheckActionRowSpec`].
    #[must_use]
    pub fn trailing_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.trailing_icon_name = Some(icon_name.into());
        self
    }

    /// Configure the `active` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigCheckActionRowSpec`].
    #[must_use]
    pub fn active(mut self) -> Self {
        self.active = true;
        self
    }

    /// Configure the `activation` setting and return the updated builder.
    ///
    /// The supplied `activation` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigCheckActionRowSpec`].
    #[must_use]
    pub fn activation(mut self, activation: BigCheckRowActivation) -> Self {
        self.activation = activation;
        self
    }

    /// Allow libadwaita markup in title/subtitle/badge.
    ///
    /// Default is plain text; strings are escaped before they reach the row.
    #[must_use]
    pub fn allow_markup(mut self) -> Self {
        self.allow_markup = true;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigCheckActionRowResolved {
        BigCheckActionRowResolved {
            title: row_text(&self.title, self.allow_markup),
            subtitle: self
                .subtitle
                .as_ref()
                .map(|subtitle| row_text(subtitle, self.allow_markup)),
            badge: self
                .badge
                .as_ref()
                .map(|badge| row_text(badge, self.allow_markup)),
            badge_css_classes: self.badge_css_classes.clone(),
            trailing_icon_name: self.trailing_icon_name.clone(),
            active: self.active,
            activation: self.activation,
        }
    }
}

/// Pure resolved row contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigCheckActionRowResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Badge.
    pub badge: Option<String>,
    /// Badge css classes.
    pub badge_css_classes: Vec<String>,
    /// Trailing icon name.
    pub trailing_icon_name: Option<String>,
    /// Active.
    pub active: bool,
    /// Activation.
    pub activation: BigCheckRowActivation,
}

/// Built row plus its explicit check button.
#[derive(Debug, Clone)]
pub struct BigCheckActionRow {
    root: adw::ActionRow,
    check: gtk::CheckButton,
}

impl BigCheckActionRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigCheckActionRowSpec, group: Option<&gtk::CheckButton>) -> Self {
        let accessible_title = spec.title.clone();
        let resolved = spec.resolved();
        let check = gtk::CheckButton::new();
        check.update_property(&[gtk::accessible::Property::Label(&accessible_title)]);

        if let Some(group) = group {
            check.set_group(Some(group));
        }

        check.set_active(resolved.active);

        let mut builder = adw::ActionRow::builder().title(resolved.title.as_str());

        match resolved.activation {
            BigCheckRowActivation::ToggleCheck => {
                builder = builder.activatable_widget(&check);
            }
            BigCheckRowActivation::OpenDetail => {
                builder = builder.activatable(true);
            }
        }

        if let Some(subtitle) = resolved.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }

        let root = builder.build();
        root.add_prefix(&check);

        if let Some(badge_text) = resolved.badge.as_deref() {
            let badge = gtk::Label::builder()
                .label(badge_text)
                .css_classes(resolved.badge_css_classes.clone())
                .valign(gtk::Align::Center)
                .build();
            root.add_suffix(&badge);
        }

        if let Some(icon_name) = resolved.trailing_icon_name.as_deref() {
            match resolved.activation {
                BigCheckRowActivation::ToggleCheck => {
                    root.add_suffix(&gtk::Image::from_icon_name(icon_name));
                }
                BigCheckRowActivation::OpenDetail => {
                    let action_button =
                        tooltip::icon_button(icon_name, &resolved.title, &["flat", "circular"]);
                    let root_weak = root.downgrade();
                    action_button.connect_clicked(move |_| {
                        if let Some(root) = root_weak.upgrade() {
                            ActionRowExt::activate(&root);
                        }
                    });
                    root.add_suffix(&action_button);
                }
            }
        }

        Self { root, check }
    }

    /// Return a reference to the `root` exposed by this [`BigCheckActionRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::ActionRow {
        &self.root
    }

    /// Return a reference to the `check` exposed by this [`BigCheckActionRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn check(&self) -> &gtk::CheckButton {
        &self.check
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (adw::ActionRow, gtk::CheckButton) {
        (self.root, self.check)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_to_inactive_plain_text_row() {
        let spec = BigCheckActionRowSpec::new("Universal");

        assert_eq!(spec.title, "Universal");
        assert_eq!(spec.subtitle, None);
        assert_eq!(spec.badge, None);
        assert_eq!(
            spec.badge_css_classes,
            ["success", "caption", "pill"].map(str::to_string)
        );
        assert_eq!(spec.trailing_icon_name, None);
        assert!(!spec.active);
        assert_eq!(spec.activation, BigCheckRowActivation::ToggleCheck);
        assert!(!spec.allow_markup);
    }

    #[test]
    fn spec_builder_records_optional_parts() {
        let spec = BigCheckActionRowSpec::new("Universal")
            .subtitle("H.264")
            .badge("Recommended")
            .badge_css_classes(["accent", "caption"])
            .trailing_icon_name("go-next-symbolic")
            .active()
            .activation(BigCheckRowActivation::OpenDetail)
            .allow_markup();

        assert_eq!(spec.subtitle.as_deref(), Some("H.264"));
        assert_eq!(spec.badge.as_deref(), Some("Recommended"));
        assert_eq!(spec.badge_css_classes, ["accent", "caption"]);
        assert_eq!(spec.trailing_icon_name.as_deref(), Some("go-next-symbolic"));
        assert!(spec.active);
        assert_eq!(spec.activation, BigCheckRowActivation::OpenDetail);
        assert!(spec.allow_markup);
    }

    #[test]
    fn resolved_escapes_markup_by_default() {
        let resolved = BigCheckActionRowSpec::new("<b>Universal</b>")
            .subtitle("<i>H.264</i>")
            .badge("<span>Recommended</span>")
            .resolved();

        assert_eq!(resolved.title, "&lt;b&gt;Universal&lt;/b&gt;");
        assert_eq!(
            resolved.subtitle.as_deref(),
            Some("&lt;i&gt;H.264&lt;/i&gt;")
        );
        assert_eq!(
            resolved.badge.as_deref(),
            Some("&lt;span&gt;Recommended&lt;/span&gt;")
        );
    }

    #[test]
    fn resolved_can_allow_markup_explicitly() {
        let resolved = BigCheckActionRowSpec::new("<b>Universal</b>")
            .subtitle("<i>H.264</i>")
            .allow_markup()
            .resolved();

        assert_eq!(resolved.title, "<b>Universal</b>");
        assert_eq!(resolved.subtitle.as_deref(), Some("<i>H.264</i>"));
    }
}
