// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Declarative preference rows: whole settings pages as const data.
//!
//! An app describes each setting as a [`BigPrefRow`] const (didactic SVG,
//! msgid title/description, persisted control) and builds pages with
//! [`pref_group`]/[`pref_groups`]. Persistence, i18n, and illustration
//! resources come from the app through [`BigPrefHost`], so row tables stay
//! pure data.
//!
//! Consumer budget (call-site density contract): one row = one const
//! constructor call; a data-only page = one `pref_groups(&host, ROWS)` line.

use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;

use crate::a11y::label_pref_group_wrapper;
use crate::input::personalization_rows::{BigFontPickerRow, BigOptionalColorRow};
use crate::input::preference_cards::{
    BigDropdownPreferenceCard, BigScalePreferenceCard, BigSpinPreferenceCard, BigSpinRange,
    BigSwitchPreferenceCard,
};
use crate::layout::didactic_card::BigDidacticCardSpec;
use crate::layout::preferences::BigPreferencesGroupSpec;

/// Typed settings access the host app provides for preference rows.
///
/// Keys are app-owned. Implementations persist on every `set_*` call and
/// return `None` from `get_*` when a key is unset or has the wrong type.
pub trait BigPreferenceStore {
    /// Read a boolean setting; `None` when unset or mistyped.
    fn get_bool(&self, key: &str) -> Option<bool>;
    /// Read an integer setting; `None` when unset or mistyped.
    fn get_i64(&self, key: &str) -> Option<i64>;
    /// Read a float setting; `None` when unset or mistyped.
    fn get_f64(&self, key: &str) -> Option<f64>;
    /// Read a string setting; `None` when unset or mistyped.
    fn get_string(&self, key: &str) -> Option<String>;
    /// Persist a boolean setting.
    fn set_bool(&self, key: &'static str, value: bool);
    /// Persist an integer setting.
    fn set_i64(&self, key: &'static str, value: i64);
    /// Persist a float setting.
    fn set_f64(&self, key: &'static str, value: f64);
    /// Persist a string setting. Keys are compile-time consts (`&'static`);
    /// values are dynamic, so `value` borrows for the call only.
    fn set_string(&self, key: &'static str, value: &str);
}

/// The suite settings store satisfies the row-persistence contract
/// directly: an app passes its [`big_app_kit::settings_store::BigSettingsStore`]
/// as [`BigPrefHost::store`] with no adapter layer.
impl BigPreferenceStore for big_app_kit::settings_store::BigSettingsStore {
    fn get_bool(&self, key: &str) -> Option<bool> {
        self.value(key)
            .as_ref()
            .and_then(serde_json::Value::as_bool)
    }
    fn get_i64(&self, key: &str) -> Option<i64> {
        self.value(key).as_ref().and_then(serde_json::Value::as_i64)
    }
    fn get_f64(&self, key: &str) -> Option<f64> {
        self.value(key).as_ref().and_then(serde_json::Value::as_f64)
    }
    fn get_string(&self, key: &str) -> Option<String> {
        self.value(key)
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    }
    fn set_bool(&self, key: &'static str, value: bool) {
        self.set(key, serde_json::Value::from(value));
    }
    fn set_i64(&self, key: &'static str, value: i64) {
        self.set(key, serde_json::Value::from(value));
    }
    fn set_f64(&self, key: &'static str, value: f64) {
        self.set(key, serde_json::Value::from(value));
    }
    fn set_string(&self, key: &'static str, value: &str) {
        self.set(key, serde_json::Value::from(value));
    }
}

/// App context for building preference rows: persistence, i18n, resources.
#[derive(Clone)]
pub struct BigPrefHost {
    /// Settings persistence.
    pub store: Rc<dyn BigPreferenceStore>,
    /// gettext-style msgid translator.
    pub translate: Rc<dyn Fn(&str) -> String>,
    /// GResource directory that holds the didactic-card SVGs.
    pub resource_prefix: &'static str,
}

impl BigPrefHost {
    /// Bind a preference host to a settings store with identity translation.
    ///
    /// The store satisfies [`BigPreferenceStore`] directly, so no adapter layer
    /// is needed; `resource_prefix` points at the GResource directory holding
    /// the didactic-card SVGs (for example
    /// [`crate::resources::ILLUSTRATION_RESOURCE_PREFIX`]). Apps that need a real
    /// translator can build [`BigPrefHost`] by hand instead.
    #[must_use]
    pub fn for_store(
        store: &big_app_kit::settings_store::BigSettingsStore,
        resource_prefix: &'static str,
    ) -> Self {
        Self {
            store: Rc::new(store.clone()),
            translate: Rc::new(str::to_owned),
            resource_prefix,
        }
    }

    fn spec(&self, svg: &'static str, title: &str, description: &str) -> BigDidacticCardSpec {
        BigDidacticCardSpec::new(
            self.resource_prefix,
            svg,
            (self.translate)(title),
            (self.translate)(description),
        )
    }
}

/// Persisted control of one declarative preference row.
#[derive(Clone, Copy)]
pub enum BigPrefControl {
    /// Boolean switch persisted under `key`.
    Switch {
        /// Settings key.
        key: &'static str,
        /// Value when the key is unset.
        default: bool,
    },
    /// Integer spin button persisted under `key`.
    IntSpin {
        /// Settings key.
        key: &'static str,
        /// Value when the key is unset.
        default: i64,
        /// Spin bounds and increments.
        range: BigSpinRange,
    },
    /// Float spin button persisted under `key`.
    FloatSpin {
        /// Settings key.
        key: &'static str,
        /// Value when the key is unset.
        default: f64,
        /// Spin bounds and increments.
        range: BigSpinRange,
        /// Displayed decimal digits.
        digits: u32,
    },
    /// Integer slider persisted under `key`.
    Scale {
        /// Settings key.
        key: &'static str,
        /// Value when the key is unset.
        default: i64,
        /// Slider bounds and increments.
        range: BigSpinRange,
    },
    /// Dropdown persisting the selected index under `key`.
    IndexDropdown {
        /// Settings key.
        key: &'static str,
        /// Option label msgids, in display order.
        options: &'static [&'static str],
        /// Index when the key is unset.
        default: u32,
    },
    /// Dropdown persisting a stable string key under `key`.
    KeyedDropdown {
        /// Settings key.
        key: &'static str,
        /// Persisted value per option, parallel to `labels`.
        keys: &'static [&'static str],
        /// Persisted value when the key is unset.
        default_key: &'static str,
        /// Option label msgids, parallel to `keys`.
        labels: &'static [&'static str],
    },
    /// Optional-color swatch + reset, persisted as a hex string under `key`.
    /// Empty/absent means "theme default"; a picked color is `#rrggbb`.
    Color {
        /// Settings key.
        key: &'static str,
    },
    /// System font-family picker + reset, persisted as a family string under
    /// `key`. Empty/absent means "theme default".
    Font {
        /// Settings key.
        key: &'static str,
    },
}

/// One preference row as const data: didactic card + persisted control.
#[derive(Clone, Copy)]
pub struct BigPrefRow {
    /// Illustration file name under the host's resource prefix.
    pub svg: &'static str,
    /// Row title msgid.
    pub title: &'static str,
    /// Row description msgid.
    pub description: &'static str,
    /// Persisted control shown on the card.
    pub control: BigPrefControl,
}

impl BigPrefRow {
    /// Boolean switch row.
    pub const fn switch(
        svg: &'static str,
        title: &'static str,
        description: &'static str,
        key: &'static str,
        default: bool,
    ) -> Self {
        Self {
            svg,
            title,
            description,
            control: BigPrefControl::Switch { key, default },
        }
    }

