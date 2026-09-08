// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Compact vertical scale popover for toolbar controls.

use relm4::gtk;
use relm4::gtk::prelude::*;

const DEFAULT_MARGIN_HORIZONTAL: i32 = 8;
const DEFAULT_MARGIN_VERTICAL: i32 = 6;
const DEFAULT_SPACING: i32 = 6;
const DEFAULT_HEIGHT: i32 = 160;

/// Display-free specification describing big vertical scale popover behaviour.
#[derive(Debug, Clone, PartialEq)]
pub struct BigVerticalScalePopoverSpec {
    /// Title.
    pub title: String,
    /// Accessible label.
    pub accessible_label: String,
    /// Minimum.
    pub minimum: f64,
    /// Maximum.
    pub maximum: f64,
    /// Step.
    pub step: f64,
    /// Initial.
    pub initial: f64,
    /// Height request.
    pub height_request: i32,
    /// Inverted.
    pub inverted: bool,
    /// Spacing.
    pub spacing: i32,
}

impl BigVerticalScalePopoverSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>, accessible_label: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            accessible_label: accessible_label.into(),
            minimum: 0.0,
            maximum: 1.0,
            step: 0.1,
            initial: 0.0,
            height_request: DEFAULT_HEIGHT,
            inverted: true,
            spacing: DEFAULT_SPACING,
        }
    }

    /// Configure the `range` setting and return the updated builder.
    ///
    /// The supplied `minimum` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigVerticalScalePopoverSpec`].
    #[must_use]
    pub fn range(mut self, minimum: f64, maximum: f64, step: f64) -> Self {
        self.minimum = minimum;
        self.maximum = maximum;
        self.step = step;
        self
    }

    /// Configure the `initial` setting and return the updated builder.
    ///
    /// The supplied `initial` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigVerticalScalePopoverSpec`].
    #[must_use]
    pub fn initial(mut self, initial: f64) -> Self {
        self.initial = initial;
        self
    }

    /// Configure the `height_request` setting and return the updated builder.
    ///
    /// The supplied `height_request` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigVerticalScalePopoverSpec`].
    #[must_use]
    pub fn height_request(mut self, height_request: i32) -> Self {
        self.height_request = height_request;
        self
    }
}

/// Build a popover containing a vertical [`gtk::Scale`] anchored to
/// `parent`. The returned pair is `(popover, scale)`; the caller
/// owns connecting `value-changed` and showing/hiding the popover.
pub fn vertical_scale_popover(
    parent: &impl IsA<gtk::Widget>,
    spec: BigVerticalScalePopoverSpec,
) -> (gtk::Popover, gtk::Scale) {
    let resolved = resolve_spec(spec);
    let popover = gtk::Popover::new();
    popover.set_parent(parent.as_ref());
    crate::feedback::popover_lifecycle::unparent_popover_on_parent_teardown(
        parent.as_ref().upcast_ref(),
        &popover,
    );
    popover.set_position(gtk::PositionType::Top);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(resolved.spacing)
        .margin_start(DEFAULT_MARGIN_HORIZONTAL)
        .margin_end(DEFAULT_MARGIN_HORIZONTAL)
        .margin_top(DEFAULT_MARGIN_VERTICAL)
        .margin_bottom(DEFAULT_MARGIN_VERTICAL)
        .build();
    let title = gtk::Label::builder()
        .label(&resolved.title)
        .css_classes(["caption"])
        .build();
    let scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Vertical)
        .draw_value(false)
        .inverted(resolved.inverted)
        .height_request(resolved.height_request)
        .build();
    scale.set_range(resolved.minimum, resolved.maximum);
    scale.set_increments(resolved.step, resolved.step);
    scale.set_value(resolved.initial);
    scale.update_property(&[gtk::accessible::Property::Label(&resolved.accessible_label)]);

    content.append(&title);
    content.append(&scale);
    popover.set_child(Some(&content));

    (popover, scale)
}

fn resolve_spec(mut spec: BigVerticalScalePopoverSpec) -> BigVerticalScalePopoverSpec {
    if !spec.minimum.is_finite() {
        spec.minimum = 0.0;
    }
    if !spec.maximum.is_finite() || spec.maximum <= spec.minimum {
        spec.maximum = spec.minimum + 1.0;
    }
    if !spec.step.is_finite() || spec.step <= 0.0 {
        spec.step = 0.1;
    }
    if !spec.initial.is_finite() {
        spec.initial = spec.minimum;
    }
    spec.initial = spec.initial.clamp(spec.minimum, spec.maximum);
    if spec.height_request <= 0 {
        spec.height_request = DEFAULT_HEIGHT;
    }
    if spec.spacing < 0 {
        spec.spacing = DEFAULT_SPACING;
    }
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_scale_spec_clamps_invalid_range_and_initial() {
        let resolved = resolve_spec(
            BigVerticalScalePopoverSpec::new("Zoom", "Waveform zoom")
                .range(10.0, 1.0, -1.0)
                .initial(f64::NAN)
                .height_request(-10),
        );

        assert_eq!(resolved.minimum, 10.0);
        assert_eq!(resolved.maximum, 11.0);
        assert_eq!(resolved.step, 0.1);
        assert_eq!(resolved.initial, 10.0);
        assert_eq!(resolved.height_request, DEFAULT_HEIGHT);
    }
}
