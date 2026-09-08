// SPDX-License-Identifier: MIT

//! Color picker spec.
//!
//! Captures the data side of the picker: hex / RGB representation,
//! optional alpha, swatch presets, and validation. The widget builder
//! turns the spec into `gtk::ColorDialogButton` or a custom palette
//! depending on `BigColorPickerMode`.

use relm4::{adw, gtk};

use adw::prelude::*;

/// Enumeration of supported big color picker mode variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigColorPickerMode {
    /// Full color dialog (`gtk::ColorDialog`).
    Full,
    /// Palette-only popover with built-in swatches.
    PaletteOnly,
    /// Inline color row (icon + chip).
    Inline,
}

/// RGBA color, components in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BigRgba {
    /// Red.
    pub red: f32,
    /// Green.
    pub green: f32,
    /// Blue.
    pub blue: f32,
    /// Alpha.
    pub alpha: f32,
}

impl Default for BigRgba {
    fn default() -> Self {
        Self {
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
        }
    }
}

impl BigRgba {
    /// Construct from `#RRGGBB` or `#RRGGBBAA` strings (with or without `#`).
    ///
    /// # Errors
    /// Returns the offending hex string when length or parsing fails.
    pub fn from_hex(hex: &str) -> Result<Self, String> {
        let s = hex.trim_start_matches('#');
        let parse = |slice: &str| -> Result<f32, String> {
            u8::from_str_radix(slice, 16)
                .map(|v| f32::from(v) / 255.0)
                .map_err(|_| format!("invalid hex byte: {slice}"))
        };
        match s.len() {
            6 => Ok(Self {
                red: parse(&s[0..2])?,
                green: parse(&s[2..4])?,
                blue: parse(&s[4..6])?,
                alpha: 1.0,
            }),
            8 => Ok(Self {
                red: parse(&s[0..2])?,
                green: parse(&s[2..4])?,
                blue: parse(&s[4..6])?,
                alpha: parse(&s[6..8])?,
            }),
            _ => Err(format!("invalid hex length: {hex}")),
        }
    }

    /// Render as `#RRGGBBAA` (always with alpha).
    #[must_use]
    pub fn to_hex(self) -> String {
        let to_byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            to_byte(self.red),
            to_byte(self.green),
            to_byte(self.blue),
            to_byte(self.alpha),
        )
    }

    /// Render as `#RRGGBB` (alpha dropped). Useful for CSS that does
    /// not need transparency.
    #[must_use]
    pub fn to_hex_rgb(self) -> String {
        let to_byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!(
            "#{:02X}{:02X}{:02X}",
            to_byte(self.red),
            to_byte(self.green),
            to_byte(self.blue),
        )
    }

    /// Render as lowercase `#rrggbb` (alpha dropped). GTK CSS and many app
    /// configs store lowercase hex and do case-sensitive string comparisons, so
    /// prefer this over [`to_hex_rgb`](Self::to_hex_rgb) for those.
    #[must_use]
    pub fn to_hex_rgb_lower(self) -> String {
        self.to_hex_rgb().to_lowercase()
    }

    /// Convert to a GDK `RGBA` for GTK color widgets.
    #[must_use]
    pub fn to_gdk_rgba(self) -> gtk::gdk::RGBA {
        gtk::gdk::RGBA::new(self.red, self.green, self.blue, self.alpha)
    }

    /// Build from a GDK `RGBA`.
    #[must_use]
    pub fn from_gdk_rgba(rgba: gtk::gdk::RGBA) -> Self {
        Self {
            red: rgba.red(),
            green: rgba.green(),
            blue: rgba.blue(),
            alpha: rgba.alpha(),
        }
    }
}

/// Color picker spec for libadwaita `ColorDialog` + optional eye-dropper.
///
/// # Capabilities
///
/// `color-picker`, `color-picker+alpha`, `color-picker+eyedropper`
///
/// # Archetypes
///
/// `editor`, `control-center`, `screenshot`, `media-converter`
#[derive(Debug, Clone, PartialEq)]
pub struct BigColorPickerSpec {
    /// Mode.
    pub mode: BigColorPickerMode,
    /// Initial.
    pub initial: BigRgba,
    /// Optional swatch palette shown alongside the dialog.
    pub palette: Vec<BigRgba>,
    /// `true` enables the alpha slider.
    pub allow_alpha: bool,
    /// `true` enables the eye-dropper (screen color picker).
    pub allow_eyedropper: bool,
}

