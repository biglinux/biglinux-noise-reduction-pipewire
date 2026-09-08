// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Style-token regions: the advanced-personalization substrate.
//!
//! An app declares the named [style regions](BigStyleRegion) it opts into (a
//! compile-time catalog), and stores the per-region token values in a
//! [`BigRegionStyleSet`]. Rendering the whole set to scoped CSS is a single
//! [`BigRegionStyleSet::to_css`] call, generalizing a per-surface CSS regen.
//!
//! The token value type is [`BigSurfaceStyle`] verbatim: the criterion-9 token
//! set (radius/padding/spacing/background/gradient/text/border/shadow). Note
//! that `spacing` is a widget-layer property (GTK box `set_spacing`), not a CSS
//! declaration, so it is intentionally absent from the emitted CSS — see
//! [`surface_css`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{BigSurfaceStyle, surface_css, surface_css_with_text_selector};

/// A named style region of an app — a criterion-9 "per program region".
///
/// The app declares which regions it opts into as a compile-time catalog; each
/// entry maps a stable `id` (the persisted key in a [`BigRegionStyleSet`]) to a
/// `css_scope` (the full CSS scope selector the region's rules are emitted
/// under, for example `.app-card` or `window.big-shell`) and a `label_msgid`
/// (the gettext message id for the region's user-facing name).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigStyleRegion {
    /// Stable region id; the persisted key in a [`BigRegionStyleSet`].
    pub id: &'static str,
    /// Full CSS scope selector the region's rules are emitted under, for
    /// example `.app-card` or `window.big-shell`.
    pub css_scope: &'static str,
    /// Gettext message id for the region's user-facing label.
    pub label_msgid: &'static str,
    /// Text-selector strategy for this region's cascading text declarations.
    ///
    /// `None` (the default) emits text under the universal descendant selector
    /// `{css_scope} *`, matching [`surface_css`]. `Some(sub_selector)` instead
    /// scopes text to an explicit descendant selector — the text selector
    /// becomes `{css_scope} {sub_selector}` — matching the shape of
    /// [`surface_css_with_text_selector`] (for example
    /// `Some(".big-shell-surface-text")` renders under
    /// `{css_scope} .big-shell-surface-text`). Use it when a region carries an
    /// explicitly-classed text node instead of theming every descendant.
    pub text_selector: Option<&'static str>,
}

/// The app's per-region token values — the persisted value type.
///
/// Reuses [`BigSurfaceStyle`] verbatim as the token set for each region. Keys
/// are region ids from the app's [`BigStyleRegion`] catalog. The map is ordered
/// so serialization and iteration are deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BigRegionStyleSet {
    /// Per-region token values, keyed by region id.
    pub regions: BTreeMap<String, BigSurfaceStyle>,
}

impl BigRegionStyleSet {
    /// An empty region style set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `style` for `region_id`, replacing any previous value.
    pub fn set(&mut self, region_id: impl Into<String>, style: BigSurfaceStyle) {
        self.regions.insert(region_id.into(), style);
    }

    /// The stored token values for `region_id`, if any.
    #[must_use]
    pub fn get(&self, region_id: &str) -> Option<&BigSurfaceStyle> {
        self.regions.get(region_id)
    }

    /// Deterministic CSS for the declared regions, each scoped.
    ///
    /// Iterates `catalog` in order and emits scoped CSS for every region that
    /// has a stored style, using that region's `css_scope`. A region's
    /// [`text_selector`](BigStyleRegion::text_selector) picks the text-cascade
    /// shape: `None` renders through [`surface_css`] (universal `{css_scope} *`),
    /// `Some(sub_selector)` renders through [`surface_css_with_text_selector`]
    /// with the text selector `{css_scope} {sub_selector}`. Catalog regions with
    /// no stored value are skipped; stored ids that are not in the catalog are
    /// ignored (the catalog owns the scope selectors).
    #[must_use]
    pub fn to_css(&self, catalog: &[BigStyleRegion]) -> String {
        let mut css = String::new();
        for region in catalog {
            if let Some(style) = self.regions.get(region.id) {
                match region.text_selector {
                    None => css.push_str(&surface_css(region.css_scope, style)),
                    Some(sub_selector) => {
                        let text_selector = format!("{} {sub_selector}", region.css_scope);
                        css.push_str(&surface_css_with_text_selector(
                            region.css_scope,
                            &text_selector,
                            style,
                        ));
                    }
                }
            }
        }
        css
    }