    /// Integer spin row.
    pub const fn int_spin(
        svg: &'static str,
        title: &'static str,
        description: &'static str,
        key: &'static str,
        default: i64,
        range: BigSpinRange,
    ) -> Self {
        Self {
            svg,
            title,
            description,
            control: BigPrefControl::IntSpin {
                key,
                default,
                range,
            },
        }
    }

    /// Float spin row.
    #[allow(clippy::too_many_arguments)]
    pub const fn float_spin(
        svg: &'static str,
        title: &'static str,
        description: &'static str,
        key: &'static str,
        default: f64,
        range: BigSpinRange,
        digits: u32,
    ) -> Self {
        Self {
            svg,
            title,
            description,
            control: BigPrefControl::FloatSpin {
                key,
                default,
                range,
                digits,
            },
        }
    }

    /// Integer slider row.
    pub const fn scale(
        svg: &'static str,
        title: &'static str,
        description: &'static str,
        key: &'static str,
        default: i64,
        range: BigSpinRange,
    ) -> Self {
        Self {
            svg,
            title,
            description,
            control: BigPrefControl::Scale {
                key,
                default,
                range,
            },
        }
    }

    /// Index-persisting dropdown row.
    pub const fn index_dropdown(
        svg: &'static str,
        title: &'static str,
        description: &'static str,
        key: &'static str,
        options: &'static [&'static str],
        default: u32,
    ) -> Self {
        Self {
            svg,
            title,
            description,
            control: BigPrefControl::IndexDropdown {
                key,
                options,
                default,
            },
        }
    }

