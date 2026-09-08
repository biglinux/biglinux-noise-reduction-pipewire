//! Didactic card builder.
//!
//! Each card pairs an SVG illustration with a title, a plain-language
//! description and the actual control widget. The pattern mirrors the
//! `big-video-converter` audio dialog so users moving between the two
//! BigLinux multimedia apps see a familiar layout.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │ ┌────────┐  Title                                  [Sw]  │
//! │ │  SVG   │  Plain-language explanation that wraps         │
//! │ │  120x80 │  to the available width.                       │
//! │ └────────┘                                                │
//! │                                                           │
//! │  optional row(s): slider, dropdown, …                     │
//! └──────────────────────────────────────────────────────────┘
//! ```
//!
//! Cards stay self-contained — callers feed them controls already
//! wired to the [`AppState`](crate::ui::state::AppState) debouncer.

use std::path::PathBuf;

use super::illustration_card::{BigIllustrationCard, BigIllustrationCardSpec};
use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, Label, Orientation, Picture};

use crate::config::illustrations_dir;

/// Width of the SVG slot inside a card, in logical pixels.
const ILLUSTRATION_WIDTH: i32 = 120;
/// Height of the SVG slot inside a card, in logical pixels.
const ILLUSTRATION_HEIGHT: i32 = 80;

/// Outer container of a didactic card. Returned as a plain `GtkBox`
/// styled with the libadwaita `card` CSS class so the surrounding
/// `PreferencesPage` already knows how to render it.
pub struct DidacticCard {
    inner: BigIllustrationCard,
}

impl DidacticCard {
    /// Build a card whose top row is `[svg | title+desc | trailing]`.
    /// `trailing` is typically a `gtk::Switch` or empty placeholder.
    ///
    /// Thin wrapper over [`BigIllustrationCard`] — the card shell, header
    /// layout and row stacking live there; this supplies the
    /// `illustrations_dir` SVGs and the explicit trailing-control a11y label.
    pub fn new(svg: &str, title: &str, description: &str, trailing: Option<&gtk::Widget>) -> Self {
        let spec =
            BigIllustrationCardSpec::from_file(illustrations_dir().join(svg), title, description)
                .picture_size(ILLUSTRATION_WIDTH, ILLUSTRATION_HEIGHT);
        if let Some(w) = trailing {
            // Direct accessible label: the shared card sets only the title↔control
            // mnemonic relation, which not every screen reader surfaces as a name.
            w.update_property(&[gtk::accessible::Property::Label(title)]);
        }
        Self {
            inner: BigIllustrationCard::new_optional(spec, trailing),
        }
    }

    /// Append a labelled row to the card (slider, dropdown, …).
    pub fn add_row(&self, row: &impl IsA<gtk::Widget>) {
        self.inner.add_row(row);
    }

    /// Hand the underlying `GtkBox` to the caller for inclusion in a
    /// page layout.
    pub fn widget(&self) -> &GtkBox {
        self.inner.widget()
    }
}

/// Build a labelled control row: `[label | scale | spin button]`.
///
/// Both widgets share the same `Adjustment` so typing into the spin
/// button updates the scale (and vice versa) without explicit wiring.
/// The spin button accepts arrow-key nudges of `step_increment` and
/// allows the user to type a precise value.
pub fn slider_row(label: &str, scale: &gtk::Scale, spin: &gtk::SpinButton) -> GtkBox {
    // Reuse the cataloged shared row; keep noise's explicit accessible labels
    // (belt-and-suspenders alongside the shared label↔control mnemonic).
    scale.update_property(&[gtk::accessible::Property::Label(label)]);
    spin.update_property(&[gtk::accessible::Property::Label(label)]);
    super::illustration_card::slider_spin_row(label, scale, spin)
}

/// `[label | … | switch]` row. Pads with a flexible filler so the
/// switch keeps its native size at the trailing edge instead of being
/// stretched by `hexpand` like in [`labelled_row`].
pub fn switch_row(label: &str, switch: &gtk::Switch) -> GtkBox {
    switch.update_property(&[gtk::accessible::Property::Label(label)]);
    super::illustration_card::switch_row(label, switch)
}

