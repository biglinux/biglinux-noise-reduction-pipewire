// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Suite-wide tab personalization: shared settings keys, declarative
//! preference rows, and the settings→chrome glue for
//! [`BigTabStrip`](crate::layout::tab_strip_host::BigTabStrip).
//!
//! Any app using the shared tab strip lists [`TAB_PREFERENCE_ROWS`] in its
//! preferences and calls [`tab_chrome_settings`] on boot and on settings
//! change — alignment/theme/expand/close-mode personalization then works
//! identically across the suite.
//!
//! Row msgids live in the `big-components` textdomain (chain the app
//! translator with [`crate::i18n::t`]). The didactic SVGs
//! (`tab-alignment.svg`, `tab-style.svg`) must exist under the host app's
//! illustration resource prefix.

use relm4::gtk;

use crate::i18n::gettext_noop;
use crate::input::preference_rows::{BigPrefRow, BigPreferenceStore};

/// Settings key: tab strip alignment (`left`/`center`).
pub const KEY_TAB_ALIGNMENT: &str = "tab_alignment";
/// Settings key: visual tab theme.
pub const KEY_TAB_THEME: &str = "tab_theme";
/// Settings key: expand tabs evenly across the strip.
pub const KEY_TAB_EXPAND_EVENLY: &str = "tab_expand_evenly";
/// Settings key: close-button visibility mode.
pub const KEY_TAB_CLOSE_MODE: &str = "tab_close_button_mode";
/// Settings key: where the tab strip is mounted in the window.
pub const KEY_TAB_STRIP_POSITION: &str = "tab_strip_position";
/// Settings key: width of a vertical (side) tab strip, logical pixels.
pub const KEY_TAB_STRIP_SIDE_WIDTH: &str = "tab_strip_side_width";

/// Default width of a vertical (side) tab strip, logical pixels.
pub const TAB_STRIP_SIDE_WIDTH_DEFAULT: i32 = 176;
/// Resize bounds for the side tab strip, logical pixels.
pub const TAB_STRIP_SIDE_WIDTH_RANGE: (i32, i32) = (120, 480);

/// Clamp a side-strip width into [`TAB_STRIP_SIDE_WIDTH_RANGE`].
#[must_use]
pub fn clamp_tab_strip_side_width(width: i32) -> i32 {
    width.clamp(TAB_STRIP_SIDE_WIDTH_RANGE.0, TAB_STRIP_SIDE_WIDTH_RANGE.1)
}

/// Read the persisted side-strip width, clamped, with the suite default.
#[must_use]
pub fn tab_strip_side_width_setting(store: &dyn BigPreferenceStore) -> i32 {
    store
        .get_i64(KEY_TAB_STRIP_SIDE_WIDTH)
        .and_then(|width| i32::try_from(width).ok())
        .map_or(TAB_STRIP_SIDE_WIDTH_DEFAULT, clamp_tab_strip_side_width)
}

/// All suite tab personalization keys — the settings-listener filter for
/// `apply_tab_personalization`.
pub const TAB_SETTING_KEYS: [&str; 6] = [
    KEY_TAB_STRIP_POSITION,
    KEY_TAB_STRIP_SIDE_WIDTH,
    KEY_TAB_ALIGNMENT,
    KEY_TAB_THEME,
    KEY_TAB_EXPAND_EVENLY,
    KEY_TAB_CLOSE_MODE,
];

/// Persisted values for [`KEY_TAB_STRIP_POSITION`].
pub const TAB_STRIP_POSITION_KEYS: [&str; 5] = ["header", "top", "bottom", "start", "end"];
/// Labels for [`TAB_STRIP_POSITION_KEYS`] (big-components msgids).
pub const TAB_STRIP_POSITION_LABELS: [&str; 5] = [
    gettext_noop("Header bar"),
    gettext_noop("Top"),
    gettext_noop("Bottom"),
    gettext_noop("Left side"),
    gettext_noop("Right side"),
];

/// Where the window mounts the tab strip. `Start`/`End` render the strip
/// VERTICALLY — tabs stacked one above the other like side-tab editors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BigTabStripPosition {
    /// Inside the header bar title area (suite default).
    #[default]
    Header,
    /// Horizontal strip above the content.
    Top,
    /// Horizontal strip below the content.
    Bottom,
    /// Vertical strip on the leading edge.
    Start,
    /// Vertical strip on the trailing edge.
    End,
}