    /// Key-persisting dropdown row.
    #[allow(clippy::too_many_arguments)]
    pub const fn keyed_dropdown(
        svg: &'static str,
        title: &'static str,
        description: &'static str,
        key: &'static str,
        keys: &'static [&'static str],
        default_key: &'static str,
        labels: &'static [&'static str],
    ) -> Self {
        Self {
            svg,
            title,
            description,
            control: BigPrefControl::KeyedDropdown {
                key,
                keys,
                default_key,
                labels,
            },
        }
    }

    /// Optional-color row: swatch + reset, persisted as a hex string.
    /// Empty/absent means "theme default".
    pub const fn color(
        svg: &'static str,
        title: &'static str,
        description: &'static str,
        key: &'static str,
    ) -> Self {
        Self {
            svg,
            title,
            description,
            control: BigPrefControl::Color { key },
        }
    }

    /// Font-picker row: system font-family picker + reset, persisted as a
    /// family string. Empty/absent means "theme default".
    pub const fn font(
        svg: &'static str,
        title: &'static str,
        description: &'static str,
        key: &'static str,
    ) -> Self {
        Self {
            svg,
            title,
            description,
            control: BigPrefControl::Font { key },
        }
    }
}

/// Build one preference group from a declarative row.
pub fn pref_group(host: &BigPrefHost, row: &BigPrefRow) -> adw::PreferencesGroup {
    card_group(&pref_card(host, row))
}

/// Build a whole preference page from declarative rows in one call.
///
/// Collapses the host + page + row loop an app used to repeat into a single
/// call: `build_pref_page(&host, title, &ROWS)`. Each row becomes one
/// untitled group in declaration order.
#[must_use]
pub fn build_pref_page(
    host: &BigPrefHost,
    title: &str,
    rows: &[BigPrefRow],
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder().title(title).build();
    for row in rows {
        page.add(&pref_group(host, row));
    }
    page
}

/// Build every row of a declarative preference page, in order.
pub fn pref_groups(host: &BigPrefHost, rows: &[BigPrefRow]) -> Vec<gtk::Widget> {
    rows.iter()
        .map(|row| pref_group(host, row).upcast())
        .collect()
}

/// Build the didactic card (without group wrapper) for one row.
pub fn pref_card(host: &BigPrefHost, row: &BigPrefRow) -> gtk::Box {
    let BigPrefRow {
        svg,
        title,
        description,
        control,
    } = *row;
    match control {
        BigPrefControl::Switch { key, default } => {
            switch_card(host, key, default, svg, title, description)
        }
        BigPrefControl::IntSpin {
            key,
            default,
            range,
        } => int_spin_card(host, key, default, range, svg, title, description),
        BigPrefControl::FloatSpin {
            key,
            default,
            range,
            digits,
        } => float_spin_card(host, key, default, range, digits, svg, title, description),
        BigPrefControl::Scale {
            key,
            default,
            range,
        } => scale_card(host, key, default, range, svg, title, description),
        BigPrefControl::IndexDropdown {
            key,
            options,
            default,
        } => index_dropdown_card(host, key, options, default, svg, title, description),
        BigPrefControl::KeyedDropdown {
            key,
            keys,
            default_key,
            labels,
        } => keyed_dropdown_card(
            host,
            key,
            keys,
            default_key,
            labels,
            svg,
            title,
            description,
        ),
        BigPrefControl::Color { key } => color_card(host, key, svg, title, description),
        BigPrefControl::Font { key } => font_card(host, key, svg, title, description),
    }
}

