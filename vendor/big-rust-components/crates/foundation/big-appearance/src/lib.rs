// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! GTK-free appearance contracts for BigLinux Rust applications.
//!
//! This crate stores user-facing surface appearance settings, emits deterministic
//! scoped CSS, and exposes the shared transparency curve used by BigLinux chrome.
//! Opacity and transparency fields are percentages in the inclusive `0..=100`
//! range unless the field documentation says otherwise.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod chrome;
pub mod region;
pub mod translucent;

pub use region::{BigRegionStyleSet, BigStylePreset, BigStyleRegion};
pub use translucent::{BigPaintBand, BigPaintFill, BigPaintPlanError, BigTranslucentSurfacePlan};

use serde::{Deserialize, Serialize};

const DEFAULT_BACKGROUND_COLOR: &str = "#2a2a30";
const DEFAULT_TEXT_COLOR: &str = "#ffffff";
const DEFAULT_BORDER_COLOR: &str = "#000000";
const DEFAULT_SHADOW_COLOR: &str = "#000000";
const BOOST_FACTOR: f64 = 0.0;
const TRANSPARENCY_CURVE_EXPONENT: f64 = 1.6;
const MAX_EFFECTIVE_TRANSPARENCY: f64 = 0.8;

/// Two-color linear gradient fill for a surface background.
///
/// Gradient colors are strict `#rrggbb` values when rendered to CSS. Invalid
/// values fall back to the default surface background color.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BigLinearGradient {
    /// Start color as `#rrggbb`.
    pub from: String,
    /// End color as `#rrggbb`.
    pub to: String,
    /// Gradient direction in degrees, clamped to `0..=360`.
    pub angle_deg: u32,
}

impl Default for BigLinearGradient {
    fn default() -> Self {
        Self {
            from: DEFAULT_BACKGROUND_COLOR.to_owned(),
            to: "#36363c".to_owned(),
            angle_deg: 135,
        }
    }
}

impl BigLinearGradient {
    /// Returns this gradient with numeric fields normalized to documented ranges.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.angle_deg = self.angle_deg.min(360);
        self
    }
}

/// Surface border settings.
///
/// `opacity` is an OPACITY percentage in `0..=100`, where `0` is fully
/// transparent and `100` is fully opaque.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BigSurfaceBorder {
    /// Border width in pixels, where `0` disables the border.
    pub width: u32,
    /// Border color as `#rrggbb`; `None` follows the fallback color.
    pub color: Option<String>,
    /// Border OPACITY percentage in `0..=100`.
    pub opacity: u32,
}

impl Default for BigSurfaceBorder {
    fn default() -> Self {
        Self {
            width: 0,
            color: Some(DEFAULT_BORDER_COLOR.to_owned()),
            opacity: 0,
        }
    }
}

impl BigSurfaceBorder {
    /// Returns this border with numeric fields normalized to documented ranges.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.width = self.width.min(8);
        self.opacity = self.opacity.min(100);
        self
    }
}

/// Surface drop-shadow settings.
///
/// `opacity` is an OPACITY percentage in `0..=100`, where `0` is fully
/// transparent and `100` is fully opaque.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BigSurfaceShadow {
    /// Blur radius in pixels, where `0` disables the shadow.
    pub size: u32,
    /// Shadow color as `#rrggbb`; `None` follows the fallback color.
    pub color: Option<String>,
    /// Shadow OPACITY percentage in `0..=100`.
    pub opacity: u32,
    /// Horizontal shadow offset in pixels.
    pub offset_x: i32,
    /// Vertical shadow offset in pixels.
    pub offset_y: i32,
}

impl Default for BigSurfaceShadow {
    fn default() -> Self {
        Self {
            size: 0,
            color: Some(DEFAULT_SHADOW_COLOR.to_owned()),
            opacity: 0,
            offset_x: 0,
            offset_y: 0,
        }
    }
}

