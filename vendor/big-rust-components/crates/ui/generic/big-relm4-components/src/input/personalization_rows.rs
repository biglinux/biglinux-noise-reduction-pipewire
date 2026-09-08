// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Personalization pref rows: color-with-reset and font-picker.
//!
//! Both bind through [`crate::input::preference_rows::BigPreferenceStore`]'s
//! existing string accessor — no new trait method. A color is a hex string
//! (`""` = theme default); a font is a family string (`""` = theme default).
//! `on_change(None)` always means "reset to theme"; `on_change(Some(v))`
//! is a user pick. Const-data adopters reach these through
//! [`crate::input::preference_rows::BigPrefControl::Color`] /
//! `BigPrefControl::Font`, mirroring the didactic-card shape of every other
//! preference row in that module.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::gtk::{cairo, gdk, gio};

use crate::feedback::tooltip;
use crate::input::color_picker::BigRgba;
use crate::layout::didactic_card::{BigDidacticCardSpec, didactic_card};

const SWATCH_WIDTH: i32 = 28;
const SWATCH_HEIGHT: i32 = 20;
const CONTROL_SPACING: i32 = 4;
const SWATCH_RADIUS: f64 = 5.0;
const CHECKER_SQUARE: f64 = 5.0;

/// Didactic card with a color swatch + reset button. An empty hex shows a
/// CHECKERBOARD (theme default, never a solid color); a `#rrggbb`/`#rrggbbaa`
/// value shows the solid swatch. Reset always fires `on_change(None)`.
#[derive(Debug, Clone)]
pub struct BigOptionalColorRow {
    root: gtk::Box,
}

impl BigOptionalColorRow {
    /// Builds the card. `initial_hex` empty means theme default;
    /// `pick_tooltip`/`reset_tooltip` are pre-translated.
    #[must_use]
    pub fn new(
        spec: BigDidacticCardSpec,
        initial_hex: &str,
        pick_tooltip: &str,
        reset_tooltip: &str,
        on_change: impl Fn(Option<String>) + Clone + 'static,
    ) -> Self {
        let color = Rc::new(RefCell::new(parse_optional_hex(initial_hex)));

        let area = gtk::DrawingArea::builder()
            .content_width(SWATCH_WIDTH)
            .content_height(SWATCH_HEIGHT)
            .valign(gtk::Align::Center)
            .build();
        {
            let color = color.clone();
            area.set_draw_func(move |_, cr, w, h| draw_swatch(cr, w, h, *color.borrow()));
        }

        let swatch = gtk::Button::builder()
            .child(&area)
            .css_classes(["flat"])
            .valign(gtk::Align::Center)
            .build();
        tooltip::set(&swatch, pick_tooltip);
        {
            let color = color.clone();
            let area_weak = area.downgrade();
            let on_change = on_change.clone();
            swatch.connect_clicked(move |button| {
                let parent = button.root().and_downcast::<gtk::Window>();
                let dialog = gtk::ColorDialog::builder().with_alpha(false).build();
                let initial = color
                    .borrow()
                    .unwrap_or_else(|| gdk::RGBA::new(0.5, 0.5, 0.5, 1.0));
                let color = color.clone();
                let area_weak = area_weak.clone();
                let on_change = on_change.clone();
                dialog.choose_rgba(
                    parent.as_ref(),
                    Some(&initial),
                    gio::Cancellable::NONE,
                    move |result| {
                        let Ok(rgba) = result else { return };
                        *color.borrow_mut() = Some(rgba);
                        if let Some(area) = area_weak.upgrade() {
                            area.queue_draw();
                        }
                        on_change(Some(BigRgba::from_gdk_rgba(rgba).to_hex_rgb_lower()));
                    },
                );
            });
        }

        let reset = gtk::Button::builder()
            .icon_name("edit-undo-symbolic")
            .css_classes(["flat"])
            .valign(gtk::Align::Center)
            .build();
        tooltip::set(&reset, reset_tooltip);
        {
            let area_weak = area.downgrade();
            reset.connect_clicked(move |_| {
                *color.borrow_mut() = None;
                if let Some(area) = area_weak.upgrade() {
                    area.queue_draw();
                }
                on_change(None);
            });
        }

        let controls = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(CONTROL_SPACING)
            .build();
        controls.append(&reset);
        controls.append(&swatch);

        Self {
            root: didactic_card(spec, &controls),
        }
    }