/// Wrap one card in an untitled `AdwPreferencesGroup` with an accessible
/// label mirrored from the card's first visible label.
pub fn card_group(card: &gtk::Box) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::untitled().build();
    group.add(card);
    if let Some(label) = first_label_text(card.upcast_ref()) {
        label_pref_group_wrapper(card, &label);
        let card_clone = card.clone();
        gtk::glib::idle_add_local_once(move || {
            label_pref_group_wrapper(&card_clone, &label);
        });
    }
    group
}

/// Didactic switch card persisted through the host store.
pub fn switch_card(
    host: &BigPrefHost,
    key: &'static str,
    default: bool,
    svg: &'static str,
    title: &str,
    description: &str,
) -> gtk::Box {
    let active = host.store.get_bool(key).unwrap_or(default);
    let card = BigSwitchPreferenceCard::new(host.spec(svg, title, description), active);
    let store = host.store.clone();
    card.switch().connect_active_notify(move |switch| {
        store.set_bool(key, switch.is_active());
    });
    card.into_parts().0
}

/// Didactic integer spin card persisted through the host store.
#[allow(clippy::too_many_arguments)]
pub fn int_spin_card(
    host: &BigPrefHost,
    key: &'static str,
    default: i64,
    range: BigSpinRange,
    svg: &'static str,
    title: &str,
    description: &str,
) -> gtk::Box {
    let initial = host.store.get_i64(key).unwrap_or(default);
    #[allow(clippy::cast_precision_loss)]
    let value = initial as f64;
    let card = BigSpinPreferenceCard::new(host.spec(svg, title, description), value, range, 0);
    let store = host.store.clone();
    card.spin().connect_value_changed(move |spin| {
        #[allow(clippy::cast_possible_truncation)]
        store.set_i64(key, spin.value() as i64);
    });
    card.into_parts().0
}

/// Didactic float spin card persisted through the host store.
#[allow(clippy::too_many_arguments)]
pub fn float_spin_card(
    host: &BigPrefHost,
    key: &'static str,
    default: f64,
    range: BigSpinRange,
    digits: u32,
    svg: &'static str,
    title: &str,
    description: &str,
) -> gtk::Box {
    let value = host.store.get_f64(key).unwrap_or(default);
    let card = BigSpinPreferenceCard::new(host.spec(svg, title, description), value, range, digits);
    let store = host.store.clone();
    card.spin().connect_value_changed(move |spin| {
        store.set_f64(key, spin.value());
    });
    card.into_parts().0
}

/// Didactic slider card persisted through the host store.
#[allow(clippy::too_many_arguments)]
pub fn scale_card(
    host: &BigPrefHost,
    key: &'static str,
    default: i64,
    range: BigSpinRange,
    svg: &'static str,
    title: &str,
    description: &str,
) -> gtk::Box {
    let initial = host.store.get_i64(key).unwrap_or(default);
    #[allow(clippy::cast_precision_loss)]
    let value = initial as f64;
    let card = BigScalePreferenceCard::new(host.spec(svg, title, description), value, range, 0);
    let store = host.store.clone();
    card.scale().connect_value_changed(move |scale| {
        #[allow(clippy::cast_possible_truncation)]
        store.set_i64(key, scale.value().round() as i64);
    });
    card.into_parts().0
}

/// Didactic dropdown card persisting the selected index.
#[allow(clippy::too_many_arguments)]
pub fn index_dropdown_card(
    host: &BigPrefHost,
    key: &'static str,
    options: &[&str],
    default: u32,
    svg: &'static str,
    title: &str,
    description: &str,
) -> gtk::Box {
    let labels: Vec<String> = options.iter().map(|s| (host.translate)(s)).collect();
    let max_index = options.len().saturating_sub(1) as i64;
    let selected = host
        .store
        .get_i64(key)
        .unwrap_or(i64::from(default))
        .clamp(0, max_index);
    let (root, dropdown) = dropdown_card_parts(
        host,
        svg,
        title,
        description,
        &labels,
        u32::try_from(selected).unwrap_or(default),
    );
    let store = host.store.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        store.set_i64(key, i64::from(dropdown.selected()));
    });
    root
}