impl BigTabStripPosition {
    /// Parse a persisted [`KEY_TAB_STRIP_POSITION`] value (unknown → Header).
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value {
            "top" => Self::Top,
            "bottom" => Self::Bottom,
            "start" => Self::Start,
            "end" => Self::End,
            _ => Self::Header,
        }
    }

    /// Whether this placement renders the strip vertically.
    #[must_use]
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Start | Self::End)
    }
}

/// Read the persisted tab strip position, falling back to the header bar.
#[must_use]
pub fn tab_strip_position_setting(store: &dyn BigPreferenceStore) -> BigTabStripPosition {
    store
        .get_string(KEY_TAB_STRIP_POSITION)
        .map_or(BigTabStripPosition::Header, |value| {
            BigTabStripPosition::from_setting(&value)
        })
}

/// Persisted values for [`KEY_TAB_ALIGNMENT`].
pub const TAB_ALIGNMENT_KEYS: [&str; 2] = ["left", "center"];
/// Labels for [`TAB_ALIGNMENT_KEYS`] (big-components msgids).
pub const TAB_ALIGNMENT_LABELS: [&str; 2] = [gettext_noop("Left"), gettext_noop("Center")];

/// Persisted values for [`KEY_TAB_THEME`].
pub const TAB_THEME_KEYS: [&str; 5] =
    ["balanced", "compact", "underline", "integrated", "elevated"];
/// Labels for [`TAB_THEME_KEYS`] (big-components msgids).
pub const TAB_THEME_LABELS: [&str; 5] = [
    gettext_noop("Balanced"),
    gettext_noop("Compact"),
    gettext_noop("Underline"),
    gettext_noop("Integrated"),
    gettext_noop("Elevated"),
];

/// Persisted values for [`KEY_TAB_CLOSE_MODE`].
pub const TAB_CLOSE_MODE_KEYS: [&str; 3] = ["always", "hover", "active"];
/// Labels for [`TAB_CLOSE_MODE_KEYS`] (big-components msgids).
pub const TAB_CLOSE_MODE_LABELS: [&str; 3] = [
    gettext_noop("Always"),
    gettext_noop("On hover"),
    gettext_noop("Active tab only"),
];

/// Default persisted tab theme.
pub const TAB_THEME_DEFAULT: &str = "balanced";

/// The five suite-wide tab preference rows, in display order.
pub const TAB_PREFERENCE_ROWS: [BigPrefRow; 5] = [
    BigPrefRow::keyed_dropdown(
        "tab-alignment.svg",
        gettext_noop("Tab Position"),
        gettext_noop(
            "Where the tab strip lives: in the header bar, above or below the content, or as a vertical strip on either side of the window.",
        ),
        KEY_TAB_STRIP_POSITION,
        &TAB_STRIP_POSITION_KEYS,
        "header",
        &TAB_STRIP_POSITION_LABELS,
    ),
    BigPrefRow::keyed_dropdown(
        "tab-alignment.svg",
        gettext_noop("Tab Alignment"),
        gettext_noop(
            "Where new tabs anchor inside the strip. Left feels classic; centre keeps the row balanced when you rarely have many tabs.",
        ),
        KEY_TAB_ALIGNMENT,
        &TAB_ALIGNMENT_KEYS,
        "center",
        &TAB_ALIGNMENT_LABELS,
    ),
    BigPrefRow::keyed_dropdown(
        "tab-style.svg",
        gettext_noop("Tab Theme"),
        gettext_noop(
            "Visual weight of the tab strip. Balanced is the default; Compact saves vertical space; Underline keeps the strip flat; Integrated blends the active tab into the terminal; Elevated lifts the active tab with a soft drop shadow.",
        ),
        KEY_TAB_THEME,
        &TAB_THEME_KEYS,
        TAB_THEME_DEFAULT,
        &TAB_THEME_LABELS,
    ),
    BigPrefRow::switch(
        "tab-alignment.svg",
        gettext_noop("Expand Tabs Evenly"),
        gettext_noop("Use the full tab strip width and divide it equally between open tabs."),
        KEY_TAB_EXPAND_EVENLY,
        false,
    ),
    BigPrefRow::keyed_dropdown(
        "tab-style.svg",
        gettext_noop("Tab Close Button"),
        gettext_noop("Choose when close buttons are visible on tabs."),
        KEY_TAB_CLOSE_MODE,
        &TAB_CLOSE_MODE_KEYS,
        "always",
        &TAB_CLOSE_MODE_LABELS,
    ),
];