impl BigSurfaceShadow {
    /// Returns this shadow with numeric fields normalized to documented ranges.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.size = self.size.min(48);
        self.opacity = self.opacity.min(100);
        self.offset_x = self.offset_x.clamp(-32, 32);
        self.offset_y = self.offset_y.clamp(-32, 32);
        self
    }
}

/// Optional text styling for a surface.
///
/// Text color is rendered fully opaque. Font family names are CSS-quoted after
/// validation; invalid names are dropped to keep the inherited font.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BigSurfaceText {
    /// Text color as `#rrggbb`; `None` follows the fallback color.
    pub color: Option<String>,
    /// Optional font family name.
    pub font_family: Option<String>,
    /// Text size in points, clamped to `8..=18`.
    pub size: u32,
    /// Render text with bold weight.
    pub font_bold: bool,
    /// Render text with italic style.
    pub font_italic: bool,
    /// Underline rendered text.
    pub font_underline: bool,
}

impl Default for BigSurfaceText {
    fn default() -> Self {
        Self {
            color: Some(DEFAULT_TEXT_COLOR.to_owned()),
            font_family: None,
            size: 10,
            font_bold: false,
            font_italic: false,
            font_underline: false,
        }
    }
}

impl BigSurfaceText {
    /// Returns this text style with numeric fields normalized to documented ranges.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.size = self.size.clamp(8, 18);
        self
    }
}

/// Reusable surface appearance settings.
///
/// `background_opacity`, border opacity, and shadow opacity are OPACITY
/// percentages in `0..=100`, where `0` is fully transparent and `100` is fully
/// opaque.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BigSurfaceStyle {
    /// Flat background color as `#rrggbb`; `None` follows the fallback color.
    pub background_color: Option<String>,
    /// Optional two-color gradient, replacing the flat background when present.
    pub gradient: Option<BigLinearGradient>,
    /// Background OPACITY percentage in `0..=100`.
    pub background_opacity: u32,
    /// Corner radius in pixels.
    pub radius: u32,
    /// Inner padding in pixels.
    pub padding: u32,
    /// Gap between related surface items in pixels.
    pub spacing: u32,
    /// Optional text styling applied to the surface and descendants.
    pub text: Option<BigSurfaceText>,
    /// Border settings.
    pub border: BigSurfaceBorder,
    /// Drop-shadow settings.
    pub shadow: BigSurfaceShadow,
}

impl Default for BigSurfaceStyle {
    fn default() -> Self {
        Self {
            background_color: Some(DEFAULT_BACKGROUND_COLOR.to_owned()),
            gradient: None,
            background_opacity: 92,
            radius: 12,
            padding: 8,
            spacing: 8,
            text: None,
            border: BigSurfaceBorder::default(),
            shadow: BigSurfaceShadow::default(),
        }
    }
}

impl BigSurfaceStyle {
    /// Returns this surface style with numeric fields normalized to documented ranges.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.gradient = self.gradient.map(BigLinearGradient::clamped);
        self.background_opacity = self.background_opacity.min(100);
        self.radius = self.radius.min(32);
        self.padding = self.padding.min(32);
        self.spacing = self.spacing.min(32);
        self.text = self.text.map(BigSurfaceText::clamped);
        self.border = self.border.clamped();
        self.shadow = self.shadow.clamped();
        self
    }
}

/// Density preference for BigLinux surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BigDensity {
    /// Roomier spacing for pointer-first or mixed input.
    #[default]
    Comfortable,
    /// Denser spacing for information-heavy or keyboard-first views.
    Compact,
}

/// Top-level appearance settings shared across BigLinux Rust apps.
///
/// `window_transparency` and `headerbar_transparency` are TRANSPARENCY
/// percentages in `0..=100`, where `0` produces alpha `1.0` and `100` produces
/// the maximum effective transparency cap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BigAppearanceSpec {
    /// Optional accent color as `#rrggbb`.
    pub accent: Option<String>,
    /// Body/window TRANSPARENCY percentage in `0..=100`.
    pub window_transparency: u8,
    /// Headerbar/chrome TRANSPARENCY percentage in `0..=100`.
    pub headerbar_transparency: u8,
    /// Whether blur should be requested by the consumer.
    pub blur_enabled: bool,
    /// Surface density preference.
    pub density: BigDensity,
}