/// Didactic dropdown card persisting a stable string key.
#[allow(clippy::too_many_arguments)]
pub fn keyed_dropdown_card(
    host: &BigPrefHost,
    setting_key: &'static str,
    keys: &'static [&'static str],
    default_key: &'static str,
    option_labels: &[&str],
    svg: &'static str,
    title: &str,
    description: &str,
) -> gtk::Box {
    debug_assert_eq!(keys.len(), option_labels.len());
    let labels: Vec<String> = option_labels.iter().map(|s| (host.translate)(s)).collect();
    let current = host
        .store
        .get_string(setting_key)
        .unwrap_or_else(|| default_key.to_owned());
    let selected = keys
        .iter()
        .position(|k| *k == current.as_str())
        .unwrap_or(0);
    let (root, dropdown) = dropdown_card_parts(
        host,
        svg,
        title,
        description,
        &labels,
        u32::try_from(selected).unwrap_or(0),
    );
    let store = host.store.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        let idx = usize::try_from(dropdown.selected()).unwrap_or(0);
        let value = keys.get(idx).copied().unwrap_or(default_key);
        store.set_string(setting_key, value);
    });
    root
}

/// Didactic optional-color card persisted as a hex string. Empty/absent is
/// "theme default".
pub fn color_card(
    host: &BigPrefHost,
    key: &'static str,
    svg: &'static str,
    title: &str,
    description: &str,
) -> gtk::Box {
    let initial = host.store.get_string(key).unwrap_or_default();
    let store = host.store.clone();
    BigOptionalColorRow::new(
        host.spec(svg, title, description),
        &initial,
        &(host.translate)("Choose a color"),
        &(host.translate)("Use the theme color"),
        move |hex| {
            store.set_string(key, &hex.unwrap_or_default());
        },
    )
    .into_root()
}

/// Didactic font-picker card persisted as a family string. Empty/absent is
/// "theme default".
pub fn font_card(
    host: &BigPrefHost,
    key: &'static str,
    svg: &'static str,
    title: &str,
    description: &str,
) -> gtk::Box {
    let initial = host.store.get_string(key).unwrap_or_default();
    let store = host.store.clone();
    BigFontPickerRow::new(
        host.spec(svg, title, description),
        &initial,
        &(host.translate)("Use the theme font"),
        move |family| {
            store.set_string(key, &family.unwrap_or_default());
        },
    )
    .into_root()
}

/// Didactic dropdown card without persistence wiring; returns root + control.
pub fn dropdown_card_parts(
    host: &BigPrefHost,
    svg: &'static str,
    title: &str,
    description: &str,
    labels: &[String],
    selected: u32,
) -> (gtk::Box, gtk::DropDown) {
    BigDropdownPreferenceCard::new(host.spec(svg, title, description), labels, selected)
        .into_parts()
}

/// Didactic switch card without persistence wiring; returns root + control.
pub fn switch_card_parts(
    host: &BigPrefHost,
    svg: &'static str,
    title: &str,
    description: &str,
    active: bool,
) -> (gtk::Box, gtk::Switch) {
    BigSwitchPreferenceCard::new(host.spec(svg, title, description), active).into_parts()
}

fn first_label_text(root: &gtk::Widget) -> Option<String> {
    if let Some(label) = root.downcast_ref::<gtk::Label>() {
        let text = label.text();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(text) = first_label_text(&widget) {
            return Some(text);
        }
        child = widget.next_sibling();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn const_rows_clamp_nothing_at_build_time() {
        const ROW: BigPrefRow = BigPrefRow::switch("a.svg", "Title", "Description", "key", true);
        match ROW.control {
            BigPrefControl::Switch { key, default } => {
                assert_eq!(key, "key");
                assert!(default);
            }
            _ => panic!("expected switch control"),
        }
    }
}
