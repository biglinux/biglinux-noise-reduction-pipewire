// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Didactic preference cards with app-wired state.

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::layout::didactic_card::{BigDidacticCardSpec, didactic_card};

const DEFAULT_SPIN_WIDTH_CHARS: i32 = 7;
const DEFAULT_SCALE_WIDTH_REQUEST: i32 = 180;

/// Numeric range descriptor reused by spin and scale preference cards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BigSpinRange {
    /// Lowest legal value.
    pub min: f64,
    /// Highest legal value (inclusive).
    pub max: f64,
    /// Increment when pressing the spin arrows or arrow keys.
    pub step: f64,
    /// Increment when pressing PageUp/PageDown.
    pub page_step: f64,
}

impl BigSpinRange {
    /// Construct a spin range with explicit bounds and step increments.
    #[must_use]
    pub const fn new(min: f64, max: f64, step: f64, page_step: f64) -> Self {
        Self {
            min,
            max,
            step,
            page_step,
        }
    }
}

/// Didactic card carrying a boolean toggle on the right.
#[derive(Debug, Clone)]
pub struct BigSwitchPreferenceCard {
    root: gtk::Box,
    switch: gtk::Switch,
}

impl BigSwitchPreferenceCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDidacticCardSpec, active: bool) -> Self {
        let switch = gtk::Switch::builder()
            .active(active)
            .valign(gtk::Align::Center)
            .build();
        switch.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        let root = didactic_card(spec, &switch);
        Self { root, switch }
    }

    /// Return a reference to the `root` exposed by this [`BigSwitchPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `switch` exposed by this [`BigSwitchPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn switch(&self) -> &gtk::Switch {
        &self.switch
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, gtk::Switch) {
        (self.root, self.switch)
    }
}

/// Didactic card carrying a numeric spin button on the right.
#[derive(Debug, Clone)]
pub struct BigSpinPreferenceCard {
    root: gtk::Box,
    spin: gtk::SpinButton,
}

impl BigSpinPreferenceCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDidacticCardSpec, value: f64, range: BigSpinRange, digits: u32) -> Self {
        let adjustment = gtk::Adjustment::new(
            value,
            range.min,
            range.max,
            range.step,
            range.page_step,
            0.0,
        );
        let spin = gtk::SpinButton::builder()
            .adjustment(&adjustment)
            .digits(digits)
            .numeric(true)
            .valign(gtk::Align::Center)
            .width_chars(DEFAULT_SPIN_WIDTH_CHARS)
            .build();
        spin.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        let root = didactic_card(spec, &spin);
        Self { root, spin }
    }

    /// Return a reference to the `root` exposed by this [`BigSpinPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `spin` exposed by this [`BigSpinPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn spin(&self) -> &gtk::SpinButton {
        &self.spin
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, gtk::SpinButton) {
        (self.root, self.spin)
    }
}

/// Didactic card carrying a drop-down on the right.
#[derive(Debug, Clone)]
pub struct BigDropdownPreferenceCard {
    root: gtk::Box,
    dropdown: gtk::DropDown,
}

impl BigDropdownPreferenceCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDidacticCardSpec, labels: &[String], selected: u32) -> Self {
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let dropdown = gtk::DropDown::builder()
            .model(&gtk::StringList::new(&label_refs))
            .selected(selected.min(labels.len().saturating_sub(1) as u32))
            .valign(gtk::Align::Center)
            .build();
        dropdown.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        let root = didactic_card(spec, &dropdown);
        Self { root, dropdown }
    }

    /// Return a reference to the `root` exposed by this [`BigDropdownPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `dropdown` exposed by this [`BigDropdownPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn dropdown(&self) -> &gtk::DropDown {
        &self.dropdown
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, gtk::DropDown) {
        (self.root, self.dropdown)
    }
}