    /// Return a reference to the built card root.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Consume `self` and yield the card root.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

/// Didactic card with a system font-family picker + reset button. Reset
/// always fires `on_change(None)`; a pick fires `on_change(Some(family))`.
#[derive(Debug, Clone)]
pub struct BigFontPickerRow {
    root: gtk::Box,
}

impl BigFontPickerRow {
    /// Builds the card. `initial_family` empty means theme default;
    /// `reset_tooltip` is pre-translated.
    #[must_use]
    pub fn new(
        spec: BigDidacticCardSpec,
        initial_family: &str,
        reset_tooltip: &str,
        on_change: impl Fn(Option<String>) + Clone + 'static,
    ) -> Self {
        let button = gtk::FontDialogButton::new(Some(gtk::FontDialog::new()));
        button.set_level(gtk::FontLevel::Family);
        button.set_valign(gtk::Align::Center);
        if !initial_family.is_empty() {
            button.set_font_desc(&gtk::pango::FontDescription::from_string(initial_family));
        }
        {
            let on_change = on_change.clone();
            button.connect_font_desc_notify(move |button| {
                if let Some(family) = button.font_desc().and_then(|desc| desc.family()) {
                    on_change(Some(family.to_string()));
                }
            });
        }

        let reset = gtk::Button::builder()
            .icon_name("edit-undo-symbolic")
            .css_classes(["flat"])
            .valign(gtk::Align::Center)
            .build();
        tooltip::set(&reset, reset_tooltip);
        reset.connect_clicked(move |_| on_change(None));

        let controls = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(CONTROL_SPACING)
            .build();
        controls.append(&reset);
        controls.append(&button);

        Self {
            root: didactic_card(spec, &controls),
        }
    }

    /// Return a reference to the built card root.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Consume `self` and yield the card root.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

/// Empty string is "theme default"; anything else parses as `#rrggbb`/`#rrggbbaa`.
fn parse_optional_hex(hex: &str) -> Option<gdk::RGBA> {
    if hex.is_empty() {
        None
    } else {
        BigRgba::from_hex(hex).ok().map(BigRgba::to_gdk_rgba)
    }
}

/// Draw the color swatch: a rounded rect filled with `color`, or a
/// checkerboard when `None` (theme default — never reads as "a color").
fn draw_swatch(cr: &cairo::Context, width: i32, height: i32, color: Option<gdk::RGBA>) {
    let (w, h, radius) = (f64::from(width), f64::from(height), SWATCH_RADIUS);
    rounded_rect(cr, 0.5, 0.5, w - 1.0, h - 1.0, radius);
    if let Some(rgba) = color {
        cr.set_source_rgb(
            f64::from(rgba.red()),
            f64::from(rgba.green()),
            f64::from(rgba.blue()),
        );
        let _ = cr.fill_preserve();
    } else {
        cr.set_source_rgb(0.86, 0.86, 0.86);
        let _ = cr.fill_preserve();
        cr.clip_preserve();
        cr.set_source_rgb(0.55, 0.55, 0.55);
        let square = CHECKER_SQUARE;
        let mut y = 0.0;
        let mut row = 0;
        while y < h {
            let mut x = if row % 2 == 0 { 0.0 } else { square };
            while x < w {
                cr.rectangle(x, y, square, square);
                x += 2.0 * square;
            }
            y += square;
            row += 1;
        }
        let _ = cr.fill();
        cr.reset_clip();
        rounded_rect(cr, 0.5, 0.5, w - 1.0, h - 1.0, radius);
    }
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

/// Append a rounded-rectangle subpath to `cr`.
fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -0.5 * PI, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, 0.5 * PI);
    cr.arc(x + r, y + h - r, r, 0.5 * PI, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_hex_means_theme_default() {
        assert!(parse_optional_hex("").is_none());
    }

    #[test]
    fn non_empty_hex_parses_to_rgba() {
        let rgba = parse_optional_hex("#ff8800").expect("valid hex parses");
        assert!((rgba.red() - 1.0).abs() < 1e-3);
        assert!((rgba.green() - 0.533).abs() < 1e-2);
        assert!((rgba.blue() - 0.0).abs() < 1e-3);
    }

    #[test]
    fn invalid_hex_falls_back_to_theme_default() {
        assert!(parse_optional_hex("not-a-color").is_none());
    }
}
