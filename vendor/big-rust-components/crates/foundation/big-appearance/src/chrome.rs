// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Window-chrome CSS generation shared by BigLinux apps: custom palette
//! (libadwaita named colors + CSS vars), translucent headerbar, transparent
//! shell layers, and the tab-strip themes for the shared tab strip.
//!
//! Promoted from big-terminal's theme engine (platform-consolidation M1).
//! All functions are pure `spec in → CSS string out`; GTK never appears.
//! Selectors target the shared window-shell classes (`.big-main-window`,
//! `.main-header-bar`, `.big-tab-button`, `.big-tab-*`); app-specific
//! selectors enter through the `extra_selectors` parameters.

const TRANSPARENCY_CURVE_EXPONENT: f64 = 1.6;
const MAX_EFFECTIVE_TRANSPARENCY: f64 = 0.8;
const INTEGRATED_HEADERBAR_RATIO: f64 = 0.9;
const SIMPLE_TAB_THEME_RULES: [(BigTabStripTheme, &str, &str, &str, &str, u8); 3] = [
    (
        BigTabStripTheme::Balanced,
        "10px",
        "",
        "color-mix(in srgb, {tone}, transparent 82%)",
        "none",
        90,
    ),
    (
        BigTabStripTheme::Compact,
        "6px",
        "padding-left: 10px;\n            padding-top: 1px;\n            padding-bottom: 1px;",
        "color-mix(in srgb, @accent_bg_color, transparent 74%)",
        "inset 0 0 0 1px color-mix(in srgb, {tone}, transparent 54%)",
        91,
    ),
    (
        BigTabStripTheme::Underline,
        "6px 6px 0 0",
        "background-color: transparent;",
        "color-mix(in srgb, {tone}, transparent 94%)",
        "inset 0 -3px 0 0 @accent_bg_color",
        92,
    ),
];

/// Chrome colors resolved from the app's active color scheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigChromePalette {
    /// Body/content background as `#rrggbb`.
    pub bg: String,
    /// Foreground/tone color as `#rrggbb`.
    pub fg: String,
    /// Headerbar background as `#rrggbb`.
    pub header_bg: String,
    /// Chrome background (popovers, dialogs, cards) as `#rrggbb`.
    pub chrome_bg: String,
}

impl BigChromePalette {
    /// Rec.601 (YIQ) luminance of the body background, `0.0..=1.0`.
    ///
    /// Matches the terminal's historical weights for chrome decisions —
    /// deliberately NOT the WCAG Rec.709 weights used for text contrast.
    #[must_use]
    pub fn luminance(&self) -> f64 {
        let (r, g, b) = hex_to_rgb_f64(&self.bg).unwrap_or((0.0, 0.0, 0.0));
        0.299 * r + 0.587 * g + 0.114 * b
    }
}

/// One window's chrome-theming input: palette + the two transparency
/// sliders + whether the palette follows the system theme.
///
/// `chrome_transparency` (headerbar slider) and `body_transparency`
/// (window slider) are TRANSPARENCY percentages in `0..=100`.
/// `follow_system: true` keeps libadwaita named colors untouched and
/// tones controls with `currentColor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigChromeSpec {
    /// Scheme-resolved chrome colors.
    pub palette: BigChromePalette,
    /// Headerbar/chrome TRANSPARENCY percentage in `0..=100`.
    pub chrome_transparency: u32,
    /// Body/window TRANSPARENCY percentage in `0..=100`.
    pub body_transparency: u32,
    /// Palette follows the system theme instead of the app scheme.
    pub follow_system: bool,
}

impl BigChromeSpec {
    fn tone(&self) -> &str {
        if self.follow_system {
            "currentColor"
        } else {
            self.palette.fg.as_str()
        }
    }
}

/// Visual style of the shared tab strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BigTabStripTheme {
    /// Rounded pill, tone-mixed active background (suite default).
    #[default]
    Balanced,
    /// Tighter padding, accent-tinted active tab with an outline.
    Compact,
    /// Flat tabs with an accent underline on the active tab.
    Underline,
    /// Luminance-aware "lifted card" active tab with cast shadows.
    Elevated,
    /// Headerbar and active tab share one composited surface paint.
    Integrated,
}

impl BigTabStripTheme {
    /// Stable settings keys, in display order.
    pub const KEYS: [&'static str; 5] =
        ["balanced", "compact", "underline", "elevated", "integrated"];

    /// Parse a persisted key; unknown values fall back to [`Self::Balanced`].
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        match key {
            "compact" => Self::Compact,
            "underline" => Self::Underline,
            "elevated" => Self::Elevated,
            "integrated" => Self::Integrated,
            _ => Self::Balanced,
        }
    }

