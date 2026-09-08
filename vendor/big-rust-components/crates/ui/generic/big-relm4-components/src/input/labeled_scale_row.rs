// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Labeled horizontal scale row with a live formatted value.

use adw::prelude::*;
use relm4::gtk;

/// Display-free specification describing big labeled scale row behaviour.
#[derive(Debug, Clone, PartialEq)]
pub struct BigLabeledScaleRowSpec {
    /// Title.
    pub title: String,
    /// Title width chars.
    pub title_width_chars: i32,
    /// Value width chars.
    pub value_width_chars: i32,
    /// Spacing.
    pub spacing: i32,
    /// Display multiplier.
    pub display_multiplier: f64,
    /// Suffix.
    pub suffix: String,
    /// Digits.
    pub digits: usize,
    /// Mark zero.
    pub mark_zero: bool,
    /// Scale height request.
    pub scale_height_request: Option<i32>,
}

impl BigLabeledScaleRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            title_width_chars: 8,
            value_width_chars: 7,
            spacing: 8,
            display_multiplier: 1.0,
            suffix: String::new(),
            digits: 0,
            mark_zero: false,
            scale_height_request: None,
        }
    }

    /// Configure the `title_width_chars` setting and return the updated builder.
    ///
    /// The supplied `width` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigLabeledScaleRowSpec`].
    #[must_use]
    pub fn title_width_chars(mut self, width: i32) -> Self {
        self.title_width_chars = width;
        self
    }

    /// Configure the `value_width_chars` setting and return the updated builder.
    ///
    /// The supplied `width` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigLabeledScaleRowSpec`].
    #[must_use]
    pub fn value_width_chars(mut self, width: i32) -> Self {
        self.value_width_chars = width;
        self
    }

    /// Configure the `spacing` setting and return the updated builder.
    ///
    /// The supplied `spacing` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigLabeledScaleRowSpec`].
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Configure the `display` setting and return the updated builder.
    ///
    /// The supplied `multiplier` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigLabeledScaleRowSpec`].
    #[must_use]
    pub fn display(mut self, multiplier: f64, suffix: impl Into<String>, digits: usize) -> Self {
        self.display_multiplier = multiplier;
        self.suffix = suffix.into();
        self.digits = digits;
        self
    }

    /// Configure the `mark_zero` setting and return the updated builder.
    ///
    /// The supplied `mark` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigLabeledScaleRowSpec`].
    #[must_use]
    pub fn mark_zero(mut self, mark: bool) -> Self {
        self.mark_zero = mark;
        self
    }

    /// Configure the `scale_height_request` setting and return the updated builder.
    ///
    /// The supplied `height` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigLabeledScaleRowSpec`].
    #[must_use]
    pub fn scale_height_request(mut self, height: i32) -> Self {
        self.scale_height_request = Some(height);
        self
    }

    /// Return a reference to the `value text` exposed by this [`BigLabeledScaleRowSpec`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn value_text(&self, value: f64) -> String {
        format_value(value * self.display_multiplier, self.digits, &self.suffix)
    }
}

/// Horizontal row: label · slider · value label, kept in sync via
/// the `adjustment` passed to [`Self::new`].
#[derive(Debug, Clone)]
pub struct BigLabeledScaleRow {
    row: gtk::Box,
    scale: gtk::Scale,
    value_label: gtk::Label,
}

impl BigLabeledScaleRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigLabeledScaleRowSpec, adjustment: &gtk::Adjustment) -> Self {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, spec.spacing);

        let label = gtk::Label::builder()
            .label(&spec.title)
            .halign(gtk::Align::Start)
            .width_chars(spec.title_width_chars)
            .build();
        row.append(&label);

        let scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .draw_value(false)
            .adjustment(adjustment)
            .build();
        scale.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        if spec.mark_zero {
            scale.add_mark(0.0, gtk::PositionType::Bottom, None);
        }
        if let Some(height) = spec.scale_height_request {
            scale.set_size_request(-1, height);
        }
        row.append(&scale);

        let value_label = gtk::Label::builder()
            .label(spec.value_text(adjustment.value()))
            .width_chars(spec.value_width_chars)
            .halign(gtk::Align::End)
            .build();
        let value_spec = spec.clone();
        let value_label_ref = value_label.clone();
        adjustment.connect_value_changed(move |adj| {
            value_label_ref.set_label(&value_spec.value_text(adj.value()));
        });
        row.append(&value_label);

        Self {
            row,
            scale,
            value_label,
        }
    }

    /// Return a reference to the `row` exposed by this [`BigLabeledScaleRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn row(&self) -> &gtk::Box {
        &self.row
    }

    /// Return a reference to the `scale` exposed by this [`BigLabeledScaleRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn scale(&self) -> &gtk::Scale {
        &self.scale
    }

    /// Return a reference to the `value label` exposed by this [`BigLabeledScaleRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn value_label(&self) -> &gtk::Label {
        &self.value_label
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Scale, gtk::Box, gtk::Label) {
        (self.scale, self.row, self.value_label)
    }
}

/// Format `value` with `digits` decimal places followed by `suffix`
/// (e.g. `" %"`, `" dB"`).
#[must_use]
pub fn format_value(value: f64, digits: usize, suffix: &str) -> String {
    format!("{value:.digits$}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_value_honors_digits_and_suffix() {
        assert_eq!(format_value(12.345, 0, "%"), "12%");
        assert_eq!(format_value(1.25, 1, " dB"), "1.2 dB");
    }

    #[test]
    fn spec_value_text_applies_multiplier() {
        let spec = BigLabeledScaleRowSpec::new("Strength").display(100.0, "%", 0);

        assert_eq!(spec.value_text(0.42), "42%");
    }
}
