// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Paint-ownership contract for translucent standalone windows.
//!
//! A translucent (blurred) window renders correctly only under SINGLE-PAINT
//! DISCIPLINE: every visible band — the chrome bands (headerbar, sidebar,
//! separators) and the content body — is filled by exactly one CSS rule, and
//! every nested layer is cleared to `transparent` so the owning band shows
//! through. Two translucent paints stacked on the same pixels multiply toward
//! opaque and destroy the blur (the 2026-07-05 file-manager regression class).
//!
//! [`BigTranslucentSurfacePlan`] encodes that invariant in the type system. The
//! adopting app supplies its own scopes ([`BigPaintBand`] selectors) and how
//! each is filled ([`BigPaintFill`]); construction rejects any selector that
//! appears in more than one band, so an adopter cannot stack translucent paints
//! by accident. [`BigTranslucentSurfacePlan::to_css`] then emits one rule per
//! band, in band order, painting every scope exactly once.
//!
//! The type is deliberately app-agnostic: it knows nothing about file-manager
//! selectors, interactive (hover/selected/focus) styling, or separator sizing.
//! The caller resolves the two fill strings (chrome band + body) from its own
//! transparency settings — for BigLinux chrome via the
//! [`crate::chrome`] helpers — and appends any non-paint styling
//! after [`to_css`](BigTranslucentSurfacePlan::to_css). GTK-free by design: pure
//! data plus deterministic string emission, validated without a display.

use std::collections::HashSet;
use std::fmt;

/// How a scope is filled in a translucent window.
///
/// The two paint values are resolved once by the caller and supplied to
/// [`BigTranslucentSurfacePlan::new`]; this enum only selects which of them (or
/// a clear) a given band uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigPaintFill {
    /// A chrome band: the `headerbar_bg`-derived fill, mixed toward transparent
    /// at the chrome opacity. Owns the pixels behind headerbars, sidebars, and
    /// separators.
    ChromeBand,
    /// The content body: the `window_bg` fill at the body alpha. Owns the pixels
    /// behind the primary content column.
    Body,
    /// Cleared to `background-color: transparent` — a nested layer, so the single
    /// owner band shows through instead of stacking a second paint.
    Transparent,
}

/// One paint band: one or more CSS selectors filled exactly one way.
///
/// Every selector in a plan must be unique across all bands; that uniqueness is
/// what makes double-painting structurally impossible (see
/// [`BigTranslucentSurfacePlan::new`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPaintBand {
    /// The CSS selectors this band paints. Emitted comma-joined as one rule.
    pub selectors: Vec<String>,
    /// How every selector in this band is filled.
    pub fill: BigPaintFill,
}

impl BigPaintBand {
    /// Build a band from an iterator of selectors and a single fill.
    #[must_use]
    pub fn new(selectors: impl IntoIterator<Item = impl Into<String>>, fill: BigPaintFill) -> Self {
        Self {
            selectors: selectors.into_iter().map(Into::into).collect(),
            fill,
        }
    }
}

/// Why a [`BigTranslucentSurfacePlan`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BigPaintPlanError {
    /// A selector appears in more than one band. Painting the same scope twice
    /// stacks translucent fills and multiplies toward opaque — the invariant the
    /// plan exists to prevent. Carries the offending selector.
    SelectorPaintedTwice {
        /// The selector that was found in a second band.
        selector: String,
    },
}

impl fmt::Display for BigPaintPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelectorPaintedTwice { selector } => write!(
                f,
                "selector `{selector}` is painted by more than one band \
                 (single-paint invariant violated)"
            ),
        }
    }
}

impl std::error::Error for BigPaintPlanError {}

/// A translucent window's complete paint plan.
///
/// Built from the resolved chrome-band fill, the resolved body fill, and the
/// ordered bands. Construction enforces the single-paint invariant: no selector
/// may appear in two bands. [`to_css`](Self::to_css) emits one rule per band, in
/// band order, so the stylesheet paints every scope exactly once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTranslucentSurfacePlan {
    chrome_fill_css: String,
    body_fill_css: String,
    bands: Vec<BigPaintBand>,
}

impl BigTranslucentSurfacePlan {
    /// Build a plan from the resolved chrome-band fill, the resolved body fill,
    /// and the bands.
    ///
    /// The caller resolves `chrome_fill_css` and `body_fill_css` from its own
    /// transparency settings (BigLinux chrome uses the
    /// [`crate::chrome`] helpers). Each band maps its fill to one
    /// of them: [`BigPaintFill::ChromeBand`] to `chrome_fill_css`,
    /// [`BigPaintFill::Body`] to `body_fill_css`, and [`BigPaintFill::Transparent`]
    /// to the literal `transparent`.
    ///
    /// # Errors
    ///
    /// Returns [`BigPaintPlanError::SelectorPaintedTwice`] if any selector appears
    /// in more than one band — the structural single-paint guarantee. Selectors
    /// are compared after trimming surrounding whitespace.
    pub fn new(
        chrome_fill_css: String,
        body_fill_css: String,
        bands: Vec<BigPaintBand>,
    ) -> Result<Self, BigPaintPlanError> {
        let mut painted: HashSet<&str> = HashSet::new();
        for band in &bands {
            for selector in &band.selectors {
                let scope = selector.trim();
                if !painted.insert(scope) {
                    return Err(BigPaintPlanError::SelectorPaintedTwice {
                        selector: scope.to_owned(),
                    });
                }
            }
        }
        Ok(Self {
            chrome_fill_css,
            body_fill_css,
            bands,
        })
    }

