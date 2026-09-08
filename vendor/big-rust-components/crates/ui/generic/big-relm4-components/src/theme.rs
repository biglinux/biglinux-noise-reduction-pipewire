// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared libadwaita color-scheme, CSS, and custom palette helpers.

use std::sync::Once;

use relm4::gtk;

/// HIGH CONTRAST CSS constant.
pub const HIGH_CONTRAST_CSS: &str = "\
.dim-label { opacity: 1.0; }
.subtitle { opacity: 1.0; }
.caption { opacity: 1.0; }
row.property > box.header > box.title > .subtitle { opacity: 1.0; color: @window_fg_color; }
list.navigation-sidebar row label.subtitle { opacity: 1.0; }
headerbar .subtitle { opacity: 1.0; }
";

/// Color palette value used across BigLinux apps.
pub struct ColorPalette {
    /// Window/background color (sRGB 8-bit triplet).
    pub bg: (u8, u8, u8),
    /// Foreground/text color (sRGB 8-bit triplet).
    pub fg: (u8, u8, u8),
    /// Symbolic-icon tint color (sRGB 8-bit triplet).
    pub icon: (u8, u8, u8),
    /// Dialog/popover background color (sRGB 8-bit triplet).
    pub dialog: (u8, u8, u8),
}

/// Render the libadwaita CSS override block for the supplied palette.
///
/// Emits `@define-color` declarations and `:root` custom properties covering
/// window, headerbar, popover, dialog, card, view, sidebar surfaces, plus
/// the BigLinux-specific `player_icon_color`. Load the result via
/// [`inject_palette_css`].
#[must_use]
pub fn build_adwaita_color_overrides(palette: &ColorPalette) -> String {
    let (bg_r, bg_g, bg_b) = palette.bg;
    let (fg_r, fg_g, fg_b) = palette.fg;
    let (ic_r, ic_g, ic_b) = palette.icon;
    let (dl_r, dl_g, dl_b) = palette.dialog;

    format!(
        "@define-color window_bg_color rgb({bg_r},{bg_g},{bg_b});\n\
         @define-color window_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color headerbar_bg_color rgb({bg_r},{bg_g},{bg_b});\n\
         @define-color headerbar_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color popover_bg_color rgb({dl_r},{dl_g},{dl_b});\n\
         @define-color popover_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color dialog_bg_color rgb({dl_r},{dl_g},{dl_b});\n\
         @define-color dialog_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color card_bg_color rgb({dl_r},{dl_g},{dl_b});\n\
         @define-color card_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color view_bg_color rgb({bg_r},{bg_g},{bg_b});\n\
         @define-color view_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color sidebar_bg_color rgb({bg_r},{bg_g},{bg_b});\n\
         @define-color sidebar_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         :root {{\n\
         --window-bg-color: rgb({bg_r},{bg_g},{bg_b});\n\
         --window-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --headerbar-bg-color: rgb({bg_r},{bg_g},{bg_b});\n\
         --headerbar-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --popover-bg-color: rgb({dl_r},{dl_g},{dl_b});\n\
         --popover-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --dialog-bg-color: rgb({dl_r},{dl_g},{dl_b});\n\
         --dialog-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --card-bg-color: rgb({dl_r},{dl_g},{dl_b});\n\
         --card-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --view-bg-color: rgb({bg_r},{bg_g},{bg_b});\n\
         --view-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         }}\n\
         @define-color player_icon_color rgb({ic_r},{ic_g},{ic_b});\n"
    )
}

/// Switch the default [`adw::StyleManager`] color scheme by string key.
///
/// Maps `"gtk-light"` to `ForceLight`, `"gtk-dark"` and `"gradient"` to
/// `ForceDark`, and any other value to `Default` (follow system).
pub fn apply_color_scheme(color_source: &str) {
    let style_manager = adw::StyleManager::default();
    match color_source {
        "gtk-light" => style_manager.set_color_scheme(adw::ColorScheme::ForceLight),
        "gtk-dark" | "gradient" => style_manager.set_color_scheme(adw::ColorScheme::ForceDark),
        _ => style_manager.set_color_scheme(adw::ColorScheme::Default),
    }
}