impl Default for BigAppearanceSpec {
    fn default() -> Self {
        Self {
            accent: None,
            window_transparency: 0,
            headerbar_transparency: 0,
            blur_enabled: false,
            density: BigDensity::Comfortable,
        }
    }
}

/// Builds deterministic CSS for `style`, scoped under `scope`.
///
/// `scope` is used as a selector prefix, for example `.app-card` or
/// `window.big-shell`. Every emitted rule begins with that scope. All opacity
/// fields in [`BigSurfaceStyle`] and nested types are OPACITY percentages in
/// `0..=100`, not transparency percentages.
#[must_use]
pub fn surface_css(scope: &str, style: &BigSurfaceStyle) -> String {
    let text_selector = format!("{scope} *");
    surface_css_with_text_selector(scope, &text_selector, style)
}

/// Builds deterministic CSS for `style`, with a dedicated text selector.
///
/// The container selector receives both container declarations and text
/// declarations so text cascades through descendants. `text_selector` receives
/// the same text declarations for explicitly-classed labels that need a
/// stronger cascade.
#[must_use]
pub fn surface_css_with_text_selector(
    container_selector: &str,
    text_selector: &str,
    style: &BigSurfaceStyle,
) -> String {
    let style = style.clone().clamped();
    let container_selector = container_selector.trim();
    let text_selector = text_selector.trim();
    let background = background_decl(&style);
    let mut css = format!(
        "{container_selector} {{\n  {background}\n  border-radius: {}px;\n  padding: {}px;\n",
        style.radius, style.padding
    );

    if style.border.width > 0 {
        let border = gtk_border_decl(
            style.border.width,
            style.border.color.as_deref(),
            style.border.opacity,
            "var(--window-fg-color)",
        )
        .expect("border width was checked");
        css.push_str(&format!("  {border}\n"));
    }

    if style.shadow.size > 0 {
        let shadow = gtk_shadow_decl(
            style.shadow.size,
            style.shadow.opacity,
            style.shadow.color.as_deref(),
            style.shadow.offset_x,
            style.shadow.offset_y,
            "black",
        )
        .expect("shadow size was checked");
        css.push_str(&format!("  {shadow}\n"));
    }

    if let Some(text) = &style.text {
        css.push_str(&text_decls(text));
    }
    css.push_str("}\n");

    if let Some(text) = &style.text {
        css.push_str(&format!("{text_selector} {{\n{}}}\n", text_decls(text)));
    }

    css
}

/// Stylesheet for the shared BigLinux tooltip popover wrapper.
#[must_use]
pub fn tooltip_surface_css(style: &BigSurfaceStyle) -> String {
    let style = style.clone().clamped();
    let container = surface_container_decls(&style);
    let text = style.text.as_ref().map(text_decls).unwrap_or_default();
    format!(
        "popover.big-tooltip {{ padding: 18px; }}\n\
         popover.big-tooltip > contents {{\n{container}}}\n\
         popover.big-tooltip label {{\n{text}}}\n\
         popover.big-tooltip b {{ font-weight: 800; }}\n\
         popover.big-tooltip small {{ opacity: 0.62; }}\n"
    )
}

/// Effective alpha for the body/window transparency setting.
///
/// This ports the BigLinux terminal transparency curve exactly with
/// `BOOST_FACTOR = 0.0`, `TRANSPARENCY_CURVE_EXPONENT = 1.6`, and
/// `MAX_EFFECTIVE_TRANSPARENCY = 0.8`.
#[must_use]
pub fn effective_body_alpha(spec: &BigAppearanceSpec) -> f64 {
    effective_alpha_from_transparency(spec.window_transparency)
}

