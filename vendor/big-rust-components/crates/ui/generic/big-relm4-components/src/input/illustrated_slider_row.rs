// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Illustrated slider row with value label and reset button.

use adw::prelude::*;
use relm4::gtk;

use crate::feedback::tooltip;

const DEFAULT_IMAGE_SIZE: i32 = 56;
const DEFAULT_SCALE_WIDTH: i32 = 200;
const DEFAULT_VALUE_WIDTH_CHARS: i32 = 5;
const DEFAULT_RESET_ICON: &str = "edit-undo-symbolic";

/// Data needed to build an illustrated slider row.
#[derive(Debug, Clone, PartialEq)]
pub struct BigIllustratedSliderRowSpec {
    /// Resource prefix.
    pub resource_prefix: String,
    /// Illustration name.
    pub illustration_name: Option<String>,
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Min.
    pub min: f64,
    /// Max.
    pub max: f64,
    /// Default.
    pub default: f64,
    /// Initial.
    pub initial: f64,
    /// Step.
    pub step: f64,
    /// Page step.
    pub page_step: f64,
    /// Digits.
    pub digits: i32,
    /// Value suffix.
    pub value_suffix: String,
    /// Value width chars.
    pub value_width_chars: i32,
    /// Scale width request.
    pub scale_width_request: i32,
    /// Image size.
    pub image_size: i32,
    /// Title heading.
    pub title_heading: bool,
    /// Mark label.
    pub mark_label: Option<String>,
    /// Reset label.
    pub reset_label: String,
    /// Reset icon name.
    pub reset_icon_name: String,
}

impl BigIllustratedSliderRowSpec {
    /// Creates a new instance.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resource_prefix: impl Into<String>,
        title: impl Into<String>,
        min: f64,
        max: f64,
        default: f64,
        initial: f64,
        step: f64,
        reset_label: impl Into<String>,
    ) -> Self {
        Self {
            resource_prefix: resource_prefix.into(),
            illustration_name: None,
            title: title.into(),
            subtitle: None,
            min,
            max,
            default,
            initial,
            step,
            page_step: step * 10.0,
            digits: 2,
            value_suffix: String::new(),
            value_width_chars: DEFAULT_VALUE_WIDTH_CHARS,
            scale_width_request: DEFAULT_SCALE_WIDTH,
            image_size: DEFAULT_IMAGE_SIZE,
            title_heading: false,
            mark_label: None,
            reset_label: reset_label.into(),
            reset_icon_name: DEFAULT_RESET_ICON.to_string(),
        }
    }

    /// Configure the `illustration_name` setting and return the updated builder.
    ///
    /// The supplied `name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustratedSliderRowSpec`].
    #[must_use]
    pub fn illustration_name(mut self, name: impl Into<String>) -> Self {
        self.illustration_name = Some(name.into());
        self
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustratedSliderRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `digits` setting and return the updated builder.
    ///
    /// The supplied `digits` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustratedSliderRowSpec`].
    #[must_use]
    pub fn digits(mut self, digits: i32) -> Self {
        self.digits = digits;
        self
    }

    /// Configure the `value_suffix` setting and return the updated builder.
    ///
    /// The supplied `suffix` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustratedSliderRowSpec`].
    #[must_use]
    pub fn value_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.value_suffix = suffix.into();
        self
    }

    /// Configure the `value_width_chars` setting and return the updated builder.
    ///
    /// The supplied `width_chars` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustratedSliderRowSpec`].
    #[must_use]
    pub fn value_width_chars(mut self, width_chars: i32) -> Self {
        self.value_width_chars = width_chars;
        self
    }

    /// Configure the `scale_width_request` setting and return the updated builder.
    ///
    /// The supplied `width_request` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustratedSliderRowSpec`].
    #[must_use]
    pub fn scale_width_request(mut self, width_request: i32) -> Self {
        self.scale_width_request = width_request;
        self
    }

    /// Configure the `title_heading` setting and return the updated builder.
    ///
    /// The supplied `title_heading` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustratedSliderRowSpec`].
    #[must_use]
    pub fn title_heading(mut self, title_heading: bool) -> Self {
        self.title_heading = title_heading;
        self
    }

    /// Configure the `mark_label` setting and return the updated builder.
    ///
    /// The supplied `mark_label` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustratedSliderRowSpec`].
    #[must_use]
    pub fn mark_label(mut self, mark_label: Option<impl Into<String>>) -> Self {
        self.mark_label = mark_label.map(Into::into);
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigIllustratedSliderRowResolved {
        let min = finite_or(self.min, 0.0);
        let max = finite_or(self.max, min).max(min);
        let default = finite_or(self.default, min).clamp(min, max);
        let initial = finite_or(self.initial, default).clamp(min, max);
        let step = finite_or(self.step, 1.0).abs().max(f64::EPSILON);
        let page_step = finite_or(self.page_step, step * 10.0).abs().max(step);
        let digits = self.digits.max(0);

        BigIllustratedSliderRowResolved {
            resource_path: self
                .illustration_name
                .as_ref()
                .map(|name| format!("{}{}", self.resource_prefix, name)),
            min,
            max,
            default,
            initial,
            step,
            page_step,
            digits,
            initial_label: format_value(initial, digits, &self.value_suffix),
            value_suffix: self.value_suffix.clone(),
            value_width_chars: self.value_width_chars.max(1),
            scale_width_request: self.scale_width_request,
            image_size: self.image_size.max(1),
            title_heading: self.title_heading,
            mark_label: self.mark_label.clone(),
            reset_label: self.reset_label.clone(),
            reset_accessible_label: reset_accessible_label(&self.reset_label, &self.title),
            reset_icon_name: self.reset_icon_name.clone(),
        }
    }
}

