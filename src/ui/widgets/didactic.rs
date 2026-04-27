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
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, Label, Orientation, Picture};

use crate::config::{illustrations_dir, AppSettings};
use crate::ui::state::AppState;

/// Width of the SVG slot inside a card, in logical pixels.
const ILLUSTRATION_WIDTH: i32 = 120;
/// Height of the SVG slot inside a card, in logical pixels.
const ILLUSTRATION_HEIGHT: i32 = 80;

/// Outer container of a didactic card. Returned as a plain `GtkBox`
/// styled with the libadwaita `card` CSS class so the surrounding
/// `PreferencesPage` already knows how to render it.
pub struct DidacticCard {
    root: GtkBox,
    body: GtkBox,
}

impl DidacticCard {
    /// Build a card whose top row is `[svg | title+desc | trailing]`.
    /// `trailing` is typically a `gtk::Switch` or empty placeholder.
    pub fn new(svg: &str, title: &str, description: &str, trailing: Option<&gtk::Widget>) -> Self {
        let root = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .css_classes(vec!["card"])
            .margin_bottom(12)
            .build();

        let header = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(16)
            .margin_top(16)
            .margin_bottom(12)
            .margin_start(16)
            .margin_end(16)
            .build();

        header.append(&illustration(svg));

        let text = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .valign(Align::Center)
            .build();

        let title_label = Label::builder()
            .label(title)
            .halign(Align::Start)
            .wrap(true)
            .use_underline(true)
            .css_classes(vec!["heading"])
            .build();
        text.append(&title_label);

        let desc_label = Label::builder()
            .label(description)
            .wrap(true)
            .xalign(0.0)
            .css_classes(vec!["dim-label"])
            .build();
        text.append(&desc_label);

        header.append(&text);

        if let Some(w) = trailing {
            w.set_valign(Align::Center);
            // Card title acts as the accessible label for the trailing
            // control (typically a `gtk::Switch`). Without this link
            // screen readers announce a bare "switch on/off" with no
            // context.
            title_label.set_mnemonic_widget(Some(w));
            header.append(w);
        }

        root.append(&header);

        let body = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();
        root.append(&body);

        Self { root, body }
    }

    /// Append a labelled row to the card (slider, dropdown, …).
    pub fn add_row(&self, row: &impl IsA<gtk::Widget>) {
        self.body.append(row);
    }

    /// Hand the underlying `GtkBox` to the caller for inclusion in a
    /// page layout.
    pub fn widget(&self) -> &GtkBox {
        &self.root
    }
}

/// Build a labelled control row: `[label | scale | spin button]`.
///
/// Both widgets share the same `Adjustment` so typing into the spin
/// button updates the scale (and vice versa) without explicit wiring.
/// The spin button accepts arrow-key nudges of `step_increment` and
/// allows the user to type a precise value.
pub fn slider_row(label: &str, scale: &gtk::Scale, spin: &gtk::SpinButton) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(12)
        .build();

    let title = Label::builder()
        .label(label)
        .xalign(0.0)
        .width_request(140)
        .use_underline(true)
        .build();
    row.append(&title);

    scale.set_hexpand(true);
    scale.set_valign(Align::Center);
    scale.set_draw_value(false);
    title.set_mnemonic_widget(Some(scale));
    row.append(scale);

    spin.set_valign(Align::Center);
    spin.set_numeric(true);
    spin.set_width_chars(5);
    spin.add_css_class("numeric");
    row.append(spin);

    row
}

/// `[label | … | switch]` row. Pads with a flexible filler so the
/// switch keeps its native size at the trailing edge instead of being
/// stretched by `hexpand` like in [`labelled_row`].
pub fn switch_row(label: &str, switch: &gtk::Switch) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(12)
        .build();

    let title = Label::builder()
        .label(label)
        .xalign(0.0)
        .width_request(140)
        .use_underline(true)
        .build();
    title.set_mnemonic_widget(Some(switch));
    row.append(&title);

    let filler = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .hexpand(true)
        .build();
    row.append(&filler);

    switch.set_valign(Align::Center);
    switch.set_halign(Align::End);
    row.append(switch);
    row
}

/// Compose a `[label | control]` row that hosts non-slider widgets
/// (dropdowns, entries…). The caller decides the trailing widget.
pub fn labelled_row(label: &str, control: &impl IsA<gtk::Widget>) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(12)
        .build();

    let title = Label::builder()
        .label(label)
        .xalign(0.0)
        .width_request(140)
        .use_underline(true)
        .build();
    title.set_mnemonic_widget(Some(control));
    row.append(&title);

    control.set_hexpand(true);
    row.append(control);
    row
}

/// `[label | scale | spin]` row driving a `0.0..=1.0` setting. The
/// underlying widgets run on a 0..100 percent scale because typing
/// `85` into the spin button feels more natural than `0.85`.
pub fn percent_slider<F>(state: &Rc<AppState>, label: &str, initial: f32, writer: F) -> GtkBox
where
    F: Fn(&mut AppSettings, f32) + 'static,
{
    let percent = (f64::from(initial) * 100.0).clamp(0.0, 100.0);
    let adj = gtk::Adjustment::new(percent, 0.0, 100.0, 1.0, 5.0, 0.0);
    let scale = gtk::Scale::new(Orientation::Horizontal, Some(&adj));
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, 0);

    {
        let state = Rc::clone(state);
        adj.connect_value_changed(move |a| {
            let v = (a.value() / 100.0).clamp(0.0, 1.0) as f32;
            state.mutate(|cfg| writer(cfg, v));
        });
    }

    slider_row(label, &scale, &spin)
}

/// `[label | scale | spin]` row driving a `0..=max` `u8` setting.
pub fn u8_slider<F>(state: &Rc<AppState>, label: &str, initial: u8, max: u8, writer: F) -> GtkBox
where
    F: Fn(&mut AppSettings, u8) + 'static,
{
    let adj = gtk::Adjustment::new(f64::from(initial), 0.0, f64::from(max), 1.0, 5.0, 0.0);
    let scale = gtk::Scale::new(Orientation::Horizontal, Some(&adj));
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, 0);

    {
        let state = Rc::clone(state);
        let max_f = f64::from(max);
        adj.connect_value_changed(move |a| {
            let v = a.value().round().clamp(0.0, max_f) as u8;
            state.mutate(|cfg| writer(cfg, v));
        });
    }

    slider_row(label, &scale, &spin)
}

/// `[label | scale | spin]` row driving a `0..=max` `u32` setting.
/// Used by controls whose values exceed `u8::MAX` — e.g. the
/// self-listen delay (millisecond range up to a few seconds).
pub fn u32_slider<F>(state: &Rc<AppState>, label: &str, initial: u32, max: u32, writer: F) -> GtkBox
where
    F: Fn(&mut AppSettings, u32) + 'static,
{
    let adj = gtk::Adjustment::new(f64::from(initial), 0.0, f64::from(max), 1.0, 50.0, 0.0);
    let scale = gtk::Scale::new(Orientation::Horizontal, Some(&adj));
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, 0);

    {
        let state = Rc::clone(state);
        let max_f = f64::from(max);
        adj.connect_value_changed(move |a| {
            let v = a.value().round().clamp(0.0, max_f) as u32;
            state.mutate(|cfg| writer(cfg, v));
        });
    }

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