/// Effective alpha for the headerbar/chrome transparency setting.
///
/// This ports the BigLinux terminal transparency curve exactly with
/// `BOOST_FACTOR = 0.0`, `TRANSPARENCY_CURVE_EXPONENT = 1.6`, and
/// `MAX_EFFECTIVE_TRANSPARENCY = 0.8`.
#[must_use]
pub fn effective_chrome_alpha(spec: &BigAppearanceSpec) -> f64 {
    effective_alpha_from_transparency(spec.headerbar_transparency)
}

fn effective_alpha_from_transparency(transparency_percent: u8) -> f64 {
    let user_transparency = f64::from(transparency_percent);
    let adjustment_factor = 1.0 + BOOST_FACTOR;
    let adjusted = (user_transparency * adjustment_factor).min(100.0);
    let curve = (adjusted / 100.0).powf(TRANSPARENCY_CURVE_EXPONENT) * MAX_EFFECTIVE_TRANSPARENCY;
    (1.0 - curve).clamp(0.0, 1.0)
}

fn background_decl(style: &BigSurfaceStyle) -> String {
    gtk_background_decl(
        style.background_color.as_deref(),
        style.gradient.as_ref(),
        style.background_opacity,
        "var(--dialog-bg-color)",
    )
}

/// Strict `#rrggbb` check for externally-supplied color strings.
#[must_use]
pub fn is_strict_hex_color(value: &str) -> bool {
    rgb_triplet(value).is_some()
}

/// CSS color expression: a valid `#rrggbb` passes through; anything else
/// renders the fallback expression (typically a GTK `var(--…)` reference,
/// so surfaces follow the theme when no explicit color is set).
#[must_use]
pub fn css_color_or(value: Option<&str>, fallback: &str) -> String {
    match value {
        Some(v) if is_strict_hex_color(v) => v.to_owned(),
        _ => fallback.to_owned(),
    }
}

/// GTK CSS `alpha(<color-expr>, a)` from an optional hex color, a fallback
/// color expression, and an OPACITY percentage in `0..=100`.
#[must_use]
pub fn gtk_alpha_color_or(value: Option<&str>, fallback: &str, opacity: u32) -> String {
    let color = css_color_or(value, fallback);
    let alpha = f64::from(opacity.min(100)) / 100.0;
    format!("alpha({color}, {alpha:.3})")
}

/// A `border:` declaration when the outline is enabled (`width > 0`),
/// else `None`. Colors follow [`gtk_alpha_color_or`] semantics.
#[must_use]
pub fn gtk_border_decl(
    width: u32,
    color: Option<&str>,
    opacity: u32,
    fallback: &str,
) -> Option<String> {
    if width == 0 {
        return None;
    }
    let color = gtk_alpha_color_or(color, fallback, opacity);
    Some(format!("border: {width}px solid {color};"))
}

/// A `box-shadow:` declaration when the shadow is enabled (`size > 0`),
/// else `None`. Colors follow [`gtk_alpha_color_or`] semantics.
#[must_use]
pub fn gtk_shadow_decl(
    size: u32,
    opacity: u32,
    color: Option<&str>,
    offset_x: i32,
    offset_y: i32,
    fallback: &str,
) -> Option<String> {
    if size == 0 {
        return None;
    }
    let color = gtk_alpha_color_or(color, fallback, opacity);
    Some(format!(
        "box-shadow: {offset_x}px {offset_y}px {size}px {color};"
    ))
}

/// A background declaration: translucent flat color, or a translucent
/// two-color linear gradient when one is given. Colors follow
/// [`gtk_alpha_color_or`] semantics; `opacity` is 0-100.
#[must_use]
pub fn gtk_background_decl(
    color: Option<&str>,
    gradient: Option<&BigLinearGradient>,
    opacity: u32,
    fallback: &str,
) -> String {
    if let Some(gradient) = gradient {
        let from = gtk_alpha_color_or(Some(&gradient.from), fallback, opacity);
        let to = gtk_alpha_color_or(Some(&gradient.to), fallback, opacity);
        format!(
            "background: linear-gradient({}deg, {from}, {to});",
            gradient.angle_deg.min(360)
        )
    } else {
        let color = gtk_alpha_color_or(color, fallback, opacity);
        format!("background-color: {color};")
    }
}