    /// The persisted settings key for this theme.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Compact => "compact",
            Self::Underline => "underline",
            Self::Elevated => "elevated",
            Self::Integrated => "integrated",
        }
    }
}

/// Opacity percentage (`0..=100`) for the chrome/headerbar slider value.
///
/// Gamma-1.6 curve with a 0.8 effective-transparency cap — the same ramp
/// the VTE body alpha uses, so both sliders track the same visual range.
#[must_use]
pub fn chrome_opacity_percent(slider: u32) -> u32 {
    let t = (f64::from(slider) / 100.0).clamp(0.0, 1.0);
    let transparency_fraction = t.powf(TRANSPARENCY_CURVE_EXPONENT) * MAX_EFFECTIVE_TRANSPARENCY;
    let opacity = (1.0 - transparency_fraction).clamp(0.0, 1.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let pct = (opacity * 100.0).round() as u32;
    pct.min(100)
}

/// Effective body alpha for `transparency` (`0..=100`) — same curve as
/// [`chrome_opacity_percent`], returned as `0.0..=1.0` alpha.
#[must_use]
pub fn body_alpha(transparency: f64) -> f64 {
    let adjusted = transparency.min(100.0);
    let curve = (adjusted / 100.0).powf(TRANSPARENCY_CURVE_EXPONENT) * MAX_EFFECTIVE_TRANSPARENCY;
    (1.0 - curve).clamp(0.0, 1.0)
}

/// Libadwaita named-color overrides + window CSS vars for the palette.
///
/// Returns `""` when the spec follows the system theme. Feeds both
/// mechanisms libadwaita widgets resolve colors through:
///
/// 1. `@define-color NAME value` — named-color lookups (AdwPreferencesGroup,
///    AdwActionRow, AdwAlertDialog, AdwBanner…). A PRIORITY_USER provider
///    beats Adwaita's PRIORITY_THEME, so these win globally.
/// 2. `--name-color: value` on `window` — widgets resolving `var(--*)`
///    directly (NavigationSplitView sidebar, 1.6+ rows) and custom CSS.
///
/// Surface tints mirror the stock Adwaita deltas (window→view→sidebar→card)
/// so preference dialogs keep their elevation cues on any scheme.
#[must_use]
pub fn palette_vars_css(spec: &BigChromeSpec) -> String {
    if spec.follow_system {
        return String::new();
    }
    let fg = &spec.palette.fg;
    let header_bg = &spec.palette.header_bg;
    let chrome_bg = &spec.palette.chrome_bg;
    // The 1px headerbar/body shade is erased: with a custom palette the
    // chrome and the content must read as a single surface.
    let header_shade = "transparent";
    let luminance = spec.palette.luminance();
    if luminance < 0.05 {
        return format!(
            r"
        @define-color headerbar_bg_color {header_bg};
        @define-color headerbar_fg_color {fg};
        @define-color headerbar_backdrop_color {header_bg};
        @define-color headerbar_shade_color {header_shade};

        window {{
            --headerbar-bg-color: {header_bg};
            --headerbar-fg-color: {fg};
            --headerbar-backdrop-color: {header_bg};
            --headerbar-shade-color: {header_shade};
        }}
        "
        );
    }
    let dark = luminance < 0.5;
    let (sidebar_mix, card_mix, view_mix) = if dark {
        ("white 6%", "white 10%", "black 6%")
    } else {
        ("black 6%", "white 3%", "white 0%")
    };
    let view_bg = format!("color-mix(in srgb, {chrome_bg}, {view_mix})");
    let sidebar_bg = format!("color-mix(in srgb, {chrome_bg}, {sidebar_mix})");
    let card_bg = format!("color-mix(in srgb, {chrome_bg}, {card_mix})");
    format!(
        r"
        @define-color window_bg_color {chrome_bg};
        @define-color window_fg_color {fg};
        @define-color view_bg_color {view_bg};
        @define-color view_fg_color {fg};
        @define-color headerbar_bg_color {header_bg};
        @define-color headerbar_fg_color {fg};
        @define-color headerbar_backdrop_color {header_bg};
        @define-color headerbar_shade_color {header_shade};
        @define-color popover_bg_color {chrome_bg};
        @define-color popover_fg_color {fg};
        @define-color dialog_bg_color {chrome_bg};
        @define-color dialog_fg_color {fg};
        @define-color card_bg_color {card_bg};
        @define-color card_fg_color {fg};
        @define-color sidebar_bg_color {sidebar_bg};
        @define-color sidebar_fg_color {fg};

        window {{
            /* Window and View Colors */
            --window-bg-color: {chrome_bg};
            --window-fg-color: {fg};
            --view-bg-color: {view_bg};
            --view-fg-color: {fg};

            /* Headerbar Colors */
            --headerbar-bg-color: {header_bg};
            --headerbar-fg-color: {fg};
            --headerbar-backdrop-color: {header_bg};
            --headerbar-shade-color: {header_shade};

            /* Popover and Dialog Colors */
            --popover-bg-color: {chrome_bg};
            --popover-fg-color: {fg};
            --dialog-bg-color: {chrome_bg};
            --dialog-fg-color: {fg};

            /* Card and Thumbnail Colors (Common in lists) */
            --card-bg-color: {card_bg};
            --card-fg-color: {fg};

            /* Sidebar (if using split view naming) */
            --sidebar-bg-color: {sidebar_bg};
            --sidebar-fg-color: {fg};
        }}
        "
    )
}

/// Shared headerbar selectors scoped to the main window. Dialog headerbars
/// stay opaque — the transparency slider only touches the main chrome.
const HEADERBAR_SELECTORS: [&str; 6] = [
    ".big-main-window headerbar.main-header-bar",
    ".big-main-window .main-header-bar",
    ".big-main-window .top-bar",
    ".big-main-window searchbar",
    ".big-main-window searchbar > box",
    ".big-main-window searchbar > revealer > box",
];

/// Translucent (or palette-painted) headerbar/chrome CSS.
///
/// Returns `""` when there is nothing to paint (opaque + system palette).
/// `extra_selectors` appends app-owned chrome surfaces (already scoped by
/// the caller) to the shared selector list.
#[must_use]
pub fn headerbar_css(spec: &BigChromeSpec, extra_selectors: &[&str]) -> String {
    if spec.chrome_transparency == 0 && spec.follow_system {
        return String::new();
    }
    let base_bg: &str = if spec.follow_system {
        "@headerbar_bg_color"
    } else {
        spec.palette.header_bg.as_str()
    };
    // Shared gamma-1.6 + 0.8-cap curve — mirrors the VTE alpha ramp so the
    // headerbar and body sliders track the same range.
    let bg_value = if spec.chrome_transparency > 0 {
        let opacity_percent = chrome_opacity_percent(spec.chrome_transparency);
        format!("color-mix(in srgb, {base_bg} {opacity_percent}%, transparent)")
    } else {
        base_bg.to_owned()
    };
    let selectors: Vec<&str> = HEADERBAR_SELECTORS
        .iter()
        .copied()
        .chain(extra_selectors.iter().copied())
        .collect();
    let plain = selectors.join(",\n");
    let backdrop = selectors.join(":backdrop,\n");
    format!(
        r"
        {plain} {{
            background-color: {bg_value};
            background-image: none;
        }}
        {backdrop}:backdrop {{
            background-color: {bg_value};
            background-image: none;
        }}
        "
    )
}

/// Shared shell layers cleared to transparent for translucent windows.
///
/// `.background` is auto-applied by libadwaita to every top-level; scoping
/// with `.big-main-window` keeps dialogs opaque. `extra_selectors` appends
/// app-owned layers (already scoped by the caller).
#[must_use]
pub fn shell_layers_transparent_css(extra_selectors: &[&str]) -> String {
    let mut selectors: Vec<&str> = vec![
        ".big-main-window .big-window-toast-root",
        ".big-main-window .big-window-overlay",
        ".big-main-window .big-window-body",
        ".big-main-window .big-dock-root",
        ".big-main-window .big-dock-paned",
        ".big-main-window .big-workspace-shell",
        ".big-main-window .big-tab-content-stack",
        ".big-main-window .big-split-tree",
    ];
    selectors.extend_from_slice(extra_selectors);
    format!(
        "\nwindow.big-main-window.background {{\n\
             background-color: transparent;\n\
             background-image: none;\n\
         }}\n\
         {} {{\n\
             background-color: transparent;\n\
             background-image: none;\n\
         }}\n",
        selectors.join(",\n")
    )
}

/// Tab-strip CSS for the shared strip classes, themed by `theme`.
#[must_use]
pub fn tab_strip_theme_css(spec: &BigChromeSpec, theme: BigTabStripTheme) -> String {
    let tone = spec.tone();
    let theme_css = match theme {
        BigTabStripTheme::Balanced | BigTabStripTheme::Compact | BigTabStripTheme::Underline => {
            let (_, radius, base, active_background, active_shadow, hover_transparency) =
                SIMPLE_TAB_THEME_RULES
                    .iter()
                    .find(|(rule_theme, _, _, _, _, _)| *rule_theme == theme)
                    .expect("simple tab theme table covers simple tab themes");
            format!(
                r"
        .big-main-window .big-tab-button {{
            border-radius: {radius};
            {base}
        }}
        .big-main-window .big-tab-button.active {{
            background-color: {};
            box-shadow: {};
        }}
        .big-main-window .big-tab-button:hover {{
            background-color: color-mix(in srgb, {tone}, transparent {hover_transparency}%);
        }}
        ",
                active_background.replace("{tone}", tone),
                active_shadow.replace("{tone}", tone),
            )
        }
        BigTabStripTheme::Elevated => tab_theme_elevated_css(spec),
        BigTabStripTheme::Integrated => tab_theme_integrated_css(spec),
    };
    format!(
        r"
        .big-main-window .big-tab-button {{
            transition: background-color 120ms ease, box-shadow 120ms ease;
        }}
        .big-main-window .big-tab-new-row {{
            background: transparent;
            border: none;
            box-shadow: none;
            border-radius: 10px;
            padding: 0 10px;
            color: color-mix(in srgb, {tone}, transparent 30%);
        }}
        .big-main-window .big-tab-new-row:hover {{
            background-color: color-mix(in srgb, {tone}, transparent 90%);
            color: {tone};
        }}
        .big-main-window .big-tab-dock-grip {{
            background: transparent;
        }}
        .scrolled-tab-bar viewport box .horizontal.active {{
            background-color: color-mix(in srgb, {tone}, transparent 82%);
        }}
        .big-main-window .big-tab-button.in-group {{
            padding-bottom: 0;
        }}
        {theme_css}
        "
    )
}

fn tab_theme_elevated_css(spec: &BigChromeSpec) -> String {
    let tone = spec.tone();
    // Luminance-aware lighting model. Light themes lean on the black
    // side-shadow as cast shadow (the strip darkens around the tab); dark
    // themes flip to a tone-tinted halo (the strip catches light around
    // the lifted edge). Either way the top of the gradient is biased
    // toward `fg` so the tab face is visibly distinct in both modes.
    let dark_theme = spec.palette.luminance() < 0.5;
    // Gradient is pure body bg at every stop — no `fg` blending, no
    // headerbar tint forced into the tab face. The visual lift comes from
    // the alpha ramp plus the depth shadows; the colour stays anchored to
    // the content so tab and body read as the same surface. Bottom alpha
    // stays non-zero (≈50% of the top) so the tab never fully dissolves
    // into the strip — it thins out enough to suggest a peeled edge while
    // staying chromatically tied to the content.
    let gradient = if spec.follow_system {
        "linear-gradient(to bottom, \
             @window_bg_color 0%, \
             color-mix(in srgb, @window_bg_color, transparent 22%) 55%, \
             color-mix(in srgb, @window_bg_color, transparent 50%) 100%)"
            .to_owned()
    } else {
        let (r, g, b) = hex_to_rgb_f64(&spec.palette.bg).unwrap_or((0.0, 0.0, 0.0));
        let a = body_alpha(f64::from(spec.body_transparency));
        format!(
            "linear-gradient(to bottom, {top} 0%, {mid} 55%, {bottom} 100%)",
            top = format_paint(r, g, b, a),
            mid = format_paint(r, g, b, a * 0.78),
            bottom = format_paint(r, g, b, a * 0.50),
        )
    };
    // Dark strips make black shadows invisible, so a tone-tinted halo
    // carries the depth cue there; light themes keep the classic black
    // cast shadow.
    let side_shadow = if dark_theme {
        format!(
            "-18px 4px 24px -6px color-mix(in srgb, {tone}, transparent 75%), \
             18px 4px 24px -6px color-mix(in srgb, {tone}, transparent 75%)"
        )
    } else {
        "-16px 4px 22px -8px rgba(0, 0, 0, 0.45), \
         16px 4px 22px -8px rgba(0, 0, 0, 0.45)"
            .to_owned()
    };
    let side_shadow_hover = if dark_theme {
        format!(
            "-22px 5px 30px -6px color-mix(in srgb, {tone}, transparent 65%), \
             22px 5px 30px -6px color-mix(in srgb, {tone}, transparent 65%)"
        )
    } else {
        "-18px 5px 26px -8px rgba(0, 0, 0, 0.52), \
         18px 5px 26px -8px rgba(0, 0, 0, 0.52)"
            .to_owned()
    };
    // Top glow doubles on dark schemes — the "lit from above" cue carries
    // the depth illusion when shadows can't.
    let top_glow_alpha = if dark_theme { 50 } else { 70 };
    let top_glow_alpha_hover = if dark_theme { 40 } else { 60 };
    let top_edge_alpha = if dark_theme { 35 } else { 50 };
    let top_edge_alpha_hover = if dark_theme { 25 } else { 40 };
    format!(
        r"
        .big-main-window .big-tab-button {{
            border-radius: 0;
            transition: background-image 180ms ease, box-shadow 180ms ease;
        }}
        .big-main-window .big-tab-button.active {{
            background-color: transparent;
            background-image: {gradient};
            box-shadow:
                inset 0 1px 0 0 color-mix(in srgb, {tone}, transparent {top_edge_alpha}%),
                0 -6px 14px -2px color-mix(in srgb, {tone}, transparent {top_glow_alpha}%),
                {side_shadow};
        }}
        .big-main-window .big-tab-button:hover {{
            background-color: color-mix(in srgb, {tone}, transparent 90%);
        }}
        .big-main-window .big-tab-button.active:hover {{
            background-color: transparent;
            background-image: {gradient};
            box-shadow:
                inset 0 1px 0 0 color-mix(in srgb, {tone}, transparent {top_edge_alpha_hover}%),
                0 -8px 18px -2px color-mix(in srgb, {tone}, transparent {top_glow_alpha_hover}%),
                {side_shadow_hover};
        }}
        "
    )
}

fn tab_theme_integrated_css(spec: &BigChromeSpec) -> String {
    // System-palette apps have no scheme colors to composite the
    // headerbar/tab pair from; the active tab fuses with the content by
    // sharing the named view color and the headerbar keeps its theme paint.
    if spec.follow_system {
        return r"
        .big-main-window .big-tab-button {
            border-radius: 8px 8px 0 0;
            background-color: transparent;
            box-shadow: none;
        }
        .big-main-window .big-tab-button.active {
            background-color: @view_bg_color;
            box-shadow: none;
        }
        .big-main-window .big-tab-button:hover {
            background-color: alpha(currentColor, 0.08);
        }
        .big-main-window .big-tab-button.active:hover {
            background-color: @view_bg_color;
        }
        "
        .to_owned();
    }
    let (integrated_header_bg, active_tab_paint) = integrated_paints(spec);
    let active_tab_fg = &spec.palette.fg;
    format!(
        r"
        .big-main-window headerbar.main-header-bar,
        .big-main-window .main-header-bar,
        .big-main-window headerbar.main-header-bar:backdrop,
        .big-main-window .main-header-bar:backdrop {{
            background-color: {integrated_header_bg};
            background-image: none;
        }}
        .big-main-window .main-header-bar,
        .big-main-window .main-header-bar > button,
        .big-main-window .main-header-bar > image,
        .big-main-window .main-header-bar > label,
        .big-main-window .main-header-bar > box,
        .big-main-window .main-header-bar > box > button,
        .big-main-window .main-header-bar > box > image,
        .big-main-window .main-header-bar > box > label,
        .big-main-window .main-header-bar windowcontrols > button {{
            color: {active_tab_fg};
        }}
        /* Reset cascade so popovers anchored to the headerbar (hamburger
           menu, tab/group right-click) keep system theme colors. */
        .big-main-window .main-header-bar popover.menu,
        .big-main-window .main-header-bar popover.menu > contents,
        .big-main-window .main-header-bar popover.menu modelbutton,
        .big-main-window .main-header-bar popover.menu label,
        .big-main-window .main-header-bar popover.menu image,
        .big-main-window .big-tab-button popover.menu,
        .big-main-window .big-tab-button popover.menu > contents,
        .big-main-window .big-tab-button popover.menu modelbutton,
        .big-main-window .big-tab-button popover.menu label,
        .big-main-window .big-tab-button popover.menu image,
        .big-main-window popover.sidebar-popover,
        .big-main-window popover.sidebar-popover label,
        .big-main-window popover.sidebar-popover image,
        .big-main-window popover.sidebar-popover modelbutton,
        .big-main-window popover.sidebar-popover button {{
            color: @theme_fg_color;
        }}
        .big-main-window .big-tab-button {{
            border-radius: 8px 8px 0 0;
            background-color: transparent;
            box-shadow: none;
        }}
        .big-main-window .big-tab-button.active {{
            background-color: {active_tab_paint};
            color: {active_tab_fg};
            box-shadow: none;
        }}
        .big-main-window .big-tab-button.active label,
        .big-main-window .big-tab-button.active image {{
            color: {active_tab_fg};
        }}
        .big-main-window .big-tab-button:hover {{
            background-color: alpha(currentColor, 0.08);
        }}
        .big-main-window .big-tab-button.active:hover {{
            background-color: {active_tab_paint};
        }}
        "
    )
}

fn integrated_headerbar_rgb(spec: &BigChromeSpec) -> (f64, f64, f64) {
    let (r, g, b) = hex_to_rgb_f64(&spec.palette.bg).unwrap_or((0.0, 0.0, 0.0));
    // Headerbar sits 4% away from the body: darker on light schemes,
    // lighter on dark schemes.
    let adj = 0.04;
    if spec.palette.luminance() >= 0.5 {
        (r * (1.0 - adj), g * (1.0 - adj), b * (1.0 - adj))
    } else {
        (
            r + (1.0 - r) * adj,
            g + (1.0 - g) * adj,
            b + (1.0 - b) * adj,
        )
    }
}

fn integrated_paints(spec: &BigChromeSpec) -> (String, String) {
    let (r_bg, g_bg, b_bg) = hex_to_rgb_f64(&spec.palette.bg).unwrap_or((0.0, 0.0, 0.0));
    let (r_hb, g_hb, b_hb) = integrated_headerbar_rgb(spec);
    let a_t = body_alpha(f64::from(spec.body_transparency));
    let ratio = INTEGRATED_HEADERBAR_RATIO
        * f64::from(chrome_opacity_percent(spec.chrome_transparency))
        / 100.0;
    let a_hb = a_t * ratio;

    let header = format_paint(r_hb, g_hb, b_hb, a_hb);

    // Degenerate guard: `ratio` ≈ 1 (or `a_t` ≈ `a_hb`) collapses the
    // denominator. With the 0.9 default ratio this can't happen at
    // runtime — guard kept for future-proofing.
    let denom = a_t - a_hb;
    if denom < 1e-6 {
        let tab = format_paint(r_bg, g_bg, b_bg, a_t);
        return (header, tab);
    }

    // Un-premultiply: solve for the tab paint that composites over the
    // headerbar paint to reproduce the body color exactly.
    let one_minus_a_hb = 1.0 - a_hb;
    let one_minus_a_t = 1.0 - a_t;
    let a_tab = (denom / one_minus_a_hb).clamp(0.0, 1.0);

    let c_tab = |bg_c: f64, hb_c: f64| -> f64 {
        ((bg_c * a_t * one_minus_a_hb - hb_c * a_hb * one_minus_a_t) / denom).clamp(0.0, 1.0)
    };
    let r_tab = c_tab(r_bg, r_hb);
    let g_tab = c_tab(g_bg, g_hb);
    let b_tab = c_tab(b_bg, b_hb);

    let tab = format_paint(r_tab, g_tab, b_tab, a_tab);
    (header, tab)
}

fn format_paint(r: f64, g: f64, b: f64, a: f64) -> String {
    if a >= 0.999 {
        format!("rgb({}, {}, {})", css_byte(r), css_byte(g), css_byte(b))
    } else {
        format!(
            "rgba({}, {}, {}, {:.4})",
            css_byte(r),
            css_byte(g),
            css_byte(b),
            a
        )
    }
}

fn css_byte(channel: f64) -> u8 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    {
        (channel * 255.0).round().clamp(0.0, 255.0) as u8
    }
}

