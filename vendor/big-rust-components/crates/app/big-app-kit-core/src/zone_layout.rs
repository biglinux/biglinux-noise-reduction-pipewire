// SPDX-License-Identifier: MIT

//! Display-free zone-layout model (RESTRUCTURE phase **D3** / decision **D8**).
//!
//! This is the data behind `big_relm4_components::input::zone_layout_editor::BigZoneLayoutEditor`
//! (the GTK visual "drag controls between zones" editor) and
//! `big_relm4_components::layout::zone_applier` (the runtime placement applier)
//! — the customization the video-player controls bar already ships and the
//! big-shell panel should adopt.
//!
//! Two halves:
//! - [`crate::zone_layout::ZoneLayoutSpec`] (the catalog + topology): which
//!   controls exist ([`crate::zone_layout::ZoneControl`]) and how the bars/zones
//!   are arranged ([`crate::zone_layout::BarSpec`]). Built fresh by the app each
//!   time the editor opens.
//! - [`crate::zone_layout::ZoneLayout`] (the value): the per-zone ordered
//!   control ids plus the set of hidden controls.
//!
//! Kept serde-free (string ids are "serde-friendly" but the framework leaves
//! persistence to the app): the video player maps a
//! [`crate::zone_layout::ZoneLayout`] to its
//! `ui-zone-*` / `ui-show-*` gsettings keys, big-shell to its panel config
//! struct.

use std::collections::{BTreeMap, BTreeSet};

/// A slot within a bar. The video-player bottom bar uses all three; its top bar
/// uses only `Start`/`End`; a single-row panel uses `Start`/`Center`/`End`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZonePlacement {
    /// Leading edge of the bar (left in LTR).
    Start,
    /// Center of the bar.
    Center,
    /// Trailing edge of the bar (right in LTR).
    End,
}

impl ZonePlacement {
    /// Every placement in render order — useful for a full single-row bar.
    pub const ALL: [ZonePlacement; 3] = [Self::Start, Self::Center, Self::End];

    /// The stable lowercase token used in [`slot_key`].
    #[must_use]
    pub fn as_key_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
        }
    }
}

/// Compose the [`ZoneLayout::slots`] key for a `(bar, placement)` pair:
/// `"<bar-id>:<placement>"` (e.g. `"bottom:center"`). The same format is used by
/// [`ZoneControl::default_slot`].
#[must_use]
pub fn slot_key(bar_id: &str, placement: ZonePlacement) -> String {
    format!("{bar_id}:{}", placement.as_key_str())
}

/// One selectable control in the editor. `label` is ALREADY translated by the
/// caller — the catalog never embeds msgids. `icon` is a symbolic icon name.
/// `default_slot` is the [`slot_key`] a control falls back to when a saved or
/// hand-edited layout omits it (see [`ZoneLayout::normalized`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneControl {
    /// Stable control id (matches the app's own control registry).
    pub id: String,
    /// Already-translated display label (tooltip + accessible name).
    pub label: String,
    /// Symbolic icon name shown in the miniature.
    pub icon: String,
    /// `"<bar>:<placement>"` slot this control lands in when otherwise unplaced.
    pub default_slot: String,
}

impl ZoneControl {
    /// Build a control. `default_slot` should be a [`slot_key`] valid in the
    /// spec the control belongs to.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        icon: impl Into<String>,
        default_slot: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: icon.into(),
            default_slot: default_slot.into(),
        }
    }
}

/// Miniature-only bar decoration (preview realism): a dim title placeholder and
/// a non-interactive seek slider. Has no effect on the persisted [`ZoneLayout`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BarDecoration {
    /// Optional dim title placeholder (e.g. "Video Title").
    pub title: Option<String>,
    /// Whether to draw a disabled seek slider above this bar.
    pub seek_slider: bool,
}

/// One bar (a `CenterBox` row in the rendered editor) and which of its zones
/// accept controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarSpec {
    /// Stable bar id (used in [`slot_key`]).
    pub id: String,
    /// Which placements this bar exposes, in render order.
    pub zones: Vec<ZonePlacement>,
    /// Miniature-only decoration.
    pub decoration: BarDecoration,
}

impl BarSpec {
    /// Build a bar exposing `zones`, with no decoration.
    #[must_use]
    pub fn new(id: impl Into<String>, zones: Vec<ZonePlacement>) -> Self {
        Self {
            id: id.into(),
            zones,
            decoration: BarDecoration::default(),
        }
    }

    /// Set the miniature decoration (title placeholder / seek slider).
    #[must_use]
    pub fn with_decoration(mut self, decoration: BarDecoration) -> Self {
        self.decoration = decoration;
        self
    }
}

/// The editor input: the control catalog plus the bar/zone topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneLayoutSpec {
    /// Every selectable control.
    pub controls: Vec<ZoneControl>,
    /// The bars, in top-to-bottom render order.
    pub bars: Vec<BarSpec>,
}

