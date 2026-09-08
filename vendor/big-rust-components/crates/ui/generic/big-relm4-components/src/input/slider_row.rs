// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Slider row with a reset action.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

const DEFAULT_WIDTH: i32 = 150;
const DEFAULT_DIGITS: i32 = 2;
const DEFAULT_RESET_ICON: &str = "edit-undo-symbolic";

/// Data needed to build a slider row.
#[derive(Debug, Clone, PartialEq)]
pub struct BigSliderRowSpec {
    /// Title.
    pub title: String,
    /// Min.
    pub min: f64,
    /// Max.
    pub max: f64,
    /// Default.
    pub default: f64,
    /// Step.
    pub step: f64,
    /// Page step.
    pub page_step: f64,
    /// Digits.
    pub digits: i32,
    /// Width request.
    pub width_request: i32,
    /// Draw value.
    pub draw_value: bool,
    /// Value position.
    pub value_position: gtk::PositionType,
    /// Reset label.
    pub reset_label: String,
    /// Reset icon name.
    pub reset_icon_name: String,
}

impl BigSliderRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        min: f64,
        max: f64,
        default: f64,
        step: f64,
        reset_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            min,
            max,
            default,
            step,
            page_step: step * 5.0,
            digits: DEFAULT_DIGITS,
            width_request: DEFAULT_WIDTH,
            draw_value: true,
            value_position: gtk::PositionType::Left,
            reset_label: reset_label.into(),
            reset_icon_name: DEFAULT_RESET_ICON.to_string(),
        }
    }

    /// Configure the `digits` setting and return the updated builder.
    ///
    /// The supplied `digits` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSliderRowSpec`].
    #[must_use]
    pub fn digits(mut self, digits: i32) -> Self {
        self.digits = digits;
        self
    }

    /// Configure the `width_request` setting and return the updated builder.
    ///
    /// The supplied `width_request` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSliderRowSpec`].
    #[must_use]
    pub fn width_request(mut self, width_request: i32) -> Self {
        self.width_request = width_request;
        self
    }

    /// Configure the `draw_value` setting and return the updated builder.
    ///
    /// The supplied `draw_value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSliderRowSpec`].
    #[must_use]
    pub fn draw_value(mut self, draw_value: bool) -> Self {
        self.draw_value = draw_value;
        self
    }

    /// Configure the `value_position` setting and return the updated builder.
    ///
    /// The supplied `position` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSliderRowSpec`].
    #[must_use]
    pub fn value_position(mut self, position: gtk::PositionType) -> Self {
        self.value_position = position;
        self
    }

    /// Configure the `reset_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSliderRowSpec`].
    #[must_use]
    pub fn reset_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.reset_icon_name = icon_name.into();
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigSliderRowResolved {
        let min = finite_or(self.min, 0.0);
        let max = finite_or(self.max, min).max(min);
        let default = finite_or(self.default, min).clamp(min, max);
        let step = finite_or(self.step, 1.0).abs().max(f64::EPSILON);
        let page_step = finite_or(self.page_step, step * 5.0).abs().max(step);

        BigSliderRowResolved {
            min,
            max,
            default,
            step,
            page_step,
            digits: self.digits.max(0),
            width_request: self.width_request,
            draw_value: self.draw_value,
            value_position: self.value_position,
            reset_label: self.reset_label.clone(),
            reset_accessible_label: reset_accessible_label(&self.reset_label, &self.title),
            reset_icon_name: self.reset_icon_name.clone(),
        }
    }
}

/// Pure slider row contract. Safe for no-display tests.
#[derive(Debug, Clone, PartialEq)]
pub struct BigSliderRowResolved {
    /// Min.
    pub min: f64,
    /// Max.
    pub max: f64,
    /// Default.
    pub default: f64,
    /// Step.
    pub step: f64,
    /// Page step.
    pub page_step: f64,
    /// Digits.
    pub digits: i32,
    /// Width request.
    pub width_request: i32,
    /// Draw value.
    pub draw_value: bool,
    /// Value position.
    pub value_position: gtk::PositionType,
    /// Reset label.
    pub reset_label: String,
    /// Reset accessible label.
    pub reset_accessible_label: String,
    /// Reset icon name.
    pub reset_icon_name: String,
}

