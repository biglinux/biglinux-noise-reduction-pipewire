// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Action row with standard edit/remove suffix buttons.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

use super::row_core::row_text;

/// Display-free specification describing big editable action row behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEditableActionRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Edit label.
    pub edit_label: String,
    /// Remove label.
    pub remove_label: String,
    /// Allow markup.
    pub allow_markup: bool,
}

impl BigEditableActionRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        edit_label: impl Into<String>,
        remove_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            prefix_icon_name: None,
            edit_label: edit_label.into(),
            remove_label: remove_label.into(),
            allow_markup: false,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigEditableActionRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `prefix_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigEditableActionRowSpec`].
    #[must_use]
    pub fn prefix_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.prefix_icon_name = Some(icon_name.into());
        self
    }

    /// Configure the `allow_markup` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigEditableActionRowSpec`].
    #[must_use]
    pub fn allow_markup(mut self) -> Self {
        self.allow_markup = true;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigEditableActionRowResolved {
        BigEditableActionRowResolved {
            title: row_text(&self.title, self.allow_markup),
            subtitle: self
                .subtitle
                .as_ref()
                .map(|subtitle| row_text(subtitle, self.allow_markup)),
            prefix_icon_name: self.prefix_icon_name.clone(),
            edit_label: self.edit_label.clone(),
            remove_label: self.remove_label.clone(),
        }
    }
}

/// Resolved counterpart of `BigEditableActionRowSpec` ready for the widget adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEditableActionRowResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Edit label.
    pub edit_label: String,
    /// Remove label.
    pub remove_label: String,
}

/// Action row with edit and remove suffix buttons, used in list
/// pages that let the user mutate per-row state.
#[derive(Debug, Clone)]
pub struct BigEditableActionRow {
    row: adw::ActionRow,
    edit_button: gtk::Button,
    remove_button: gtk::Button,
}

impl BigEditableActionRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigEditableActionRowSpec) -> Self {
        let resolved = spec.resolved();
        let mut builder = adw::ActionRow::builder()
            .title(resolved.title.as_str())
            .activatable(true);
        if let Some(subtitle) = resolved.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }
        let row = builder.build();

        if let Some(icon_name) = resolved.prefix_icon_name.as_deref() {
            row.add_prefix(&gtk::Image::from_icon_name(icon_name));
        }

        let edit_button =
            tooltip::icon_button("document-edit-symbolic", &resolved.edit_label, &["flat"]);
        edit_button.set_valign(gtk::Align::Center);
        row.add_suffix(&edit_button);

        let remove_button =
            tooltip::icon_button("user-trash-symbolic", &resolved.remove_label, &["flat"]);
        remove_button.set_valign(gtk::Align::Center);
        row.add_suffix(&remove_button);

        Self {
            row,
            edit_button,
            remove_button,
        }
    }

    /// Return a reference to the `row` exposed by this [`BigEditableActionRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn row(&self) -> &adw::ActionRow {
        &self.row
    }

    /// Return a reference to the `edit button` exposed by this [`BigEditableActionRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn edit_button(&self) -> &gtk::Button {
        &self.edit_button
    }

    /// Return a reference to the `remove button` exposed by this [`BigEditableActionRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn remove_button(&self) -> &gtk::Button {
        &self.remove_button
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (adw::ActionRow, gtk::Button, gtk::Button) {
        (self.row, self.edit_button, self.remove_button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_escapes_plain_text() {
        let resolved = BigEditableActionRowSpec::new("<b>Name</b>", "Edit", "Remove")
            .subtitle("<x>")
            .resolved();

        assert_eq!(resolved.title, "&lt;b&gt;Name&lt;/b&gt;");
        assert_eq!(resolved.subtitle.as_deref(), Some("&lt;x&gt;"));
    }

    #[test]
    fn resolved_keeps_button_labels() {
        let resolved = BigEditableActionRowSpec::new("Name", "Edit row", "Remove row").resolved();

        assert_eq!(resolved.edit_label, "Edit row");
        assert_eq!(resolved.remove_label, "Remove row");
    }
}