/// A safe `font-family:` declaration, or `None` to keep the theme font.
/// Only ASCII alphanumerics, spaces and hyphens pass; the value is quoted
/// (no CSS injection from user-supplied family names).
#[must_use]
pub fn css_font_family_decl(family: Option<&str>) -> Option<String> {
    font_family_decl(family)
}

fn surface_container_decls(style: &BigSurfaceStyle) -> String {
    let background = background_decl(style);
    let mut decls = format!(
        "  {background}\n  border-radius: {}px;\n  padding: {}px;\n",
        style.radius, style.padding
    );
    if let Some(border) = gtk_border_decl(
        style.border.width,
        style.border.color.as_deref(),
        style.border.opacity,
        "var(--window-fg-color)",
    ) {
        decls.push_str(&format!("  {border}\n"));
    }
    if let Some(shadow) = gtk_shadow_decl(
        style.shadow.size,
        style.shadow.opacity,
        style.shadow.color.as_deref(),
        style.shadow.offset_x,
        style.shadow.offset_y,
        "black",
    ) {
        decls.push_str(&format!("  {shadow}\n"));
    }
    decls
}

fn text_decls(text: &BigSurfaceText) -> String {
    let fg = css_color_or(text.color.as_deref(), "var(--window-fg-color)");
    let mut decls = format!("  font-size: {}pt;\n  color: {fg};\n", text.size);
    if let Some(font_family) = font_family_decl(text.font_family.as_deref()) {
        decls.push_str(&format!("  {font_family}\n"));
    }
    if text.font_bold {
        decls.push_str("  font-weight: bold;\n");
    }
    if text.font_italic {
        decls.push_str("  font-style: italic;\n");
    }
    if text.font_underline {
        decls.push_str("  text-decoration-line: underline;\n");
    }
    decls
}

fn rgb_triplet(hex: &str) -> Option<(u8, u8, u8)> {
    if hex.len() != 7
        || !hex.starts_with('#')
        || !hex[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }

    let red = u8::from_str_radix(&hex[1..3], 16).ok()?;
    let green = u8::from_str_radix(&hex[3..5], 16).ok()?;
    let blue = u8::from_str_radix(&hex[5..7], 16).ok()?;
    Some((red, green, blue))
}