/// Built action row plus direct handles for app wiring.
#[derive(Debug, Clone)]
pub struct BigSliderRow {
    row: adw::ActionRow,
    scale: gtk::Scale,
    reset_button: gtk::Button,
}

impl BigSliderRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigSliderRowSpec) -> Self {
        let resolved = spec.resolved();
        let row = adw::ActionRow::builder().title(&spec.title).build();

        let scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Horizontal)
            .draw_value(resolved.draw_value)
            .value_pos(resolved.value_position)
            .hexpand(true)
            .digits(resolved.digits)
            .width_request(resolved.width_request)
            .build();
        scale.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        scale.set_range(resolved.min, resolved.max);
        scale.set_value(resolved.default);
        scale.set_increments(resolved.step, resolved.page_step);

        let reset_button = tooltip::icon_button(
            &resolved.reset_icon_name,
            &resolved.reset_label,
            &["flat", "circular"],
        );
        reset_button.update_property(&[gtk::accessible::Property::Label(
            &resolved.reset_accessible_label,
        )]);
        reset_button.set_valign(gtk::Align::Center);

        let suffix = gtk::Box::builder()
            .spacing(4)
            .valign(gtk::Align::Center)
            .build();
        suffix.append(&scale);
        suffix.append(&reset_button);
        row.add_suffix(&suffix);

        Self {
            row,
            scale,
            reset_button,
        }
    }

    /// Return a reference to the `row` exposed by this [`BigSliderRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn row(&self) -> &adw::ActionRow {
        &self.row
    }

    /// Return a reference to the `scale` exposed by this [`BigSliderRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn scale(&self) -> &gtk::Scale {
        &self.scale
    }

    /// Return a reference to the `reset button` exposed by this [`BigSliderRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn reset_button(&self) -> &gtk::Button {
        &self.reset_button
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (adw::ActionRow, gtk::Scale, gtk::Button) {
        (self.row, self.scale, self.reset_button)
    }
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

fn reset_accessible_label(reset_label: &str, title: &str) -> String {
    let reset = reset_label.trim();
    let title = title.trim();
    if reset.is_empty() || title.is_empty() || reset.to_lowercase().contains(&title.to_lowercase())
    {
        reset.to_owned()
    } else {
        format!("{reset}: {title}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_clamps_invalid_defaults() {
        let resolved = BigSliderRowSpec::new("Volume", 0.0, 10.0, 99.0, 0.5, "Reset").resolved();

        assert_eq!(resolved.default, 10.0);
        assert_eq!(resolved.step, 0.5);
        assert_eq!(resolved.page_step, 2.5);
        assert!(resolved.draw_value);
        assert_eq!(resolved.value_position, gtk::PositionType::Left);
        assert_eq!(resolved.reset_accessible_label, "Reset: Volume");
    }

    #[test]
    fn resolved_sanitizes_non_finite_ranges() {
        let resolved = BigSliderRowSpec::new("Speed", f64::NAN, f64::NAN, f64::NAN, 0.0, "Reset")
            .digits(-1)
            .resolved();

        assert_eq!(resolved.min, 0.0);
        assert_eq!(resolved.max, 0.0);
        assert_eq!(resolved.default, 0.0);
        assert!(resolved.step > 0.0);
        assert_eq!(resolved.digits, 0);
    }

    #[test]
    fn resolved_keeps_value_display_policy() {
        let resolved = BigSliderRowSpec::new("Zoom", 0.0, 1.0, 0.0, 0.1, "Reset")
            .draw_value(false)
            .value_position(gtk::PositionType::Right)
            .resolved();

        assert!(!resolved.draw_value);
        assert_eq!(resolved.value_position, gtk::PositionType::Right);
    }

    #[test]
    fn reset_accessible_label_keeps_contextual_label() {
        assert_eq!(
            reset_accessible_label("Reset volume", "Volume"),
            "Reset volume"
        );
        assert_eq!(reset_accessible_label("Reset", "Volume"), "Reset: Volume");
    }
}
