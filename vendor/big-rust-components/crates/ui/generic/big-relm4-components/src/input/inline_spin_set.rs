// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Compact inline spin-button sets for dense settings cards.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

use super::preference_cards::BigSpinRange;

const DEFAULT_SPACING: i32 = 10;
const DEFAULT_SPIN_FIELD_SPACING: i32 = 4;

/// Display-free specification describing one inline spin-button field.
#[derive(Debug, Clone, PartialEq)]
pub struct BigInlineSpinFieldSpec {
    /// Id.
    pub id: String,
    /// Label.
    pub label: String,
    /// Value.
    pub value: f64,
    /// Range.
    pub range: BigSpinRange,
    /// Digits.
    pub digits: u32,
    /// Width chars.
    pub width_chars: i32,
    /// Unit.
    pub unit: Option<String>,
}

impl BigInlineSpinFieldSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        value: f64,
        range: BigSpinRange,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
            range,
            digits: 0,
            width_chars: 4,
            unit: None,
        }
    }

    /// Configure the `digits` setting and return the updated builder.
    ///
    /// The supplied `digits` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigInlineSpinFieldSpec`].
    #[must_use]
    pub fn digits(mut self, digits: u32) -> Self {
        self.digits = digits;
        self
    }

    /// Configure the `width_chars` setting and return the updated builder.
    ///
    /// The supplied `width_chars` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigInlineSpinFieldSpec`].
    #[must_use]
    pub fn width_chars(mut self, width_chars: i32) -> Self {
        self.width_chars = width_chars.max(1);
        self
    }

    /// Configure the `unit` setting and return the updated builder.
    ///
    /// The supplied `unit` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigInlineSpinFieldSpec`].
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }
}

/// Horizontal cluster of labeled spin buttons; used for fields like
/// "width × height" where the user edits several numbers inline.
#[derive(Debug, Clone)]
pub struct BigInlineSpinSet {
    root: gtk::Box,
    spins: Vec<(String, gtk::SpinButton)>,
}

impl BigInlineSpinSet {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spin_fields: &[BigInlineSpinFieldSpec]) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(DEFAULT_SPACING)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        let mut spins = Vec::with_capacity(spin_fields.len());

        for spin_field in spin_fields {
            let row = spin_field_row(spin_field);
            let spin = spin_field_button(spin_field);
            row.append(&spin);
            if let Some(unit) = spin_field.unit.as_deref() {
                let unit_label = gtk::Label::builder()
                    .label(unit)
                    .css_classes(["dim-label"])
                    .build();
                row.append(&unit_label);
            }
            root.append(&row);
            spins.push((spin_field.id.clone(), spin));
        }

        let accessible_label = spin_fields
            .iter()
            .map(|spin_field| spin_field.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if !accessible_label.is_empty() {
            root.set_accessible_role(gtk::AccessibleRole::Group);
            root.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);
        }

        Self { root, spins }
    }

    /// Return a reference to the `root` exposed by this [`BigInlineSpinSet`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `spins` exposed by this [`BigInlineSpinSet`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn spins(&self) -> &[(String, gtk::SpinButton)] {
        &self.spins
    }

    /// Return the current `spin` value held by this [`BigInlineSpinSet`].
    #[must_use]
    pub fn spin(&self, id: &str) -> Option<&gtk::SpinButton> {
        self.spins
            .iter()
            .find_map(|(candidate, spin)| (candidate == id).then_some(spin))
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, Vec<(String, gtk::SpinButton)>) {
        (self.root, self.spins)
    }
}

fn spin_field_row(spin_field: &BigInlineSpinFieldSpec) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(DEFAULT_SPIN_FIELD_SPACING)
        .valign(gtk::Align::Center)
        .build();
    let label = gtk::Label::builder().label(&spin_field.label).build();
    row.append(&label);
    row
}

fn spin_field_button(spin_field: &BigInlineSpinFieldSpec) -> gtk::SpinButton {
    let adjustment = gtk::Adjustment::new(
        spin_field.value,
        spin_field.range.min,
        spin_field.range.max,
        spin_field.range.step,
        spin_field.range.page_step,
        0.0,
    );
    let spin = gtk::SpinButton::builder()
        .adjustment(&adjustment)
        .digits(spin_field.digits)
        .numeric(true)
        .width_chars(spin_field.width_chars)
        .build();
    spin.update_property(&[gtk::accessible::Property::Label(&spin_field.label)]);
    tooltip::set(&spin, &spin_field.label);
    spin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spin_field_builder_records_unit_and_width() {
        let spin_field = BigInlineSpinFieldSpec::new(
            "delay",
            "Delay",
            100.0,
            BigSpinRange::new(0.0, 1000.0, 50.0, 100.0),
        )
        .width_chars(0)
        .unit("ms");

        assert_eq!(spin_field.width_chars, 1);
        assert_eq!(spin_field.unit.as_deref(), Some("ms"));
    }
}