    /// Returns this set with every region's style normalized to documented
    /// ranges (see [`BigSurfaceStyle::clamped`]).
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            regions: self
                .regions
                .into_iter()
                .map(|(id, style)| (id, style.clamped()))
                .collect(),
        }
    }
}

/// A named, shippable [`BigRegionStyleSet`] — the preset layer.
///
/// Generalizes a builtin-preset system (a named bundle of per-region token
/// values an app can ship and offer the user), independent of any UI toolkit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BigStylePreset {
    /// Stable preset id.
    pub id: String,
    /// The preset's per-region token values.
    pub styles: BigRegionStyleSet,
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &[BigStyleRegion] = &[
        BigStyleRegion {
            id: "clock",
            css_scope: ".big-shell-clock",
            label_msgid: "Clock",
            text_selector: None,
        },
        BigStyleRegion {
            id: "menu",
            css_scope: "window.big-shell-menu",
            label_msgid: "Menu",
            text_selector: None,
        },
    ];

    #[test]
    fn to_css_emits_one_scoped_block_per_declared_region() {
        let mut styles = BigRegionStyleSet::new();
        styles.set(
            "clock",
            BigSurfaceStyle {
                radius: 6,
                padding: 4,
                ..BigSurfaceStyle::default()
            },
        );
        styles.set(
            "menu",
            BigSurfaceStyle {
                radius: 18,
                padding: 10,
                ..BigSurfaceStyle::default()
            },
        );

        let css = styles.to_css(CATALOG);

        // Catalog order: clock before menu, each under its own scope selector.
        let clock_at = css.find(".big-shell-clock {").expect("clock block present");
        let menu_at = css
            .find("window.big-shell-menu {")
            .expect("menu block present");
        assert!(clock_at < menu_at, "catalog order preserved: {css}");
        assert!(css.contains(".big-shell-clock {\n  background-color: alpha(#2a2a30, 0.920);\n  border-radius: 6px;\n  padding: 4px;\n}\n"), "{css}");
        assert!(css.contains("window.big-shell-menu {\n  background-color: alpha(#2a2a30, 0.920);\n  border-radius: 18px;\n  padding: 10px;\n}\n"), "{css}");
    }

    #[test]
    fn to_css_class_text_selector_matches_surface_css_with_text_selector() {
        // A region opting into an explicit descendant text class must emit
        // byte-for-byte what `surface_css_with_text_selector` produces for the
        // same scope + `{css_scope} {sub_selector}` text selector — this is the
        // shell's `.{scope} .big-shell-surface-text` shape.
        const CLASS_CATALOG: &[BigStyleRegion] = &[BigStyleRegion {
            id: "clock",
            css_scope: ".big-shell-clock",
            label_msgid: "Clock",
            text_selector: Some(".big-shell-surface-text"),
        }];

        let style = BigSurfaceStyle {
            radius: 18,
            padding: 6,
            spacing: 7,
            background_opacity: 50,
            text: Some(crate::BigSurfaceText {
                color: Some("#f2f2f2".to_owned()),
                font_family: Some("Noto Sans".to_owned()),
                size: 13,
                font_bold: true,
                font_italic: true,
                font_underline: true,
            }),
            ..BigSurfaceStyle::default()
        };
        let mut styles = BigRegionStyleSet::new();
        styles.set("clock", style.clone());

        let expected = surface_css_with_text_selector(
            ".big-shell-clock",
            ".big-shell-clock .big-shell-surface-text",
            &style,
        );
        let css = styles.to_css(CLASS_CATALOG);
        assert_eq!(css, expected, "{css}");
        // The class selector is emitted, not the universal-descendant one.
        assert!(
            css.contains(".big-shell-clock .big-shell-surface-text {"),
            "{css}"
        );
        assert!(!css.contains(".big-shell-clock * {"), "{css}");
    }

    #[test]
    fn to_css_default_text_selector_is_universal_descendant() {
        // With `text_selector: None` the text cascade stays the universal
        // `{css_scope} *` shape from `surface_css` — the default is unchanged.
        const UNIVERSAL_CATALOG: &[BigStyleRegion] = &[BigStyleRegion {
            id: "clock",
            css_scope: ".big-shell-clock",
            label_msgid: "Clock",
            text_selector: None,
        }];
        let mut styles = BigRegionStyleSet::new();
        styles.set(
            "clock",
            BigSurfaceStyle {
                text: Some(crate::BigSurfaceText::default()),
                ..Default::default()
            },
        );
        let css = styles.to_css(UNIVERSAL_CATALOG);
        assert!(css.contains(".big-shell-clock * {"), "{css}");
        assert!(!css.contains(".big-shell-surface-text"), "{css}");
    }

    #[test]
    fn to_css_omits_spacing_by_design() {
        // `spacing` is a widget property (GTK box set_spacing), never CSS.
        let mut styles = BigRegionStyleSet::new();
        styles.set(
            "clock",
            BigSurfaceStyle {
                spacing: 24,
                ..BigSurfaceStyle::default()
            },
        );
        let css = styles.to_css(CATALOG);
        assert!(css.contains("border-radius:"), "{css}");
        assert!(css.contains("padding:"), "{css}");
        assert!(
            !css.contains("spacing"),
            "spacing must not reach CSS: {css}"
        );
        assert!(!css.contains("gap"), "no gap declaration either: {css}");
    }

    #[test]
    fn to_css_skips_regions_without_a_stored_value() {
        let mut styles = BigRegionStyleSet::new();
        styles.set("menu", BigSurfaceStyle::default());
        let css = styles.to_css(CATALOG);
        assert!(
            !css.contains(".big-shell-clock"),
            "unset region absent: {css}"
        );
        assert!(
            css.contains("window.big-shell-menu {"),
            "set region present: {css}"
        );
    }

    #[test]
    fn get_and_set_round_trip() {
        let mut styles = BigRegionStyleSet::new();
        assert!(styles.get("clock").is_none());
        let style = BigSurfaceStyle {
            radius: 7,
            ..BigSurfaceStyle::default()
        };
        styles.set("clock", style.clone());
        assert_eq!(styles.get("clock"), Some(&style));
    }

    #[test]
    fn clamped_normalizes_every_region() {
        let mut styles = BigRegionStyleSet::new();
        styles.set(
            "clock",
            BigSurfaceStyle {
                radius: 999,
                padding: 999,
                ..BigSurfaceStyle::default()
            },
        );
        styles.set(
            "menu",
            BigSurfaceStyle {
                radius: 999,
                spacing: 999,
                ..BigSurfaceStyle::default()
            },
        );

        let styles = styles.clamped();

        assert_eq!(styles.get("clock").unwrap().radius, 32);
        assert_eq!(styles.get("clock").unwrap().padding, 32);
        assert_eq!(styles.get("menu").unwrap().radius, 32);
        assert_eq!(styles.get("menu").unwrap().spacing, 32);
    }

    #[test]
    fn region_style_set_serde_is_a_plain_map() {
        let mut styles = BigRegionStyleSet::new();
        styles.set("clock", BigSurfaceStyle::default());
        let json = serde_json::to_string(&styles).unwrap();
        // `#[serde(transparent)]` → the map is the top-level value, no wrapper.
        assert!(json.starts_with("{\"clock\":"), "{json}");
        let back: BigRegionStyleSet = serde_json::from_str(&json).unwrap();
        assert_eq!(back, styles);
    }

    #[test]
    fn preset_bundles_a_named_region_set() {
        let mut styles = BigRegionStyleSet::new();
        styles.set("clock", BigSurfaceStyle::default());
        let preset = BigStylePreset {
            id: "midnight".to_owned(),
            styles,
        };
        let json = serde_json::to_string(&preset).unwrap();
        let back: BigStylePreset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, preset);
        assert_eq!(back.id, "midnight");
    }
}
