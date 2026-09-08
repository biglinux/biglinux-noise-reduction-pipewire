// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Suite-wide chrome-band personalization: shared settings keys, a declarative
//! preference row, and the settings→placement glue for a config-placed
//! [`BigChromeBandHost`](crate::layout::chrome_band_host::BigChromeBandHost).
//!
//! This is the toolbar-band analogue of the tab-strip position preference in
//! [`tab_prefs`](crate::layout::tab_prefs): an app lists
//! [`TOOLBAR_BAND_PREFERENCE_ROWS`] in its preferences and calls
//! [`toolbar_band_placement_setting`] on boot and on settings change to place
//! its command toolbar in the header edge docks or hide it.
//!
//! Row msgids live in the `big-components` textdomain (chain the app translator
//! with [`crate::i18n::t`]). The didactic SVG (`tab-alignment.svg`) must exist
//! under the host app's illustration resource prefix.

use big_app_kit::window_shell::BigBandPlacement;

use crate::i18n::gettext_noop;
use crate::input::preference_rows::{BigPrefRow, BigPreferenceStore};

/// Settings key: where the movable command toolbar band is mounted.
pub const KEY_TOOLBAR_BAND_PLACEMENT: &str = "toolbar_band_placement";

/// Persisted values for [`KEY_TOOLBAR_BAND_PLACEMENT`].
pub const TOOLBAR_BAND_PLACEMENT_KEYS: [&str; 5] = ["top", "bottom", "start", "end", "hidden"];
/// Labels for [`TOOLBAR_BAND_PLACEMENT_KEYS`] (big-components msgids).
pub const TOOLBAR_BAND_PLACEMENT_LABELS: [&str; 5] = [
    gettext_noop("Top"),
    gettext_noop("Bottom"),
    gettext_noop("Left side"),
    gettext_noop("Right side"),
    gettext_noop("Hidden"),
];

/// Default persisted toolbar band placement key.
pub const TOOLBAR_BAND_PLACEMENT_DEFAULT: &str = "top";

/// Parse a persisted [`KEY_TOOLBAR_BAND_PLACEMENT`] value into a
/// [`BigBandPlacement`] (unknown → [`BigBandPlacement::Top`]).
#[must_use]
pub fn toolbar_band_placement_from_setting(value: &str) -> BigBandPlacement {
    match value {
        "bottom" => BigBandPlacement::Bottom,
        "start" => BigBandPlacement::Start,
        "end" => BigBandPlacement::End,
        "hidden" => BigBandPlacement::Hidden,
        _ => BigBandPlacement::Top,
    }
}

/// Read the persisted toolbar band placement, falling back to the top edge.
#[must_use]
pub fn toolbar_band_placement_setting(store: &dyn BigPreferenceStore) -> BigBandPlacement {
    store
        .get_string(KEY_TOOLBAR_BAND_PLACEMENT)
        .map_or(BigBandPlacement::Top, |value| {
            toolbar_band_placement_from_setting(&value)
        })
}

/// The suite-wide toolbar-band preference row, in the same dropdown family as
/// [`TAB_PREFERENCE_ROWS`](crate::layout::tab_prefs::TAB_PREFERENCE_ROWS).
pub const TOOLBAR_BAND_PREFERENCE_ROWS: [BigPrefRow; 1] = [BigPrefRow::keyed_dropdown(
    "tab-alignment.svg",
    gettext_noop("Toolbar Position"),
    gettext_noop(
        "Where the command toolbar lives: above or below the content, as a vertical strip on either side of the window, or hidden.",
    ),
    KEY_TOOLBAR_BAND_PLACEMENT,
    &TOOLBAR_BAND_PLACEMENT_KEYS,
    TOOLBAR_BAND_PLACEMENT_DEFAULT,
    &TOOLBAR_BAND_PLACEMENT_LABELS,
)];

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
    fn placement_parses_every_persisted_key() {
        let placements: Vec<BigBandPlacement> = TOOLBAR_BAND_PLACEMENT_KEYS
            .iter()
            .map(|key| toolbar_band_placement_from_setting(key))
            .collect();
        assert_eq!(
            placements,
            [
                BigBandPlacement::Top,
                BigBandPlacement::Bottom,
                BigBandPlacement::Start,
                BigBandPlacement::End,
                BigBandPlacement::Hidden,
            ]
        );
    }

    #[test]
    fn unknown_placement_falls_back_to_top() {
        assert_eq!(
            toolbar_band_placement_from_setting("nonsense"),
            BigBandPlacement::Top
        );
    }

    #[test]
    fn placement_setting_defaults_to_top() {
        assert_eq!(
            toolbar_band_placement_setting(&EmptyStore),
            BigBandPlacement::Top
        );
    }

    #[test]
    fn preference_row_binds_the_toolbar_band_key() {
        let key = match TOOLBAR_BAND_PREFERENCE_ROWS[0].control {
            BigPrefControl::KeyedDropdown { key, .. } => key,
            _ => panic!("toolbar band row must be a keyed dropdown"),
        };
        assert_eq!(key, KEY_TOOLBAR_BAND_PLACEMENT);
    }
}