impl ZoneLayoutSpec {
    /// Build a spec from a control catalog and a bar topology.
    #[must_use]
    pub fn new(controls: Vec<ZoneControl>, bars: Vec<BarSpec>) -> Self {
        Self { controls, bars }
    }

    /// Every valid `(bar, placement)` slot key, in render order.
    #[must_use]
    pub fn slot_keys(&self) -> Vec<String> {
        self.bars
            .iter()
            .flat_map(|bar| bar.zones.iter().map(move |&p| slot_key(&bar.id, p)))
            .collect()
    }

    /// Look up a control by id.
    #[must_use]
    pub fn control(&self, id: &str) -> Option<&ZoneControl> {
        self.controls.iter().find(|c| c.id == id)
    }
}

/// The edited value: per-zone ordered control ids plus the hidden set.
///
/// `slots` maps a [`slot_key`] to the ordered control ids in that zone; `hidden`
/// is the set of control ids toggled off (a control is visible by default).
/// Run [`ZoneLayout::normalized`] on load AND after every edit so an old or
/// hand-edited value self-heals (dedup, drop-unknown, append-missing-defaults).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZoneLayout {
    /// `"<bar>:<placement>"` → ordered control ids.
    pub slots: BTreeMap<String, Vec<String>>,
    /// Control ids hidden by the user (visible when absent).
    pub hidden: BTreeSet<String>,
}

impl ZoneLayout {
    /// The ordered control ids in `(bar, placement)`, or `&[]` if the slot is
    /// empty/absent.
    #[must_use]
    pub fn zone(&self, bar_id: &str, placement: ZonePlacement) -> &[String] {
        self.slots
            .get(&slot_key(bar_id, placement))
            .map_or(&[], Vec::as_slice)
    }

    /// The `(bar, placement)` zone as a comma-separated string (the form
    /// gsettings-backed apps like the video player store).
    #[must_use]
    pub fn zone_csv(&self, bar_id: &str, placement: ZonePlacement) -> String {
        self.zone(bar_id, placement).join(",")
    }

    /// Whether `control_id` is visible (not in [`ZoneLayout::hidden`]).
    #[must_use]
    pub fn is_visible(&self, control_id: &str) -> bool {
        !self.hidden.contains(control_id)
    }

    /// Show or hide `control_id`.
    pub fn set_visible(&mut self, control_id: &str, visible: bool) {
        if visible {
            self.hidden.remove(control_id);
        } else {
            self.hidden.insert(control_id.to_owned());
        }
    }

    /// Build a layout from per-zone CSV strings (placement only — visibility is
    /// set separately via [`ZoneLayout::set_visible`], mirroring the video
    /// player's split `ui-zone-*` + `ui-show-*` keys). `get_csv` is called once
    /// per `(bar, placement)` slot. The result is [`normalized`](Self::normalized).
    #[must_use]
    pub fn from_csv_zones(
        spec: &ZoneLayoutSpec,
        get_csv: impl Fn(&str, ZonePlacement) -> String,
    ) -> Self {
        let mut layout = Self::default();
        for bar in &spec.bars {
            for &placement in &bar.zones {
                let ids = get_csv(&bar.id, placement)
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                layout.slots.insert(slot_key(&bar.id, placement), ids);
            }
        }
        layout.normalized(spec)
    }