/// Pure row contract. Safe for no-display tests.
#[derive(Debug, Clone, PartialEq)]
pub struct BigIllustratedSliderRowResolved {
    /// Resource path.
    pub resource_path: Option<String>,
    /// Min.
    pub min: f64,
    /// Max.
    pub max: f64,
    /// Default.
    pub default: f64,
    /// Initial.
    pub initial: f64,
    /// Step.
    pub step: f64,
    /// Page step.
    pub page_step: f64,
    /// Digits.
    pub digits: i32,
    /// Initial label.
    pub initial_label: String,
    /// Value suffix.
    pub value_suffix: String,
    /// Value width chars.
    pub value_width_chars: i32,
    /// Scale width request.
    pub scale_width_request: i32,
    /// Image size.
    pub image_size: i32,
    /// Title heading.
    pub title_heading: bool,
    /// Mark label.
    pub mark_label: Option<String>,
    /// Reset label.
    pub reset_label: String,
    /// Reset accessible label.
    pub reset_accessible_label: String,
    /// Reset icon name.
    pub reset_icon_name: String,
}

/// Built row plus direct handles for app wiring.
#[derive(Debug, Clone)]
pub struct BigIllustratedSliderRow {
    root: gtk::Box,
    adjustment: gtk::Adjustment,
    scale: gtk::Scale,
    value_label: gtk::Label,
    reset_button: gtk::Button,
}

impl BigIllustratedSliderRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigIllustratedSliderRowSpec) -> Self {
        let resolved = spec.resolved();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();

        if let Some(path) = resolved.resource_path.as_deref() {
            let image = gtk::Image::from_resource(path);
            image.set_pixel_size(resolved.image_size);
            image.set_margin_top(6);
            image.set_margin_bottom(6);
            image.set_valign(gtk::Align::Center);
            root.append(&image);
        }

        let text_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(if resolved.title_heading { 4 } else { 2 })
            .hexpand(true)
            .build();
        text_box.append(&title_label(&spec.title, resolved.title_heading));

        if let Some(subtitle) = spec.subtitle.as_deref() {
            text_box.append(&subtitle_label(subtitle));
        }

        let slider_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let adjustment = gtk::Adjustment::new(
            resolved.initial,
            resolved.min,
            resolved.max,
            resolved.step,
            resolved.page_step,
            0.0,
        );
        let scale = gtk::Scale::builder()
            .adjustment(&adjustment)
            .draw_value(false)
            .hexpand(true)
            .width_request(resolved.scale_width_request)
            .build();
        scale.set_digits(resolved.digits);
        scale.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        scale.add_mark(
            resolved.default,
            gtk::PositionType::Bottom,
            resolved.mark_label.as_deref(),
        );
        slider_box.append(&scale);

        let value_label = gtk::Label::builder()
            .label(&resolved.initial_label)
            .width_chars(resolved.value_width_chars)
            .xalign(1.0)
            .build();
        value_label.add_css_class("monospace");
        slider_box.append(&value_label);