/// Compose a `[label | control]` row that hosts non-slider widgets
/// (dropdowns, entries…). The caller decides the trailing widget.
pub fn labelled_row(label: &str, control: &impl IsA<gtk::Widget>) -> GtkBox {
    control
        .upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label(label)]);
    super::illustration_card::labelled_row(label, control)
}

/// `[label | scale | spin]` row driving a `0.0..=1.0` setting. The
/// underlying widgets run on a 0..100 percent scale because typing
/// `85` into the spin button feels more natural than `0.85`.
/// `[label | scale | spin]` row for a `0.0..=1.0` setting that reports changes
/// through a caller-supplied callback instead of mutating
/// [`AppState`](crate::ui::state::AppState) directly
/// — the Relm4 message path (the handler emits a typed `MicInput`; the
/// component's `update` owns the state transition). Same 0–100 percent UI as
/// [`percent_slider_on_change`].
pub fn percent_slider_on_change<F>(label: &str, initial: f32, on_change: F) -> GtkBox
where
    F: Fn(f32) + 'static,
{
    let percent = (f64::from(initial) * 100.0).clamp(0.0, 100.0);
    let adj = gtk::Adjustment::new(percent, 0.0, 100.0, 1.0, 5.0, 0.0);
    let scale = gtk::Scale::new(Orientation::Horizontal, Some(&adj));
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, 0);

    adj.connect_value_changed(move |a| {
        let v = (a.value() / 100.0).clamp(0.0, 1.0) as f32;
        on_change(v);
    });

    slider_row(label, &scale, &spin)
}

/// `[label | scale | spin]` row driving a `0..=max` `u8` setting.
pub fn u8_slider_on_change<F>(label: &str, initial: u8, max: u8, on_change: F) -> GtkBox
where
    F: Fn(u8) + 'static,
{
    let adj = gtk::Adjustment::new(f64::from(initial), 0.0, f64::from(max), 1.0, 5.0, 0.0);
    let scale = gtk::Scale::new(Orientation::Horizontal, Some(&adj));
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, 0);

    let max_f = f64::from(max);
    adj.connect_value_changed(move |a| {
        let v = a.value().round().clamp(0.0, max_f) as u8;
        on_change(v);
    });

    slider_row(label, &scale, &spin)
}

/// `[label | scale | spin]` row driving a `0..=max` `u32` setting.
/// Used by controls whose values exceed `u8::MAX` — e.g. the
/// self-listen delay (millisecond range up to a few seconds).
pub fn u32_slider_on_change<F>(label: &str, initial: u32, max: u32, on_change: F) -> GtkBox
where
    F: Fn(u32) + 'static,
{
    let adj = gtk::Adjustment::new(f64::from(initial), 0.0, f64::from(max), 1.0, 50.0, 0.0);
    let scale = gtk::Scale::new(Orientation::Horizontal, Some(&adj));
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, 0);

    let max_f = f64::from(max);
    adj.connect_value_changed(move |a| {
        let v = a.value().round().clamp(0.0, max_f) as u32;
        on_change(v);
    });

    slider_row(label, &scale, &spin)
}

/// Horizontal hairline used between logical groups inside a card.
pub fn group_separator() -> gtk::Separator {
    let sep = gtk::Separator::new(Orientation::Horizontal);
    sep.set_margin_start(16);
    sep.set_margin_end(16);
    sep.set_margin_top(8);
    sep.set_margin_bottom(8);
    sep
}

/// Standalone section header (e.g. "Microphone", "Output").
pub fn section_header(title: &str, margin_top: i32) -> Label {
    Label::builder()
        .label(title)
        .halign(Align::Start)
        .margin_top(margin_top)
        .margin_bottom(8)
        .css_classes(vec!["title-3"])
        .build()
}

/// Resolve `<illustrations_dir>/<file>` and load it into a
/// `gtk::Picture` sized to the card slot.
pub fn illustration(file: &str) -> Picture {
    let path: PathBuf = illustrations_dir().join(file);
    let picture = Picture::for_filename(&path);
    picture.set_size_request(ILLUSTRATION_WIDTH, ILLUSTRATION_HEIGHT);
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_valign(Align::Center);
    picture
}