const DEFAULT_HEX_ENTRY_WIDTH_CHARS: i32 = 9;
const DEFAULT_ROW_SPACING: i32 = 6;
const DEFAULT_CELL_SPACING: i32 = 2;
const DEFAULT_CELL_ROW_SPACING: i32 = 4;

/// Data used to build a hex color editor row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigHexColorRowSpec {
    /// Row title.
    pub title: String,
    /// Initial hex value.
    pub initial_hex: String,
    /// Accessible suffix for the color dialog button.
    pub picker_accessible_suffix: String,
    /// Accessible suffix for the hex entry.
    pub entry_accessible_suffix: String,
    /// Entry width in characters.
    pub entry_width_chars: i32,
    /// Space between the entry and color button.
    pub spacing: i32,
    /// Allow libadwaita markup in the row title.
    pub allow_markup: bool,
}

impl BigHexColorRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>, initial_hex: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            initial_hex: initial_hex.into(),
            picker_accessible_suffix: "color picker".to_owned(),
            entry_accessible_suffix: "hex value".to_owned(),
            entry_width_chars: DEFAULT_HEX_ENTRY_WIDTH_CHARS,
            spacing: DEFAULT_ROW_SPACING,
            allow_markup: false,
        }
    }

    /// Configure localized accessible suffixes for the picker and entry.
    #[must_use]
    pub fn accessible_suffixes(
        mut self,
        picker_suffix: impl Into<String>,
        entry_suffix: impl Into<String>,
    ) -> Self {
        self.picker_accessible_suffix = picker_suffix.into();
        self.entry_accessible_suffix = entry_suffix.into();
        self
    }

    /// Configure the entry width in characters.
    #[must_use]
    pub fn entry_width_chars(mut self, entry_width_chars: i32) -> Self {
        self.entry_width_chars = entry_width_chars;
        self
    }

    /// Configure spacing between controls.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Allow libadwaita markup in the row title.
    ///
    /// Default is plain text; strings are escaped by libadwaita's row title
    /// rendering before they reach markup.
    #[must_use]
    pub fn allow_markup(mut self) -> Self {
        self.allow_markup = true;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigHexColorRowResolved {
        BigHexColorRowResolved {
            title: self.title.clone(),
            initial_hex: self.initial_hex.clone(),
            picker_accessible_label: accessible_label(&self.title, &self.picker_accessible_suffix),
            entry_accessible_label: accessible_label(&self.title, &self.entry_accessible_suffix),
            entry_width_chars: self.entry_width_chars.max(1),
            spacing: self.spacing.max(0),
            allow_markup: self.allow_markup,
        }
    }
}

/// Pure resolved hex color row contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigHexColorRowResolved {
    /// Row title.
    pub title: String,
    /// Initial hex value.
    pub initial_hex: String,
    /// Accessible label for the color dialog button.
    pub picker_accessible_label: String,
    /// Accessible label for the hex entry.
    pub entry_accessible_label: String,
    /// Entry width in characters.
    pub entry_width_chars: i32,
    /// Space between controls.
    pub spacing: i32,
    /// Allow libadwaita markup in the row title.
    pub allow_markup: bool,
}

/// Built libadwaita row with a synchronized hex entry and color button.
#[derive(Debug, Clone)]
pub struct BigHexColorRow {
    root: adw::ActionRow,
    entry: gtk::Entry,
}

impl BigHexColorRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigHexColorRowSpec) -> Self {
        let resolved = spec.resolved();
        let root = adw::ActionRow::builder()
            .title(resolved.title.as_str())
            .use_markup(resolved.allow_markup)
            .build();
        let controls = build_hex_color_controls(
            &resolved.initial_hex,
            resolved.entry_width_chars,
            Some(resolved.picker_accessible_label.as_str()),
            Some(resolved.entry_accessible_label.as_str()),
        );
        let suffix = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(resolved.spacing)
            .build();
        suffix.append(&controls.entry);
        suffix.append(&controls.button);
        root.add_suffix(&suffix);
        root.set_activatable_widget(Some(&controls.entry));