    /// Return a self-healed copy against `spec`: every valid slot is present
    /// (possibly empty); each control appears in AT MOST ONE zone (first
    /// occurrence in render order wins); ids not in `spec.controls` and slots
    /// not in `spec` are dropped; any spec control left unplaced is appended to
    /// its [`ZoneControl::default_slot`]; `hidden` keeps only valid ids.
    #[must_use]
    pub fn normalized(&self, spec: &ZoneLayoutSpec) -> Self {
        let valid_ids: BTreeSet<&str> = spec.controls.iter().map(|c| c.id.as_str()).collect();
        let mut result = Self::default();
        let mut placed: BTreeSet<String> = BTreeSet::new();

        // Walk bars/zones in render order, pulling saved ids per slot.
        for bar in &spec.bars {
            for &placement in &bar.zones {
                let key = slot_key(&bar.id, placement);
                let mut ids = Vec::new();
                if let Some(saved) = self.slots.get(&key) {
                    for id in saved {
                        if valid_ids.contains(id.as_str()) && placed.insert(id.clone()) {
                            ids.push(id.clone());
                        }
                    }
                }
                result.slots.insert(key, ids);
            }
        }

        // Append any control the saved layout omitted to its default slot. A
        // control whose default_slot isn't a valid slot in this spec is
        // unreachable — left unplaced (the spec author's bug, not a panic).
        for control in &spec.controls {
            if !placed.contains(&control.id)
                && let Some(slot) = result.slots.get_mut(&control.default_slot)
            {
                slot.push(control.id.clone());
                placed.insert(control.id.clone());
            }
        }

        result.hidden = self
            .hidden
            .iter()
            .filter(|h| valid_ids.contains(h.as_str()))
            .cloned()
            .collect();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ZoneLayoutSpec {
        ZoneLayoutSpec::new(
            vec![
                ZoneControl::new("play", "Play", "media-playback-start", "bar:center"),
                ZoneControl::new("volume", "Volume", "audio-volume-high", "bar:start"),
                ZoneControl::new("fullscreen", "Fullscreen", "view-fullscreen", "bar:end"),
            ],
            vec![BarSpec::new("bar", ZonePlacement::ALL.to_vec())],
        )
    }

    #[test]
    fn slot_key_format() {
        assert_eq!(slot_key("bottom", ZonePlacement::Center), "bottom:center");
    }

    #[test]
    fn slot_keys_lists_every_bar_zone_in_render_order() {
        let spec = spec();
        assert_eq!(spec.slot_keys(), ["bar:start", "bar:center", "bar:end"]);
    }

    #[test]
    fn control_lookup_matches_by_id_or_none() {
        let spec = spec();
        assert_eq!(
            spec.control("volume").map(|c| c.id.as_str()),
            Some("volume")
        );
        assert_eq!(spec.control("play").map(|c| c.id.as_str()), Some("play"));
        assert!(spec.control("missing").is_none());
    }

    #[test]
    fn from_csv_drops_empty_segments_and_keeps_order() {
        let spec = spec();
        // Non-default slot with an embedded empty segment: the two real ids must
        // survive (empties dropped) IN ORDER, proving the values flow through
        // `from_csv` rather than the default-slot append fallback.
        let layout = ZoneLayout::from_csv_zones(&spec, |bar, placement| match (bar, placement) {
            ("bar", ZonePlacement::Center) => "volume,,play".to_string(),
            _ => String::new(),
        });
        assert_eq!(
            layout.zone("bar", ZonePlacement::Center),
            ["volume", "play"]
        );
        assert!(layout.zone("bar", ZonePlacement::Start).is_empty());
    }

    #[test]
    fn from_csv_round_trips_to_zone_csv() {
        let spec = spec();
        let layout = ZoneLayout::from_csv_zones(&spec, |bar, placement| match (bar, placement) {
            ("bar", ZonePlacement::Start) => "volume".to_string(),
            ("bar", ZonePlacement::Center) => "play".to_string(),
            ("bar", ZonePlacement::End) => "fullscreen".to_string(),
            _ => String::new(),
        });
        assert_eq!(layout.zone_csv("bar", ZonePlacement::Start), "volume");
        assert_eq!(layout.zone_csv("bar", ZonePlacement::Center), "play");
        assert_eq!(layout.zone("bar", ZonePlacement::End), ["fullscreen"]);
    }

    #[test]
    fn normalized_dedups_a_control_to_first_zone() {
        let spec = spec();
        let mut layout = ZoneLayout::default();
        // "play" appears in two zones — only the first (render order) survives.
        layout
            .slots
            .insert("bar:start".into(), vec!["play".into(), "volume".into()]);
        layout
            .slots
            .insert("bar:center".into(), vec!["play".into()]);
        let n = layout.normalized(&spec);
        assert_eq!(n.zone("bar", ZonePlacement::Start), ["play", "volume"]);
        assert!(n.zone("bar", ZonePlacement::Center).is_empty());
    }

    #[test]
    fn normalized_drops_unknown_ids_and_slots() {
        let spec = spec();
        let mut layout = ZoneLayout::default();
        layout
            .slots
            .insert("bar:center".into(), vec!["play".into(), "ghost".into()]);
        layout
            .slots
            .insert("nope:center".into(), vec!["play".into()]);
        let n = layout.normalized(&spec);
        assert_eq!(n.zone("bar", ZonePlacement::Center), ["play"]);
        assert!(!n.slots.contains_key("nope:center"));
    }

    #[test]
    fn normalized_appends_missing_to_default_slot() {
        let spec = spec();
        let mut layout = ZoneLayout::default();
        layout
            .slots
            .insert("bar:center".into(), vec!["play".into()]);
        let n = layout.normalized(&spec);
        assert_eq!(n.zone("bar", ZonePlacement::Start), ["volume"]);
        assert_eq!(n.zone("bar", ZonePlacement::Center), ["play"]);
        assert_eq!(n.zone("bar", ZonePlacement::End), ["fullscreen"]);
    }

    #[test]
    fn visibility_set_and_get() {
        let mut layout = ZoneLayout::default();
        assert!(layout.is_visible("play"));
        layout.set_visible("play", false);
        assert!(!layout.is_visible("play"));
        layout.set_visible("play", true);
        assert!(layout.is_visible("play"));
    }

    #[test]
    fn normalized_keeps_only_valid_hidden() {
        let spec = spec();
        let mut layout = ZoneLayout::default();
        layout.set_visible("volume", false);
        layout.set_visible("ghost", false);
        let n = layout.normalized(&spec);
        assert!(n.hidden.contains("volume"));
        assert!(!n.hidden.contains("ghost"));
    }
}