/// Read the strip-wide chrome settings (expand-evenly + close mode) from a
/// preference store, with suite defaults. Feed the result to
/// `apply_tab_chrome` on the app's tab strip or workspace shell.
#[must_use]
pub fn tab_chrome_settings(store: &dyn BigPreferenceStore) -> (bool, String) {
    let expand_evenly = store.get_bool(KEY_TAB_EXPAND_EVENLY).unwrap_or(false);
    let close_mode = store
        .get_string(KEY_TAB_CLOSE_MODE)
        .unwrap_or_else(|| "always".to_owned());
    (expand_evenly, close_mode)
}

/// Read the persisted tab theme, falling back to the suite default.
#[must_use]
pub fn tab_theme_setting(store: &dyn BigPreferenceStore) -> String {
    store
        .get_string(KEY_TAB_THEME)
        .unwrap_or_else(|| TAB_THEME_DEFAULT.to_owned())
}

/// Map the persisted alignment value to the tab bar's horizontal align.
#[must_use]
pub fn tab_alignment_halign(store: &dyn BigPreferenceStore) -> gtk::Align {
    match store.get_string(KEY_TAB_ALIGNMENT).as_deref() {
        Some("left") => gtk::Align::Start,
        _ => gtk::Align::Center,
    }
}

/// Generate the tab-theme stylesheet for a shared
/// [`BigTabStrip`](crate::layout::tab_strip_host::BigTabStrip) using the
/// canonical `big-tab-button` classes.
///
/// Delegates to the suite pipeline
/// (`big_appearance::chrome::tab_strip_theme_css`) in follow-system mode:
/// tone comes from `currentColor`/named colors, so the strip tracks the
/// GTK theme. Apps with their own palette (VTE schemes) build a real
/// `BigChromeSpec` and call the generator directly instead. Unknown theme
/// keys render Balanced. Installed via
/// [`crate::theme::inject_palette_css`] on boot and whenever
/// [`KEY_TAB_THEME`] changes.
#[must_use]
pub fn tab_theme_css(theme: &str) -> String {
    use big_appearance::chrome::{
        BigChromePalette, BigChromeSpec, BigTabStripTheme, tab_strip_theme_css,
    };
    let spec = BigChromeSpec {
        palette: BigChromePalette {
            bg: String::new(),
            fg: String::new(),
            header_bg: String::new(),
            chrome_bg: String::new(),
        },
        chrome_transparency: 0,
        body_transparency: 0,
        follow_system: true,
    };
    tab_strip_theme_css(&spec, BigTabStripTheme::from_key(theme))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::preference_rows::BigPrefControl;

    struct EmptyStore;
    impl BigPreferenceStore for EmptyStore {
        fn get_bool(&self, _: &str) -> Option<bool> {
            None
        }
        fn get_i64(&self, _: &str) -> Option<i64> {
            None
        }
        fn get_f64(&self, _: &str) -> Option<f64> {
            None
        }
        fn get_string(&self, _: &str) -> Option<String> {
            None
        }
        fn set_bool(&self, _: &'static str, _: bool) {}
        fn set_i64(&self, _: &'static str, _: i64) {}
        fn set_f64(&self, _: &'static str, _: f64) {}
        fn set_string(&self, _: &'static str, _: &str) {}
    }

    #[test]
    fn rows_cover_the_four_suite_keys_in_order() {
        let keys: Vec<&str> = TAB_PREFERENCE_ROWS
            .iter()
            .map(|row| match row.control {
                BigPrefControl::Switch { key, .. } | BigPrefControl::KeyedDropdown { key, .. } => {
                    key
                }
                _ => panic!("unexpected control kind in tab rows"),
            })
            .collect();
        // Every setting key has a row EXCEPT the side width — that one is
        // set by dragging the strip's resize grip, not from preferences.
        let expected: Vec<&str> = TAB_SETTING_KEYS
            .into_iter()
            .filter(|key| *key != KEY_TAB_STRIP_SIDE_WIDTH)
            .collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn chrome_settings_fall_back_to_suite_defaults() {
        let (expand, close) = tab_chrome_settings(&EmptyStore);
        assert!(!expand);
        assert_eq!(close, "always");
    }

    #[test]
    fn theme_setting_falls_back_to_balanced() {
        assert_eq!(tab_theme_setting(&EmptyStore), TAB_THEME_DEFAULT);
    }

    #[test]
    fn alignment_falls_back_to_center() {
        assert_eq!(tab_alignment_halign(&EmptyStore), gtk::Align::Center);
    }

    struct LeftStore;
    impl BigPreferenceStore for LeftStore {
        fn get_bool(&self, _: &str) -> Option<bool> {
            None
        }
        fn get_i64(&self, _: &str) -> Option<i64> {
            None
        }
        fn get_f64(&self, _: &str) -> Option<f64> {
            None
        }
        fn get_string(&self, key: &str) -> Option<String> {
            (key == KEY_TAB_ALIGNMENT).then(|| "left".to_owned())
        }
        fn set_bool(&self, _: &'static str, _: bool) {}
        fn set_i64(&self, _: &'static str, _: i64) {}
        fn set_f64(&self, _: &'static str, _: f64) {}
        fn set_string(&self, _: &'static str, _: &str) {}
    }

    #[test]
    fn alignment_left_maps_to_start() {
        assert_eq!(tab_alignment_halign(&LeftStore), gtk::Align::Start);
    }

    #[test]
    fn every_theme_key_renders_a_distinct_canonical_stylesheet() {
        let sheets: Vec<String> = TAB_THEME_KEYS.iter().map(|k| tab_theme_css(k)).collect();
        for (key, sheet) in TAB_THEME_KEYS.iter().zip(&sheets) {
            assert!(
                sheet.contains(".big-tab-button.active"),
                "{key} must style the canonical active class"
            );
            assert!(
                !sheet.contains("custom-tab-button"),
                "{key} must not leak terminal-local class names"
            );
        }
        for (i, a) in sheets.iter().enumerate() {
            for b in &sheets[i + 1..] {
                assert_ne!(a, b, "theme variants must differ");
            }
        }
    }

    #[test]
    fn unknown_theme_key_falls_back_to_balanced() {
        assert_eq!(tab_theme_css("nonsense"), tab_theme_css("balanced"));
    }

    #[test]
    fn underline_and_compact_lean_on_the_accent_color() {
        assert!(tab_theme_css("underline").contains("@accent_bg_color"));
        assert!(tab_theme_css("compact").contains("@accent_bg_color"));
    }

    #[test]
    fn strip_position_parses_every_persisted_key() {
        let positions: Vec<BigTabStripPosition> = TAB_STRIP_POSITION_KEYS
            .iter()
            .map(|key| BigTabStripPosition::from_setting(key))
            .collect();
        assert_eq!(
            positions,
            [
                BigTabStripPosition::Header,
                BigTabStripPosition::Top,
                BigTabStripPosition::Bottom,
                BigTabStripPosition::Start,
                BigTabStripPosition::End,
            ]
        );
        assert_eq!(
            BigTabStripPosition::from_setting("nonsense"),
            BigTabStripPosition::Header
        );
    }

    #[test]
    fn only_side_positions_are_vertical() {
        assert!(BigTabStripPosition::Start.is_vertical());
        assert!(BigTabStripPosition::End.is_vertical());
        assert!(!BigTabStripPosition::Header.is_vertical());
        assert!(!BigTabStripPosition::Top.is_vertical());
        assert!(!BigTabStripPosition::Bottom.is_vertical());
    }

    #[test]
    fn strip_position_setting_defaults_to_header() {
        assert_eq!(
            tab_strip_position_setting(&EmptyStore),
            BigTabStripPosition::Header
        );
    }
}