        Self {
            root,
            entry: controls.entry,
        }
    }

    /// Return a reference to the root row.
    #[must_use]
    pub fn root(&self) -> &adw::ActionRow {
        &self.root
    }

    /// Return a reference to the hex entry.
    #[must_use]
    pub fn entry(&self) -> &gtk::Entry {
        &self.entry
    }

    /// Return the normalized lowercase `#rrggbb` value when valid.
    #[must_use]
    pub fn hex(&self) -> Option<String> {
        normalized_hex(self.entry.text().as_str())
    }

    /// Consume `self` and yield the root row.
    #[must_use]
    pub fn into_root(self) -> adw::ActionRow {
        self.root
    }
}

/// Data used to build a compact hex color grid cell.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigHexColorCellSpec {
    /// Cell label.
    pub title: String,
    /// Initial hex value.
    pub initial_hex: String,
    /// Accessible suffix for the color dialog button.
    pub picker_accessible_suffix: String,
    /// Accessible suffix for the hex entry.
    pub entry_accessible_suffix: String,
    /// Entry width in characters.
    pub entry_width_chars: i32,
    /// Space between label and control row.
    pub spacing: i32,
    /// Space between entry and color button.
    pub row_spacing: i32,
}

impl BigHexColorCellSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>, initial_hex: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            initial_hex: initial_hex.into(),
            picker_accessible_suffix: "color picker".to_owned(),
            entry_accessible_suffix: "hex value".to_owned(),
            entry_width_chars: 8,
            spacing: DEFAULT_CELL_SPACING,
            row_spacing: DEFAULT_CELL_ROW_SPACING,
        }
    }

    /// Configure localized accessible suffixes for the picker and entry.
    #[must_use]
    pub fn accessible_suffixes(
        mut self,
        picker_suffix: impl Into<String>,
        entry_suffix: impl Into<String>,
    ) -> Self {
        self.picker_accessible_suffix = picker_suffix.into();
        self.entry_accessible_suffix = entry_suffix.into();
        self
    }

    /// Configure the entry width in characters.
    #[must_use]
    pub fn entry_width_chars(mut self, entry_width_chars: i32) -> Self {
        self.entry_width_chars = entry_width_chars;
        self
    }

    /// Configure spacing between label and control row.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Configure spacing between entry and color button.
    #[must_use]
    pub fn row_spacing(mut self, row_spacing: i32) -> Self {
        self.row_spacing = row_spacing;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigHexColorCellResolved {
        BigHexColorCellResolved {
            title: self.title.clone(),
            initial_hex: self.initial_hex.clone(),
            picker_accessible_label: accessible_label(&self.title, &self.picker_accessible_suffix),
            entry_accessible_label: accessible_label(&self.title, &self.entry_accessible_suffix),
            entry_width_chars: self.entry_width_chars.max(1),
            spacing: self.spacing.max(0),
            row_spacing: self.row_spacing.max(0),
        }
    }
}

/// Pure resolved hex color cell contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigHexColorCellResolved {
    /// Cell label.
    pub title: String,
    /// Initial hex value.
    pub initial_hex: String,
    /// Accessible label for the color dialog button.
    pub picker_accessible_label: String,
    /// Accessible label for the hex entry.
    pub entry_accessible_label: String,
    /// Entry width in characters.
    pub entry_width_chars: i32,
    /// Space between label and control row.
    pub spacing: i32,
    /// Space between entry and color button.
    pub row_spacing: i32,
}

/// Built compact color grid cell with a synchronized hex entry and color button.
#[derive(Debug, Clone)]
pub struct BigHexColorCell {
    root: gtk::Box,
    entry: gtk::Entry,
}

impl BigHexColorCell {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigHexColorCellSpec) -> Self {
        let resolved = spec.resolved();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(resolved.spacing)
            .build();
        let label = gtk::Label::builder()
            .label(resolved.title.as_str())
            .halign(gtk::Align::Start)
            .css_classes(["caption"])
            .build();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(resolved.row_spacing)
            .build();
        let controls = build_hex_color_controls(
            &resolved.initial_hex,
            resolved.entry_width_chars,
            Some(resolved.picker_accessible_label.as_str()),
            Some(resolved.entry_accessible_label.as_str()),
        );
        row.append(&controls.entry);
        row.append(&controls.button);
        root.append(&label);
        root.append(&row);

