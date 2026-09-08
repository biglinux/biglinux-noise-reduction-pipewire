// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! A group of debounced illustrated sliders for live media adjustment
//! (brightness / contrast / saturation / gamma / hue …).
//!
//! One panel, any backend: each axis carries a [`BigIllustratedSliderRowSpec`]
//! plus an `apply` sink invoked (debounced) with the new value as the user
//! drags. The image viewer drives an image pipeline, the video player drives
//! the mpv video equalizer, the camera drives v4l2 controls — all through the
//! same panel. Labels/subtitles stay caller-supplied so each app keeps its own
//! gettext domain; this module only owns the build loop and the canonical color
//! axis range so those stay consistent across apps.

use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use relm4::gtk;

use crate::input::debounce::connect_debounced_adjustment;
use crate::input::illustrated_slider_row::{BigIllustratedSliderRow, BigIllustratedSliderRowSpec};

/// Default debounce window for a live adjustment slider (matches the image and
/// mpv apps' historical 200 ms).
pub const DEFAULT_ADJUST_DEBOUNCE: Duration = Duration::from_millis(200);

/// Default vertical spacing between sliders in a [`BigAdjustPanel`]. Override
/// with `panel.root().set_spacing(n)` after building.
pub const DEFAULT_SLIDER_SPACING: i32 = 12;

/// Canonical minimum for a normalized color axis (brightness/contrast/…).
pub const COLOR_AXIS_MIN: f64 = -100.0;
/// Canonical maximum for a normalized color axis.
pub const COLOR_AXIS_MAX: f64 = 100.0;
/// Canonical neutral (no-change) value for a normalized color axis.
pub const COLOR_AXIS_DEFAULT: f64 = 0.0;
/// Canonical step for a normalized color axis.
pub const COLOR_AXIS_STEP: f64 = 1.0;

/// One adjustment axis: the slider spec plus the sink that applies its value.
pub struct BigAdjustAxis {
    /// Slider appearance + range.
    pub spec: BigIllustratedSliderRowSpec,
    /// Debounced sink invoked with the new value while the user drags.
    pub apply: Rc<dyn Fn(f64)>,
}

impl BigAdjustAxis {
    /// Pair a slider spec with an apply sink.
    #[must_use]
    pub fn new(spec: BigIllustratedSliderRowSpec, apply: impl Fn(f64) + 'static) -> Self {
        Self {
            spec,
            apply: Rc::new(apply),
        }
    }
}

/// A canonical color axis spec (`-100..=100`, neutral `0`, step `1`, 0 digits).
///
/// Keeps brightness/contrast/saturation/hue consistent across the image and
/// video color adjusters. `title`/`subtitle` are caller-supplied (already
/// translated). Chain [`BigIllustratedSliderRowSpec::illustration_name`] if the
/// app ships an axis illustration; omit it for an icon-less slider. Axes with a
/// different domain (image gamma `0.1..=10`, v4l2 controls with per-camera
/// ranges) build their spec directly instead.
#[must_use]
pub fn color_axis_spec(
    resource_prefix: impl Into<String>,
    title: impl Into<String>,
    subtitle: impl Into<String>,
    initial: f64,
    reset_label: impl Into<String>,
) -> BigIllustratedSliderRowSpec {
    BigIllustratedSliderRowSpec::new(
        resource_prefix,
        title,
        COLOR_AXIS_MIN,
        COLOR_AXIS_MAX,
        COLOR_AXIS_DEFAULT,
        initial.clamp(COLOR_AXIS_MIN, COLOR_AXIS_MAX),
        COLOR_AXIS_STEP,
        reset_label,
    )
    .subtitle(subtitle)
    .digits(0)
}

/// A vertical group of debounced illustrated adjustment sliders.
///
/// Backend-agnostic: the caller supplies the axes (spec + sink). Each slider's
/// adjustment is wired through [`connect_debounced_adjustment`], so the apply
/// sink only fires after the user pauses — cheap to drive a live pipeline.
pub struct BigAdjustPanel {
    root: gtk::Box,
    rows: Vec<BigIllustratedSliderRow>,
}

impl BigAdjustPanel {
    /// Build a panel from `axes`, debouncing each slider with `debounce`.
    #[must_use]
    pub fn new(axes: Vec<BigAdjustAxis>, debounce: Duration) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, DEFAULT_SLIDER_SPACING);
        let mut rows = Vec::with_capacity(axes.len());
        for axis in axes {
            let row = BigIllustratedSliderRow::new(axis.spec);
            let apply = axis.apply;
            connect_debounced_adjustment(row.adjustment(), debounce, move |value| apply(value));
            root.append(row.root());
            rows.push(row);
        }
        Self { root, rows }
    }

    /// The panel's root container, ready to embed in a dialog/page.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// The built slider rows, in axis order (for reset wiring / value reads).
    #[must_use]
    pub fn rows(&self) -> &[BigIllustratedSliderRow] {
        &self.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_axis_spec_uses_canonical_range_and_clamps_initial() {
        let spec = color_axis_spec("res", "Brightness", "neutral 0", 250.0, "Reset");
        assert_eq!(spec.min, COLOR_AXIS_MIN);
        assert_eq!(spec.max, COLOR_AXIS_MAX);
        assert_eq!(spec.default, COLOR_AXIS_DEFAULT);
        assert_eq!(spec.step, COLOR_AXIS_STEP);
        assert_eq!(spec.digits, 0);
        // initial out of range is clamped into [min, max].
        assert_eq!(spec.initial, COLOR_AXIS_MAX);
        // icon-less by default; callers chain illustration_name() if they ship one.
        assert_eq!(spec.illustration_name, None);
        assert_eq!(spec.subtitle.as_deref(), Some("neutral 0"));
    }
}