    /// Emit the plan as CSS: `background-color: <fill>; background-image: none;`
    /// per band, one rule per band, selectors comma-joined, in band order.
    ///
    /// Deterministic — the output depends only on the plan's fills and band
    /// order. Non-paint styling (interactive states, separator sizing) is the
    /// adopting app's responsibility, appended after this string.
    #[must_use]
    pub fn to_css(&self) -> String {
        let mut css = String::new();
        for band in &self.bands {
            let fill = match band.fill {
                BigPaintFill::ChromeBand => self.chrome_fill_css.as_str(),
                BigPaintFill::Body => self.body_fill_css.as_str(),
                BigPaintFill::Transparent => "transparent",
            };
            css.push_str(&band.selectors.join(",\n"));
            css.push_str(" {\n    background-color: ");
            css.push_str(fill);
            css.push_str(";\n    background-image: none;\n}\n");
        }
        css
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_css_emits_background_and_clears_image_per_band() {
        let plan = BigTranslucentSurfacePlan::new(
            "CHROME".to_owned(),
            "BODY".to_owned(),
            vec![
                BigPaintBand::new(["a", "b"], BigPaintFill::ChromeBand),
                BigPaintBand::new(["c"], BigPaintFill::Body),
                BigPaintBand::new(["d"], BigPaintFill::Transparent),
            ],
        )
        .expect("distinct selectors build a plan");

        assert_eq!(
            plan.to_css(),
            "a,\nb {\n    background-color: CHROME;\n    background-image: none;\n}\n\
             c {\n    background-color: BODY;\n    background-image: none;\n}\n\
             d {\n    background-color: transparent;\n    background-image: none;\n}\n"
        );
    }

    #[test]
    fn transparent_fill_emits_the_transparent_keyword() {
        let plan = BigTranslucentSurfacePlan::new(
            "CHROME".to_owned(),
            "BODY".to_owned(),
            vec![BigPaintBand::new([".nested"], BigPaintFill::Transparent)],
        )
        .expect("plan");

        let css = plan.to_css();
        assert!(css.contains("background-color: transparent;"));
        assert!(!css.contains("CHROME"));
        assert!(!css.contains("BODY"));
    }

    #[test]
    fn new_rejects_a_selector_painted_by_two_bands() {
        let err = BigTranslucentSurfacePlan::new(
            "CHROME".to_owned(),
            "BODY".to_owned(),
            vec![
                BigPaintBand::new([".shared"], BigPaintFill::ChromeBand),
                BigPaintBand::new([".shared"], BigPaintFill::Transparent),
            ],
        )
        .expect_err("a selector in two bands violates single-paint");

        assert_eq!(
            err,
            BigPaintPlanError::SelectorPaintedTwice {
                selector: ".shared".to_owned(),
            }
        );
    }

    #[test]
    fn new_compares_selectors_after_trimming_whitespace() {
        let err = BigTranslucentSurfacePlan::new(
            "CHROME".to_owned(),
            "BODY".to_owned(),
            vec![
                BigPaintBand::new(["  .shared"], BigPaintFill::ChromeBand),
                BigPaintBand::new([".shared  "], BigPaintFill::Body),
            ],
        )
        .expect_err("whitespace-only differences name the same scope");

        assert_eq!(
            err,
            BigPaintPlanError::SelectorPaintedTwice {
                selector: ".shared".to_owned(),
            }
        );
    }

    /// The single-paint contract, ported from the file-manager `css.rs`
    /// `paints_each_band_exactly_once` test onto the type itself: with the
    /// file-manager surface shape (three chrome bands, one body band, cleared
    /// layers, every selector distinct) each fill appears exactly as many times
    /// as it owns bands.
    #[test]
    fn plan_paints_each_band_exactly_once() {
        let chrome = "color-mix(in srgb, @headerbar_bg_color 80%, transparent)".to_owned();
        let body = "alpha(@window_bg_color, 0.40)".to_owned();
        let plan = BigTranslucentSurfacePlan::new(
            chrome.clone(),
            body.clone(),
            vec![
                BigPaintBand::new(["headerbar"], BigPaintFill::ChromeBand),
                BigPaintBand::new(
                    ["headerbar > windowhandle", ".top-bar"],
                    BigPaintFill::Transparent,
                ),
                BigPaintBand::new([".main-box"], BigPaintFill::Body),
                BigPaintBand::new([".main-box .main-box"], BigPaintFill::Transparent),
                BigPaintBand::new(
                    [".sidebar", ".side-panel-header-band"],
                    BigPaintFill::ChromeBand,
                ),
                BigPaintBand::new(["paned > separator"], BigPaintFill::ChromeBand),
            ],
        )
        .expect("distinct selectors build a plan");

        let css = plan.to_css();
        assert_eq!(
            css.matches(&chrome).count(),
            3,
            "each chrome band must paint exactly once"
        );
        assert_eq!(
            css.matches(&body).count(),
            1,
            "the content body must paint exactly once"
        );
        assert_eq!(
            css.matches("background-color: transparent;").count(),
            2,
            "each cleared layer must clear exactly once"
        );
    }

    #[test]
    fn plan_error_display_names_the_offending_selector() {
        let err = BigPaintPlanError::SelectorPaintedTwice {
            selector: ".dup".to_owned(),
        };
        let message = err.to_string();
        assert!(message.contains(".dup"), "{message}");
        assert!(message.contains("single-paint invariant"), "{message}");
    }
}