        Self {
            root,
            entry: controls.entry,
        }
    }

    /// Return a reference to the root cell container.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the hex entry.
    #[must_use]
    pub fn entry(&self) -> &gtk::Entry {
        &self.entry
    }

    /// Return the normalized lowercase `#rrggbb` value when valid.
    #[must_use]
    pub fn hex(&self) -> Option<String> {
        normalized_hex(self.entry.text().as_str())
    }

    /// Consume `self` and yield the root container.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

struct HexColorControls {
    entry: gtk::Entry,
    button: gtk::ColorDialogButton,
}

fn build_hex_color_controls(
    initial_hex: &str,
    entry_width_chars: i32,
    picker_accessible_label: Option<&str>,
    entry_accessible_label: Option<&str>,
) -> HexColorControls {
    let entry = gtk::Entry::builder()
        .max_length(9)
        .width_chars(entry_width_chars.max(1))
        .css_classes(["monospace"])
        .valign(gtk::Align::Center)
        .build();
    let dialog = gtk::ColorDialog::builder().with_alpha(false).build();
    let button = gtk::ColorDialogButton::builder()
        .dialog(&dialog)
        .valign(gtk::Align::Center)
        .build();

    if let Some(label) = picker_accessible_label {
        button.update_property(&[gtk::accessible::Property::Label(label)]);
    }
    if let Some(label) = entry_accessible_label {
        entry.update_property(&[gtk::accessible::Property::Label(label)]);
    }

    if let Some(rgba) = parse_hex_color(initial_hex) {
        button.set_rgba(&rgba);
        entry.set_text(&rgba_to_lower_hex(rgba));
    } else {
        entry.set_text(initial_hex);
    }

    let entry_for_button = entry.clone();
    button.connect_rgba_notify(move |btn| {
        let hex = rgba_to_lower_hex(btn.rgba());
        if entry_for_button.text() != hex.as_str() {
            entry_for_button.set_text(&hex);
        }
    });

    let button_weak = button.downgrade();
    entry.connect_changed(move |entry| {
        let text = entry.text();
        if let Some(rgba) = parse_hex_color(text.as_str()) {
            entry.remove_css_class("error");
            if let Some(button) = button_weak.upgrade()
                && button.rgba() != rgba
            {
                button.set_rgba(&rgba);
            }
        } else {
            entry.add_css_class("error");
        }
    });

    HexColorControls { entry, button }
}

fn parse_hex_color(hex: &str) -> Option<gtk::gdk::RGBA> {
    gtk::gdk::RGBA::parse(hex.trim()).ok()
}

fn rgba_to_lower_hex(rgba: gtk::gdk::RGBA) -> String {
    BigRgba::from_gdk_rgba(rgba).to_hex_rgb_lower()
}

fn normalized_hex(hex: &str) -> Option<String> {
    parse_hex_color(hex).map(rgba_to_lower_hex)
}

fn accessible_label(title: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        title.to_owned()
    } else {
        format!("{title}, {suffix}")
    }
}

impl Default for BigColorPickerSpec {
    fn default() -> Self {
        Self {
            mode: BigColorPickerMode::Full,
            initial: BigRgba::default(),
            palette: Vec::new(),
            allow_alpha: true,
            allow_eyedropper: true,
        }
    }
}