        {
            let value_label = value_label.clone();
            let suffix = resolved.value_suffix.clone();
            let digits = resolved.digits;
            adjustment.connect_value_changed(move |adjustment| {
                value_label.set_label(&format_value(adjustment.value(), digits, &suffix));
            });
        }

        let reset_button =
            tooltip::icon_button(&resolved.reset_icon_name, &resolved.reset_label, &["flat"]);
        reset_button.update_property(&[gtk::accessible::Property::Label(
            &resolved.reset_accessible_label,
        )]);
        {
            let adjustment = adjustment.clone();
            let default = resolved.default;
            reset_button.connect_clicked(move |_| adjustment.set_value(default));
        }
        slider_box.append(&reset_button);

        text_box.append(&slider_box);
        root.append(&text_box);

        Self {
            root,
            adjustment,
            scale,
            value_label,
            reset_button,
        }
    }

    /// Return a reference to the `root` exposed by this [`BigIllustratedSliderRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `adjustment` exposed by this [`BigIllustratedSliderRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn adjustment(&self) -> &gtk::Adjustment {
        &self.adjustment
    }

    /// Return a reference to the `scale` exposed by this [`BigIllustratedSliderRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn scale(&self) -> &gtk::Scale {
        &self.scale
    }

    /// Return a reference to the `value label` exposed by this [`BigIllustratedSliderRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn value_label(&self) -> &gtk::Label {
        &self.value_label
    }

    /// Return a reference to the `reset button` exposed by this [`BigIllustratedSliderRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn reset_button(&self) -> &gtk::Button {
        &self.reset_button
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        gtk::Box,
        gtk::Adjustment,
        gtk::Scale,
        gtk::Label,
        gtk::Button,
    ) {
        (
            self.root,
            self.adjustment,
            self.scale,
            self.value_label,
            self.reset_button,
        )
    }
}

fn title_label(text: &str, heading: bool) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .build();
    if heading {
        label.add_css_class("heading");
    }
    label
}

fn subtitle_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .build();
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    label
}

fn format_value(value: f64, digits: i32, suffix: &str) -> String {
    let precision = usize::try_from(digits.max(0)).unwrap_or(0);
    format!("{value:.precision$}{suffix}")
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
    fn resolved_builds_video_resource_path_and_value_label() {
        let resolved = BigIllustratedSliderRowSpec::new(
            "/app/illustrations/",
            "Subtitle Delay",
            -10.0,
            10.0,
            0.0,
            1.25,
            0.1,
            "Reset",
        )
        .illustration_name("sub_delay.svg")
        .subtitle("Shift subtitles")
        .value_suffix(" s")
        .value_width_chars(7)
        .scale_width_request(240)
        .title_heading(true)
        .mark_label(Some("0"))
        .resolved();

        assert_eq!(
            resolved.resource_path.as_deref(),
            Some("/app/illustrations/sub_delay.svg")
        );
        assert_eq!(resolved.initial_label, "1.25 s");
        assert_eq!(resolved.value_width_chars, 7);
        assert_eq!(resolved.scale_width_request, 240);
        assert!(resolved.title_heading);
        assert_eq!(resolved.mark_label.as_deref(), Some("0"));
        assert_eq!(resolved.reset_accessible_label, "Reset: Subtitle Delay");
    }

    #[test]
    fn resolved_clamps_initial_and_default() {
        let resolved = BigIllustratedSliderRowSpec::new(
            "",
            "Brightness",
            0.0,
            1.0,
            9.0,
            f64::NAN,
            0.0,
            "Reset",
        )
        .digits(-1)
        .resolved();

        assert_eq!(resolved.default, 1.0);
        assert_eq!(resolved.initial, 1.0);
        assert_eq!(resolved.initial_label, "1");
        assert!(resolved.step > 0.0);
        assert_eq!(resolved.digits, 0);
    }

    #[test]
    fn format_value_honors_zero_decimals() {
        assert_eq!(format_value(42.4, 0, ""), "42");
        assert_eq!(format_value(-1.25, 2, " dB"), "-1.25 dB");
    }

    #[test]
    fn reset_accessible_label_keeps_contextual_label() {
        assert_eq!(
            reset_accessible_label("Reset brightness", "Brightness"),
            "Reset brightness"
        );
        assert_eq!(
            reset_accessible_label("Reset", "Brightness"),
            "Reset: Brightness"
        );
    }
}
