// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Switch helpers.

use relm4::gtk;

use crate::feedback::tooltip;

/// Data used to build a standalone switch control.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSwitchControlSpec {
    /// Accessible label exposed for assistive technology.
    pub accessible_label: String,
    /// Tooltip text. Defaults to the accessible label.
    pub tooltip: Option<String>,
    /// Initial active state.
    pub active: bool,
}

impl BigSwitchControlSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(accessible_label: impl Into<String>, active: bool) -> Self {
        Self {
            accessible_label: accessible_label.into(),
            tooltip: None,
            active,
        }
    }

    /// Configure the tooltip and return the updated builder.
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigSwitchControlResolved {
        BigSwitchControlResolved {
            accessible_label: self.accessible_label.clone(),
            tooltip: self.tooltip.clone(),
            active: self.active,
        }
    }
}

/// Pure resolved switch contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSwitchControlResolved {
    /// Accessible label exposed for assistive technology.
    pub accessible_label: String,
    /// Tooltip text. When absent, use [`Self::accessible_label`].
    pub tooltip: Option<String>,
    /// Initial active state.
    pub active: bool,
}

impl BigSwitchControlResolved {
    /// Return the tooltip text that should be attached to the switch.
    #[must_use]
    pub fn tooltip_label(&self) -> &str {
        self.tooltip.as_deref().unwrap_or(&self.accessible_label)
    }
}

/// Built standalone switch control.
#[derive(Debug, Clone)]
pub struct BigSwitchControl {
    root: gtk::Switch,
}

impl BigSwitchControl {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigSwitchControlSpec) -> Self {
        use relm4::gtk::prelude::*;

        let resolved = spec.resolved();
        let root = gtk::Switch::builder()
            .active(resolved.active)
            .valign(gtk::Align::Center)
            .build();
        tooltip::set(&root, resolved.tooltip_label());
        root.update_property(&[gtk::accessible::Property::Label(&resolved.accessible_label)]);
        Self { root }
    }

    /// Return a reference to the root switch.
    #[must_use]
    pub fn root(&self) -> &gtk::Switch {
        &self.root
    }

    /// Consume `self` and yield the underlying switch.
    #[must_use]
    pub fn into_root(self) -> gtk::Switch {
        self.root
    }
}

/// Keep two switches in sync without notify ping-pong.
///
/// Both directions hold the *other* switch WEAK: a two-way strong bind makes the
/// switches pin each other (mutual reference cycle) so neither finalizes when the
/// dialog closes. Each handler upgrades-or-skips.
pub fn sync_switches(dialog_switch: &gtk::Switch, source_switch: &gtk::Switch) {
    use relm4::gtk::prelude::*;
    dialog_switch.set_active(source_switch.is_active());

    {
        let source_weak = source_switch.downgrade();
        dialog_switch.connect_active_notify(move |switch| {
            if let Some(source_switch) = source_weak.upgrade()
                && source_switch.is_active() != switch.is_active()
            {
                source_switch.set_active(switch.is_active());
            }
        });
    }

    {
        let dialog_weak = dialog_switch.downgrade();
        source_switch.connect_active_notify(move |source| {
            if let Some(dialog_switch) = dialog_weak.upgrade()
                && dialog_switch.is_active() != source.is_active()
            {
                dialog_switch.set_active(source.is_active());
            }
        });
    }
}