impl BigColorPickerSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(initial: BigRgba) -> Self {
        Self {
            initial,
            ..Self::default()
        }
    }

    /// Builder: sets mode.
    #[must_use]
    pub fn with_mode(mut self, mode: BigColorPickerMode) -> Self {
        self.mode = mode;
        self
    }

    /// Builder: sets palette.
    #[must_use]
    pub fn with_palette(mut self, palette: Vec<BigRgba>) -> Self {
        self.palette = palette;
        self
    }

    /// Configure the `without_alpha` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigColorPickerSpec`].
    #[must_use]
    pub fn without_alpha(mut self) -> Self {
        self.allow_alpha = false;
        self
    }

    /// Configure the `without_eyedropper` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigColorPickerSpec`].
    #[must_use]
    pub fn without_eyedropper(mut self) -> Self {
        self.allow_eyedropper = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_hex_parses_six_digits() {
        let c = BigRgba::from_hex("#FF8800").unwrap();
        assert!((c.red - 1.0).abs() < 1e-3);
        assert!((c.green - 0.533).abs() < 1e-2);
        assert!((c.blue - 0.0).abs() < 1e-3);
        assert!((c.alpha - 1.0).abs() < 1e-3);
    }

    #[test]
    fn rgba_hex_parses_eight_digits() {
        let c = BigRgba::from_hex("FF8800CC").unwrap();
        assert!((c.alpha - 0.8).abs() < 1e-2);
    }

    #[test]
    fn invalid_hex_length_rejected() {
        assert!(BigRgba::from_hex("#FFF").is_err());
        assert!(BigRgba::from_hex("xyz").is_err());
    }

    #[test]
    fn invalid_hex_byte_rejected() {
        assert!(BigRgba::from_hex("#ZZFFFF").is_err());
    }

    #[test]
    fn hex_round_trip_preserves_values() {
        let c = BigRgba::from_hex("#FF8800CC").unwrap();
        let back = BigRgba::from_hex(&c.to_hex()).unwrap();
        assert!((back.red - c.red).abs() < 1e-3);
        assert!((back.alpha - c.alpha).abs() < 1e-3);
    }

    #[test]
    fn hex_rgb_drops_alpha() {
        let c = BigRgba::from_hex("#FF8800CC").unwrap();
        assert_eq!(c.to_hex_rgb(), "#FF8800");
    }

    #[test]
    fn hex_rgb_lower_is_lowercase_no_alpha() {
        let c = BigRgba::from_hex("#FF8800CC").unwrap();
        assert_eq!(c.to_hex_rgb_lower(), "#ff8800");
    }

    #[test]
    fn gdk_round_trip_and_lower_hex_match_legacy() {
        // Parity with the per-app `rgba_to_hex` it replaces: clamp*255 round,
        // lowercase `#rrggbb`. gdk RGBA -> BigRgba -> hex must be byte-stable.
        let rgba = gtk::gdk::RGBA::new(1.0, 0.533_333_3, 0.0, 1.0);
        let c = BigRgba::from_gdk_rgba(rgba);
        assert_eq!(c.to_hex_rgb_lower(), "#ff8800");
        let back = c.to_gdk_rgba();
        assert!((back.red() - rgba.red()).abs() < 1e-6);
        assert!((back.green() - rgba.green()).abs() < 1e-6);
        assert!((back.blue() - rgba.blue()).abs() < 1e-6);
    }

    #[test]
    fn spec_palette_carries_through() {
        let palette = vec![
            BigRgba::from_hex("#000000").unwrap(),
            BigRgba::from_hex("#FFFFFF").unwrap(),
        ];
        let spec = BigColorPickerSpec::new(BigRgba::default()).with_palette(palette.clone());
        assert_eq!(spec.palette.len(), 2);
    }

    #[test]
    fn alpha_and_eyedropper_can_be_disabled() {
        let spec = BigColorPickerSpec::default()
            .without_alpha()
            .without_eyedropper();
        assert!(!spec.allow_alpha);
        assert!(!spec.allow_eyedropper);
    }

    #[test]
    fn hex_color_row_resolves_accessible_labels_and_width() {
        let resolved = BigHexColorRowSpec::new("Foreground", "#ffffff")
            .accessible_suffixes("selector de cor", "valor hexadecimal")
            .entry_width_chars(-4)
            .spacing(-1)
            .resolved();

        assert_eq!(resolved.initial_hex, "#ffffff");
        assert_eq!(
            resolved.picker_accessible_label,
            "Foreground, selector de cor"
        );
        assert_eq!(
            resolved.entry_accessible_label,
            "Foreground, valor hexadecimal"
        );
        assert_eq!(resolved.entry_width_chars, 1);
        assert_eq!(resolved.spacing, 0);
    }

    #[test]
    fn hex_color_cell_resolves_accessible_labels_and_spacing() {
        let resolved = BigHexColorCellSpec::new("Bright Red", "#ff0000")
            .accessible_suffixes("color picker", "hex value")
            .entry_width_chars(0)
            .spacing(-2)
            .row_spacing(-3)
            .resolved();

        assert_eq!(resolved.picker_accessible_label, "Bright Red, color picker");
        assert_eq!(resolved.entry_accessible_label, "Bright Red, hex value");
        assert_eq!(resolved.entry_width_chars, 1);
        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.row_spacing, 0);
    }

    #[test]
    fn accessible_label_omits_empty_suffix() {
        assert_eq!(accessible_label("Cursor", ""), "Cursor");
    }
}