/// Load `css` into a process-wide [`gtk::CssProvider`] above user priority.
///
/// `provider_slot` caches the provider so repeated calls re-use the same
/// instance, replacing its contents instead of stacking new providers on
/// the [`gtk::gdk::Display`].
pub fn inject_palette_css(css: &str, provider_slot: &mut Option<gtk::CssProvider>) {
    let provider = if let Some(ref provider) = *provider_slot {
        provider.clone()
    } else {
        let provider = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
            );
        }
        *provider_slot = Some(provider.clone());
        provider
    };
    provider.load_from_string(css);
}

/// Load process-wide application CSS at application priority.
pub fn load_app_css(css: &str) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(css);

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Load process-wide application CSS once, guarded by `loaded`.
pub fn load_app_css_once(css: &'static str, loaded: &'static Once) {
    loaded.call_once(|| load_app_css(css));
}

/// Run `on_dark_changed(is_dark)` whenever the libadwaita dark state flips.
///
/// Listens on the process-wide [`adw::StyleManager`] `dark` property, so it
/// tracks both explicit color-scheme changes and system light/dark switches.
/// The listener lives for the process lifetime; call it once from an
/// idempotent boot path (guard with a `Once`/`OnceLock` if a call site can
/// fire more than once). Generalized from big-terminal's app-local
/// `install_style_dark_listener`, which reruns a full theme refresh; this
/// primitive stays behavior-free and hands the new dark state to the caller.
pub fn install_style_dark_listener(on_dark_changed: impl Fn(bool) + 'static) {
    adw::StyleManager::default().connect_dark_notify(move |style_manager| {
        on_dark_changed(style_manager.is_dark());
    });
}

/// Decode a `#RRGGBB` or `RRGGBB` hex string into an `(R, G, B)` tuple.
///
/// The leading `#` is optional. Strings shorter than 6 hex digits, or with
/// non-hex characters, fall back to the BigLinux placeholder `(42, 42, 48)`
/// so callers always receive a renderable triple. The shorthand `#RGB`
/// form is **not** expanded.
#[must_use]
pub fn parse_hex_color(hex_str: &str) -> (u8, u8, u8) {
    let hex = hex_str.trim_start_matches('#');
    if hex.len() >= 6 {
        (
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(42),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(42),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(48),
        )
    } else {
        (42, 42, 48)
    }
}

/// Decode a hex color string into an opaque [`gtk::gdk::RGBA`].
///
/// Delegates to [`parse_hex_color`], divides each channel by 255, and sets
/// alpha to `1.0`. Inherits the same malformed-input fallback.
#[must_use]
pub fn parse_hex_to_rgba(hex_str: &str) -> gtk::gdk::RGBA {
    let (r, g, b) = parse_hex_color(hex_str);
    gtk::gdk::RGBA::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_color_accepts_hash_and_plain() {
        assert_eq!(parse_hex_color("#FF8000"), (255, 128, 0));
        assert_eq!(parse_hex_color("00FF80"), (0, 255, 128));
    }

    #[test]
    fn parse_hex_color_uses_visible_fallback() {
        assert_eq!(parse_hex_color("#FFF"), (42, 42, 48));
        assert_eq!(parse_hex_color(""), (42, 42, 48));
    }

    #[test]
    fn build_overrides_contains_color_contract() {
        let palette = ColorPalette {
            bg: (30, 30, 30),
            fg: (220, 220, 220),
            icon: (100, 150, 200),
            dialog: (40, 40, 40),
        };
        let css = build_adwaita_color_overrides(&palette);
        assert!(css.contains("@define-color window_bg_color rgb(30,30,30);"));
        assert!(css.contains("@define-color player_icon_color rgb(100,150,200);"));
        assert!(css.contains("--dialog-fg-color: rgb(220,220,220);"));
    }
}
