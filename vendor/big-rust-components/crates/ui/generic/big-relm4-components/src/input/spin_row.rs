// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux action row with an accessible spin-button suffix.

use adw::prelude::*;
use relm4::gtk;

use super::preference_cards::BigSpinRange;

const DEFAULT_WIDTH_CHARS: i32 = 7;

/// Data needed to build a spin row.
#[derive(Debug, Clone, PartialEq)]
pub struct BigSpinRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Current value.
    pub value: f64,
    /// Numeric range.
    pub range: BigSpinRange,
    /// Decimal digits.
    pub digits: u32,
    /// Spin button width in characters.
    pub width_chars: i32,
}

impl BigSpinRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>, value: f64, range: BigSpinRange) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            value,
            range,
            digits: 0,
            width_chars: DEFAULT_WIDTH_CHARS,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSpinRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `digits` setting and return the updated builder.
    ///
    /// The supplied `digits` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSpinRowSpec`].
    #[must_use]
    pub fn digits(mut self, digits: u32) -> Self {
        self.digits = digits;
        self
    }

    /// Configure the `width_chars` setting and return the updated builder.
    ///
    /// The supplied `width_chars` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSpinRowSpec`].
    #[must_use]
    pub fn width_chars(mut self, width_chars: i32) -> Self {
        self.width_chars = width_chars;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigSpinRowResolved {
        let min = finite_or(self.range.min, 0.0);
        let max = finite_or(self.range.max, min).max(min);
        let step = finite_or(self.range.step, 1.0).abs().max(f64::EPSILON);
        let page_step = finite_or(self.range.page_step, step * 10.0).abs().max(step);

        BigSpinRowResolved {
            value: finite_or(self.value, min).clamp(min, max),
            min,
            max,
            step,
            page_step,
            digits: self.digits,
            width_chars: self.width_chars.max(1),
        }
    }
}

/// Pure spin row contract. Safe for no-display tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BigSpinRowResolved {
    /// Current value.
    pub value: f64,
    /// Lowest legal value.
    pub min: f64,
    /// Highest legal value.
    pub max: f64,
    /// Step increment.
    pub step: f64,
    /// Page increment.
    pub page_step: f64,
    /// Decimal digits.
    pub digits: u32,
    /// Spin button width in characters.
    pub width_chars: i32,
}

/// Built action row plus direct handle for app wiring.
#[derive(Debug, Clone)]
pub struct BigSpinRow {
    row: adw::ActionRow,
    spin: gtk::SpinButton,
}

impl BigSpinRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigSpinRowSpec) -> Self {
        let resolved = spec.resolved();
        let adjustment = gtk::Adjustment::new(
            resolved.value,
            resolved.min,
            resolved.max,
            resolved.step,
            resolved.page_step,
            0.0,
        );
        let spin = gtk::SpinButton::builder()
            .adjustment(&adjustment)
            .digits(resolved.digits)
            .numeric(true)
            .valign(gtk::Align::Center)
            .width_chars(resolved.width_chars)
            .build();
        spin.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        if let Some(subtitle) = spec.subtitle.as_deref() {
            spin.update_property(&[gtk::accessible::Property::Description(subtitle)]);
        }

        let mut row_builder = adw::ActionRow::builder().title(&spec.title);
        if let Some(subtitle) = spec.subtitle.as_deref() {
            row_builder = row_builder.subtitle(subtitle);
        }
        let row = row_builder.build();
        row.add_suffix(&spin);
        row.set_activatable_widget(Some(&spin));

        Self { row, spin }
    }

    /// Return a reference to the `row` exposed by this [`BigSpinRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn row(&self) -> &adw::ActionRow {
        &self.row
    }

    /// Return a reference to the `spin` exposed by this [`BigSpinRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn spin(&self) -> &gtk::SpinButton {
        &self.spin
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (adw::ActionRow, gtk::SpinButton) {
        (self.row, self.spin)
    }
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_clamps_value_and_width() {
        let resolved = BigSpinRowSpec::new(
            "Maximum devices",
            50.0,
            BigSpinRange::new(1.0, 20.0, 1.0, 10.0),
        )
        .width_chars(0)
        .resolved();

        assert_eq!(resolved.value, 20.0);
        assert_eq!(resolved.width_chars, 1);
    }

    #[test]
    fn resolved_normalizes_invalid_numbers() {
        let resolved = BigSpinRowSpec::new(
            "Frame rate",
            f64::NAN,
            BigSpinRange::new(f64::NAN, f64::NAN, 0.0, f64::NAN),
        )
        .resolved();

        assert_eq!(resolved.value, 0.0);
        assert_eq!(resolved.min, 0.0);
        assert_eq!(resolved.max, 0.0);
        assert!(resolved.step > 0.0);
        assert!(resolved.page_step >= resolved.step);
    }
}