/// Didactic card carrying a font-family dropdown. The first entry is a
/// caller-supplied "theme default" option that maps to `None`; the rest are
/// the supplied family names. State-free: wire
/// `dropdown().connect_selected_notify` and resolve the choice with
/// [`BigFontFamilyPreferenceCard::family_at`].
#[derive(Debug, Clone)]
pub struct BigFontFamilyPreferenceCard {
    root: gtk::Box,
    dropdown: gtk::DropDown,
}

impl BigFontFamilyPreferenceCard {
    /// Build the card. `default_label` is the first ("use the theme font")
    /// entry; `families` are the selectable names; `current` preselects a
    /// family if it is in `families`, otherwise the default entry.
    #[must_use]
    pub fn new(
        spec: BigDidacticCardSpec,
        default_label: &str,
        families: &[&str],
        current: Option<&str>,
    ) -> Self {
        let mut labels: Vec<&str> = Vec::with_capacity(families.len() + 1);
        labels.push(default_label);
        labels.extend_from_slice(families);
        let selected = current
            .and_then(|family| families.iter().position(|known| *known == family))
            .map_or(0, |index| index as u32 + 1);
        let dropdown = gtk::DropDown::builder()
            .model(&gtk::StringList::new(&labels))
            .selected(selected)
            .valign(gtk::Align::Center)
            .build();
        dropdown.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        let root = didactic_card(spec, &dropdown);
        Self { root, dropdown }
    }

    /// Resolve a dropdown index to a family name: index 0 is the theme
    /// default (`None`); every other index maps into `families`.
    #[must_use]
    pub fn family_at(families: &[&str], index: u32) -> Option<String> {
        (index as usize)
            .checked_sub(1)
            .and_then(|i| families.get(i))
            .map(|family| (*family).to_owned())
    }

    /// Return a reference to the `root` exposed by this card.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `dropdown` exposed by this card.
    #[must_use]
    pub fn dropdown(&self) -> &gtk::DropDown {
        &self.dropdown
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, gtk::DropDown) {
        (self.root, self.dropdown)
    }
}

/// Didactic card carrying a horizontal slider on the right.
#[derive(Debug, Clone)]
pub struct BigScalePreferenceCard {
    root: gtk::Box,
    scale: gtk::Scale,
}

impl BigScalePreferenceCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDidacticCardSpec, value: f64, range: BigSpinRange, digits: i32) -> Self {
        let adjustment = gtk::Adjustment::new(
            value,
            range.min,
            range.max,
            range.step,
            range.page_step,
            0.0,
        );
        let scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Horizontal)
            .adjustment(&adjustment)
            .digits(digits)
            .draw_value(true)
            .width_request(DEFAULT_SCALE_WIDTH_REQUEST)
            .valign(gtk::Align::Center)
            .build();
        scale.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        let root = didactic_card(spec, &scale);
        Self { root, scale }
    }

    /// Return a reference to the `root` exposed by this [`BigScalePreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `scale` exposed by this [`BigScalePreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn scale(&self) -> &gtk::Scale {
        &self.scale
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, gtk::Scale) {
        (self.root, self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spin_range_records_bounds() {
        let range = BigSpinRange::new(0.0, 10.0, 1.0, 5.0);
        assert_eq!(range.min, 0.0);
        assert_eq!(range.max, 10.0);
        assert_eq!(range.step, 1.0);
        assert_eq!(range.page_step, 5.0);
    }

    #[test]
    fn font_family_index_zero_is_theme_default() {
        let families = ["Cantarell", "Noto Sans"];
        assert_eq!(BigFontFamilyPreferenceCard::family_at(&families, 0), None);
        assert_eq!(
            BigFontFamilyPreferenceCard::family_at(&families, 1),
            Some("Cantarell".to_owned())
        );
        assert_eq!(
            BigFontFamilyPreferenceCard::family_at(&families, 2),
            Some("Noto Sans".to_owned())
        );
        assert_eq!(BigFontFamilyPreferenceCard::family_at(&families, 99), None);
    }
}
