// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Suite-wide appearance personalization rows and settings readers.

use big_app_kit::window_shell::{BigWindowOpacityPercent, BigWindowTransparencyPolicy};

use crate::i18n::gettext_noop;
use crate::input::preference_cards::BigSpinRange;
use crate::input::preference_rows::{BigPrefRow, BigPreferenceStore};

/// Settings key: window body transparency, `0..=100`.
pub const KEY_TRANSPARENCY: &str = "transparency";
/// Settings key: header/chrome transparency, `0..=100`.
pub const KEY_HEADERBAR_TRANSPARENCY: &str = "headerbar_transparency";
/// Settings key: compositor background blur.
pub const KEY_BACKGROUND_BLUR: &str = "background_blur";

/// All suite appearance keys, used by settings listeners.
pub const APPEARANCE_SETTING_KEYS: [&str; 3] = [
    KEY_TRANSPARENCY,
    KEY_HEADERBAR_TRANSPARENCY,
    KEY_BACKGROUND_BLUR,
];

const PERCENT_RANGE: BigSpinRange = BigSpinRange::new(0.0, 100.0, 1.0, 10.0);

/// The suite-wide appearance preference rows, in display order.
pub const APPEARANCE_PREFERENCE_ROWS: [BigPrefRow; 3] = [
    BigPrefRow::scale(
        "window-transparency.svg",
        gettext_noop("Window Transparency"),
        gettext_noop("Make the main window body show more of the desktop behind it."),
        KEY_TRANSPARENCY,
        0,
        PERCENT_RANGE,
    ),
    BigPrefRow::scale(
        "headerbar-transparency.svg",
        gettext_noop("Header Bar Transparency"),
        gettext_noop("Make the header bar and window chrome show more of the desktop behind them."),
        KEY_HEADERBAR_TRANSPARENCY,
        0,
        PERCENT_RANGE,
    ),
    BigPrefRow::switch(
        "background-blur.svg",
        gettext_noop("Background Blur"),
        gettext_noop(
            "Blur the desktop behind transparent parts of the window when the compositor supports it.",
        ),
        KEY_BACKGROUND_BLUR,
        false,
    ),
];

/// Read the suite appearance settings with defaults and `0..=100` clamping.
#[must_use]
pub fn appearance_settings(store: &dyn BigPreferenceStore) -> (u32, u32, bool) {
    (
        clamped_percent_setting(store, KEY_TRANSPARENCY),
        clamped_percent_setting(store, KEY_HEADERBAR_TRANSPARENCY),
        store.get_bool(KEY_BACKGROUND_BLUR).unwrap_or(false),
    )
}

/// Convert suite appearance settings into the shared window-shell policy.
#[must_use]
pub fn appearance_transparency_policy(
    transparency: u32,
    headerbar_transparency: u32,
) -> BigWindowTransparencyPolicy {
    match (headerbar_transparency > 0, transparency > 0) {
        (false, false) => BigWindowTransparencyPolicy::Opaque,
        (false, true) => BigWindowTransparencyPolicy::TranslucentBody {
            body_opacity: body_opacity_percent(transparency),
        },
        (true, false) => BigWindowTransparencyPolicy::TranslucentChromeAndBody {
            chrome_opacity: chrome_opacity_percent(headerbar_transparency),
            body_opacity: BigWindowOpacityPercent::OPAQUE,
        },
        (true, true) => BigWindowTransparencyPolicy::TranslucentChromeAndBody {
            chrome_opacity: chrome_opacity_percent(headerbar_transparency),
            body_opacity: body_opacity_percent(transparency),
        },
    }
}

/// Map a body transparency slider value to shell body opacity.
#[must_use]
pub fn body_opacity_percent(transparency: u32) -> BigWindowOpacityPercent {
    opacity_percent_from_alpha(big_appearance::chrome::body_alpha(f64::from(
        transparency.min(100),
    )))
}

/// Map a header/chrome transparency slider value to shell chrome opacity.
#[must_use]
pub fn chrome_opacity_percent(headerbar_transparency: u32) -> BigWindowOpacityPercent {
    let opacity = big_appearance::chrome::chrome_opacity_percent(headerbar_transparency.min(100));
    let opacity = u8::try_from(opacity).unwrap_or(100);
    BigWindowOpacityPercent::new(opacity).unwrap_or(BigWindowOpacityPercent::OPAQUE)
}

fn clamped_percent_setting(store: &dyn BigPreferenceStore, key: &str) -> u32 {
    if let Some(value) = store.get_i64(key) {
        return u32::try_from(value.clamp(0, 100)).unwrap_or(0);
    }
    store.get_f64(key).map_or(0, clamped_f64_percent)
}

fn clamped_f64_percent(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        value.round().clamp(0.0, 100.0) as u32
    }
}

fn opacity_percent_from_alpha(alpha: f64) -> BigWindowOpacityPercent {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let percent = (alpha * 100.0).round().clamp(0.0, 100.0) as u8;
    BigWindowOpacityPercent::new(percent).unwrap_or(BigWindowOpacityPercent::OPAQUE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::preference_rows::BigPrefControl;

    struct AppearanceTestStore;

    impl BigPreferenceStore for AppearanceTestStore {
        fn get_bool(&self, key: &str) -> Option<bool> {
            (key == KEY_BACKGROUND_BLUR).then_some(true)
        }

        fn get_i64(&self, key: &str) -> Option<i64> {
            match key {
                KEY_TRANSPARENCY => Some(142),
                _ => None,
            }
        }

        fn get_f64(&self, key: &str) -> Option<f64> {
            match key {
                KEY_HEADERBAR_TRANSPARENCY => Some(-12.0),
                _ => None,
            }
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
    fn rows_cover_the_suite_appearance_keys_in_order() {
        let keys: Vec<&str> = APPEARANCE_PREFERENCE_ROWS
            .iter()
            .map(|row| match row.control {
                BigPrefControl::Scale { key, .. } | BigPrefControl::Switch { key, .. } => key,
                _ => panic!("unexpected control kind in appearance rows"),
            })
            .collect();

        assert_eq!(keys, APPEARANCE_SETTING_KEYS);
    }

    #[test]
    fn appearance_settings_clamp_percent_values() {
        assert_eq!(appearance_settings(&AppearanceTestStore), (100, 0, true));
    }

    #[test]
    fn opacity_mapping_keeps_default_appearance_opaque() {
        assert_eq!(
            appearance_transparency_policy(0, 0),
            BigWindowTransparencyPolicy::Opaque
        );
        assert_eq!(body_opacity_percent(0), BigWindowOpacityPercent::OPAQUE);
        assert_eq!(chrome_opacity_percent(0), BigWindowOpacityPercent::OPAQUE);
    }
}