fn font_family_decl(family: Option<&str>) -> Option<String> {
    let family = family?.trim();
    if family.is_empty()
        || !family.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == ' ' || character == '-'
        })
    {
        return None;
    }

    Some(format!("font-family: \"{family}\";"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamping_normalizes_surface_ranges() {
        let style = BigSurfaceStyle {
            gradient: Some(BigLinearGradient {
                from: "#000000".to_owned(),
                to: "#ffffff".to_owned(),
                angle_deg: 999,
            }),
            background_opacity: 999,
            radius: 999,
            padding: 999,
            spacing: 999,
            text: Some(BigSurfaceText {
                size: 999,
                ..BigSurfaceText::default()
            }),
            border: BigSurfaceBorder {
                width: 999,
                opacity: 999,
                ..BigSurfaceBorder::default()
            },
            shadow: BigSurfaceShadow {
                size: 999,
                opacity: 999,
                offset_x: 999,
                offset_y: -999,
                ..BigSurfaceShadow::default()
            },
            ..BigSurfaceStyle::default()
        }
        .clamped();

        assert_eq!(style.gradient.unwrap().angle_deg, 360);
        assert_eq!(style.background_opacity, 100);
        assert_eq!(style.radius, 32);
        assert_eq!(style.padding, 32);
        assert_eq!(style.spacing, 32);
        assert_eq!(style.text.unwrap().size, 18);
        assert_eq!(style.border.width, 8);
        assert_eq!(style.border.opacity, 100);
        assert_eq!(style.shadow.size, 48);
        assert_eq!(style.shadow.opacity, 100);
        assert_eq!(style.shadow.offset_x, 32);
        assert_eq!(style.shadow.offset_y, -32);
    }

    #[test]
    fn gradient_css_is_exact_and_deterministic() {
        let style = BigSurfaceStyle {
            gradient: Some(BigLinearGradient {
                from: "#102030".to_owned(),
                to: "#405060".to_owned(),
                angle_deg: 90,
            }),
            background_opacity: 80,
            radius: 14,
            padding: 10,
            spacing: 6,
            text: Some(BigSurfaceText {
                color: Some("#f0f0f0".to_owned()),
                font_family: Some("Noto Sans".to_owned()),
                size: 13,
                font_bold: true,
                font_italic: true,
                font_underline: true,
            }),
            border: BigSurfaceBorder {
                width: 2,
                color: Some("#ffffff".to_owned()),
                opacity: 60,
            },
            shadow: BigSurfaceShadow {
                size: 12,
                color: Some("#000000".to_owned()),
                opacity: 40,
                offset_x: 1,
                offset_y: 2,
            },
            ..BigSurfaceStyle::default()
        };

        assert_eq!(
            surface_css(".card", &style),
            ".card {\n  background: linear-gradient(90deg, alpha(#102030, 0.800), alpha(#405060, 0.800));\n  border-radius: 14px;\n  padding: 10px;\n  border: 2px solid alpha(#ffffff, 0.600);\n  box-shadow: 1px 2px 12px alpha(#000000, 0.400);\n  font-size: 13pt;\n  color: #f0f0f0;\n  font-family: \"Noto Sans\";\n  font-weight: bold;\n  font-style: italic;\n  text-decoration-line: underline;\n}\n.card * {\n  font-size: 13pt;\n  color: #f0f0f0;\n  font-family: \"Noto Sans\";\n  font-weight: bold;\n  font-style: italic;\n  text-decoration-line: underline;\n}\n"
        );
    }

    #[test]
    fn option_colors_follow_gtk_fallbacks() {
        let css = surface_css(
            ".surface",
            &BigSurfaceStyle {
                background_color: None,
                background_opacity: 25,
                text: Some(BigSurfaceText {
                    color: None,
                    ..BigSurfaceText::default()
                }),
                border: BigSurfaceBorder {
                    width: 1,
                    color: None,
                    opacity: 75,
                },
                ..BigSurfaceStyle::default()
            },
        );

        assert!(
            css.contains("background-color: alpha(var(--dialog-bg-color), 0.250);"),
            "{css}"
        );
        assert!(
            css.contains("border: 1px solid alpha(var(--window-fg-color), 0.750);"),
            "{css}"
        );
        assert!(css.contains("color: var(--window-fg-color);"), "{css}");
    }

    #[test]
    fn transparency_curve_matches_terminal_expectations() {
        let mut spec = BigAppearanceSpec::default();
        assert_eq!(effective_body_alpha(&spec), 1.0);

        spec.window_transparency = 100;
        assert!((effective_body_alpha(&spec) - 0.2).abs() < f64::EPSILON);

        spec.headerbar_transparency = 50;
        let expected = 1.0 - 0.5_f64.powf(TRANSPARENCY_CURVE_EXPONENT) * MAX_EFFECTIVE_TRANSPARENCY;
        assert!((effective_chrome_alpha(&spec) - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn gtk_decl_helpers_follow_theme_fallbacks() {
        assert_eq!(
            css_color_or(Some("#102030"), "var(--dialog-bg-color)"),
            "#102030"
        );
        assert_eq!(
            css_color_or(Some("evil; }"), "var(--dialog-bg-color)"),
            "var(--dialog-bg-color)"
        );
        assert_eq!(
            gtk_alpha_color_or(None, "var(--window-fg-color)", 60),
            "alpha(var(--window-fg-color), 0.600)"
        );
        assert_eq!(gtk_border_decl(0, None, 50, "black"), None);
        assert_eq!(
            gtk_border_decl(2, Some("#ffffff"), 60, "var(--window-fg-color)").as_deref(),
            Some("border: 2px solid alpha(#ffffff, 0.600);")
        );
        assert_eq!(
            gtk_shadow_decl(12, 40, None, 0, 4, "black").as_deref(),
            Some("box-shadow: 0px 4px 12px alpha(black, 0.400);")
        );
        let gradient = BigLinearGradient {
            from: "#101010".to_owned(),
            to: "#202020".to_owned(),
            angle_deg: 90,
        };
        assert_eq!(
            gtk_background_decl(None, Some(&gradient), 80, "var(--dialog-bg-color)"),
            "background: linear-gradient(90deg, alpha(#101010, 0.800), alpha(#202020, 0.800));"
        );
        assert_eq!(
            gtk_background_decl(None, None, 92, "var(--dialog-bg-color)"),
            "background-color: alpha(var(--dialog-bg-color), 0.920);"
        );
    }

    #[test]
    fn tooltip_surface_css_wraps_container_and_text_decls() {
        let style = BigSurfaceStyle {
            radius: 14,
            padding: 9,
            background_opacity: 80,
            text: Some(BigSurfaceText {
                color: Some("#f0f0f0".to_owned()),
                ..BigSurfaceText::default()
            }),
            ..BigSurfaceStyle::default()
        };
        let css = tooltip_surface_css(&style);
        // Fixed wrapper selectors — kills a whole-function stub.
        assert!(css.contains("popover.big-tooltip {"), "{css}");
        assert!(css.contains("popover.big-tooltip > contents {"), "{css}");
        assert!(css.contains("popover.big-tooltip label {"), "{css}");
        // Container declarations flow from `surface_container_decls` — kills its stub.
        assert!(css.contains("border-radius: 14px;"), "{css}");
        assert!(css.contains("padding: 9px;"), "{css}");
        assert!(
            css.contains("background-color: alpha(#2a2a30, 0.800);"),
            "{css}"
        );
        // Text declarations reach the label block.
        assert!(css.contains("color: #f0f0f0;"), "{css}");
    }

    #[test]
    fn css_font_family_decl_quotes_valid_and_rejects_unsafe() {
        // Valid family (alphanumerics + space): quoted verbatim.
        assert_eq!(
            css_font_family_decl(Some("Noto Sans")).as_deref(),
            Some("font-family: \"Noto Sans\";")
        );
        // A hyphen is allowed and passes through — guards the `== '-'` branch.
        assert_eq!(
            css_font_family_decl(Some("a-b")).as_deref(),
            Some("font-family: \"a-b\";")
        );
        // A non-empty family with an unsafe char is dropped — guards the
        // `is_empty() || !all(valid)` disjunction against `&&`.
        assert_eq!(css_font_family_decl(Some("Bad;Name")), None);
        // Empty / whitespace-only families keep the theme font.
        assert_eq!(css_font_family_decl(Some("   ")), None);
        assert_eq!(css_font_family_decl(None), None);
    }

    #[test]
    fn is_strict_hex_color_requires_hash_len7_and_hexdigits() {
        // Canonical valid value.
        assert!(is_strict_hex_color("#0a1b2c"));
        // len == 7 but no leading `#`, rest all hexdigits: the `!starts_with('#')`
        // disjunct alone must reject it (guards both `||` against `&&`).
        assert!(!is_strict_hex_color("0123456"));
        // Leading `#`, all hexdigits, but too long: the `len != 7` disjunct alone
        // must reject it.
        assert!(!is_strict_hex_color("#1234567"));
        // Right length and `#`, but a non-hex digit: the byte-check disjunct.
        assert!(!is_strict_hex_color("#12345g"));
    }

    #[test]
    fn every_css_rule_is_scope_isolated() {
        let css = surface_css(
            "window.big-test",
            &BigSurfaceStyle {
                text: Some(BigSurfaceText::default()),
                ..BigSurfaceStyle::default()
            },
        );

        for rule in css.split("}\n").filter(|rule| !rule.trim().is_empty()) {
            assert!(rule.starts_with("window.big-test"), "{rule}");
        }
    }
}