fn hex_to_rgb_f64(hex: &str) -> Option<(f64, f64, f64)> {
    if hex.len() != 7
        || !hex.starts_with('#')
        || !hex[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let r = u8::from_str_radix(&hex[1..3], 16).ok()?;
    let g = u8::from_str_radix(&hex[3..5], 16).ok()?;
    let b = u8::from_str_radix(&hex[5..7], 16).ok()?;
    Some((
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> BigChromeSpec {
        BigChromeSpec {
            palette: BigChromePalette {
                bg: "#282a36".to_owned(),
                fg: "#f8f8f2".to_owned(),
                header_bg: "#3a3c4e".to_owned(),
                chrome_bg: "#282a36".to_owned(),
            },
            chrome_transparency: 15,
            body_transparency: 40,
            follow_system: false,
        }
    }

    #[test]
    fn luminance_is_rec601_of_bg() {
        let lum = spec().palette.luminance();
        assert!((lum - 0.166).abs() < 0.01, "{lum}");
    }

    #[test]
    fn chrome_opacity_percent_matches_terminal_curve() {
        assert_eq!(chrome_opacity_percent(0), 100);
        assert_eq!(chrome_opacity_percent(100), 20);
        // 50% slider: 1 - 0.5^1.6 * 0.8 ≈ 0.7364 → 74
        assert_eq!(chrome_opacity_percent(50), 74);
    }

    #[test]
    fn body_alpha_matches_terminal_curve() {
        assert!((body_alpha(0.0) - 1.0).abs() < f64::EPSILON);
        assert!((body_alpha(100.0) - 0.2).abs() < 1e-9);
    }

    #[test]
    fn palette_vars_follow_system_is_empty() {
        let mut s = spec();
        s.follow_system = true;
        assert_eq!(palette_vars_css(&s), "");
    }

    #[test]
    fn palette_vars_define_named_colors_and_window_vars() {
        let css = palette_vars_css(&spec());
        assert!(css.contains("@define-color window_bg_color #282a36;"));
        assert!(css.contains("@define-color headerbar_bg_color #3a3c4e;"));
        assert!(css.contains("--card-bg-color: color-mix(in srgb, #282a36, white 10%);"));
        assert!(css.contains("@define-color headerbar_shade_color transparent;"));
    }

    #[test]
    fn near_black_palette_only_overrides_headerbar() {
        let mut s = spec();
        s.palette.bg = "#000000".to_owned();
        let css = palette_vars_css(&s);
        assert!(css.contains("@define-color headerbar_bg_color"));
        assert!(!css.contains("@define-color window_bg_color"));
    }

    #[test]
    fn headerbar_css_mixes_palette_with_slider_opacity() {
        let css = headerbar_css(&spec(), &[".big-main-window .command-toolbar"]);
        assert!(css.contains("color-mix(in srgb, #3a3c4e 96%, transparent)"));
        assert!(css.contains(".big-main-window .command-toolbar"));
        assert!(css.contains(".big-main-window .main-header-bar:backdrop"));
    }

    #[test]
    fn headerbar_css_opaque_system_theme_is_empty() {
        let mut s = spec();
        s.chrome_transparency = 0;
        s.follow_system = true;
        assert_eq!(headerbar_css(&s, &[]), "");
    }

    #[test]
    fn headerbar_css_custom_palette_opaque_paints_flat_color() {
        // Opaque (transparency 0) but a CUSTOM palette: the early-out only fires
        // for opaque AND system, so a custom palette must still paint — and at
        // transparency 0 the fill is the flat `header_bg`, NOT a 100% color-mix.
        let mut s = spec();
        s.chrome_transparency = 0;
        s.follow_system = false;
        let css = headerbar_css(&s, &[]);
        assert!(
            !css.is_empty(),
            "custom palette must paint even when opaque"
        );
        assert!(css.contains("background-color: #3a3c4e;"), "{css}");
        assert!(
            !css.contains("color-mix"),
            "transparency 0 paints a flat color, not a mix: {css}"
        );
    }

    #[test]
    fn shell_layers_strip_scopes_main_window_only() {
        let css = shell_layers_transparent_css(&[".big-main-window .my-app-layer"]);
        assert!(css.contains("window.big-main-window.background"));
        assert!(css.contains(".big-main-window .big-tab-content-stack"));
        assert!(css.contains(".big-main-window .my-app-layer"));
        assert!(!css.contains("\nwindow {\n"));
    }

    #[test]
    fn tab_theme_keys_roundtrip() {
        for key in BigTabStripTheme::KEYS {
            assert_eq!(BigTabStripTheme::from_key(key).key(), key);
        }
        assert_eq!(
            BigTabStripTheme::from_key("nonsense"),
            BigTabStripTheme::Balanced
        );
    }

    #[test]
    fn balanced_theme_uses_palette_tone() {
        let css = tab_strip_theme_css(&spec(), BigTabStripTheme::Balanced);
        assert!(css.contains("color-mix(in srgb, #f8f8f2, transparent 82%)"));
        assert!(css.contains(".big-main-window .big-tab-new-row"));
    }

    #[test]
    fn follow_system_tones_with_current_color() {
        let mut s = spec();
        s.follow_system = true;
        let css = tab_strip_theme_css(&s, BigTabStripTheme::Underline);
        assert!(css.contains("color-mix(in srgb, currentColor, transparent 94%)"));
        assert!(!css.contains("#f8f8f2"));
    }

    #[test]
    fn elevated_dark_scheme_paints_translucent_body_gradient() {
        let css = tab_strip_theme_css(&spec(), BigTabStripTheme::Elevated);
        // body_alpha(40) = 1 - 0.4^1.6*0.8 ≈ 0.8153
        assert!(css.contains("rgba(40, 42, 54, 0.8153)"), "{css}");
        // dark scheme → tone-tinted halo, not black cast shadow
        assert!(css.contains("color-mix(in srgb, #f8f8f2, transparent 75%)"));
    }

    #[test]
    fn integrated_follow_system_uses_named_view_color_and_keeps_headerbar() {
        let mut s = spec();
        s.follow_system = true;
        let css = tab_strip_theme_css(&s, BigTabStripTheme::Integrated);
        assert!(css.contains("background-color: @view_bg_color;"));
        assert!(!css.contains("headerbar.main-header-bar"));
        assert!(!css.contains("#282a36"));
    }

    #[test]
    fn integrated_header_and_tab_paints_composite_to_body() {
        // Golden paints for the dark `spec()` (bg #282a36, chrome 15%, body 40%).
        // The documented un-premultiply compositing must reproduce these exactly,
        // so any perturbation of the integrated paint arithmetic is caught:
        //   header = headerbar tint (bg lifted 4% toward white) at a_t*0.9*opacity%
        //   tab    = paint that, composited over the header, reproduces the body.
        let (header, tab) = integrated_paints(&spec());
        assert_eq!(header, "rgba(49, 51, 62, 0.7044)");
        assert_eq!(tab, "rgba(30, 32, 45, 0.3752)");

        // Dark scheme (luminance < 0.5): the headerbar tint moves each channel 4%
        // toward white — `c + (1 - c) * 0.04` — pinning the else-branch formula.
        let (r_hb, g_hb, b_hb) = integrated_headerbar_rgb(&spec());
        let toward_white = |c: f64| c + (1.0 - c) * 0.04;
        assert!((r_hb - toward_white(40.0 / 255.0)).abs() < 1e-12);
        assert!((g_hb - toward_white(42.0 / 255.0)).abs() < 1e-12);
        assert!((b_hb - toward_white(54.0 / 255.0)).abs() < 1e-12);

        // Both golden paints reach the rendered stylesheet in their proper blocks.
        let css = tab_strip_theme_css(&spec(), BigTabStripTheme::Integrated);
        assert!(
            css.contains("background-color: rgba(49, 51, 62, 0.7044)"),
            "{css}"
        );
        assert!(
            css.contains("background-color: rgba(30, 32, 45, 0.3752)"),
            "{css}"
        );
        assert!(css.contains("headerbar.main-header-bar"));
        assert!(css.contains("popover.menu"));
    }

    #[test]
    fn integrated_headerbar_light_scheme_shades_toward_black() {
        // A light scheme (luminance >= 0.5) exercises the OTHER branch: each
        // channel is multiplied by `1 - 0.04`, shading the headerbar toward black.
        let light = BigChromeSpec {
            palette: BigChromePalette {
                bg: "#c8d2e6".to_owned(),
                ..spec().palette
            },
            ..spec()
        };
        assert!(light.palette.luminance() >= 0.5);
        let (r_hb, g_hb, b_hb) = integrated_headerbar_rgb(&light);
        let toward_black = |c: f64| c * (1.0 - 0.04);
        assert!((r_hb - toward_black(200.0 / 255.0)).abs() < 1e-12);
        assert!((g_hb - toward_black(210.0 / 255.0)).abs() < 1e-12);
        assert!((b_hb - toward_black(230.0 / 255.0)).abs() < 1e-12);
    }

    #[test]
    fn luminance_rejects_malformed_hex_as_black() {
        // `hex_to_rgb_f64` must reject anything that is not exactly `#rrggbb`; a
        // rejected value falls back to black (luminance 0.0). Each malformed input
        // isolates one disjunct of the validation guard.
        let with_bg = |bg: &str| BigChromePalette {
            bg: bg.to_owned(),
            ..spec().palette
        };
        // len == 7, all hexdigits, but no leading '#'.
        assert_eq!(with_bg("0123456").luminance(), 0.0);
        // leading '#', all hexdigits, but too long.
        assert_eq!(with_bg("#1234567").luminance(), 0.0);
        // right shape but a non-hex digit.
        assert_eq!(with_bg("#12345g").luminance(), 0.0);
        // a real color is not black.
        assert!(with_bg("#ffffff").luminance() > 0.99);
    }
}
