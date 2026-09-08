// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Unified declarative chrome-as-data band-placement model (framework
//! criterion 8).
//!
//! This generalizes the toolbar-only [`BigWorkspaceToolbarPolicy`] into a model
//! where every movable window-chrome band — the headerbar, the tab strip, the
//! toolbar, and the footer/status band — can independently sit in the
//! headerbar, on any edge, or be hidden, and where per-control placement inside
//! a band comes from the [`ZoneLayoutSpec`] catalog.
//!
//! This wave lands the display-free SPEC only: the types, validation, and the
//! [`From`] bridge from the legacy toolbar policy. There is no GTK band-host
//! adapter and no live band mounting yet — those land with the terminal toolbar
//! migration, when [`BigWorkspaceWindowShellSpec::toolbar_policy`] is folded
//! into a [`BigChromeBand::Toolbar`] [`BigChromeBandPolicy`] and removed.
//!
//! [`BigWorkspaceToolbarPolicy`]: super::BigWorkspaceToolbarPolicy
//! [`BigWorkspaceWindowShellSpec::toolbar_policy`]: super::BigWorkspaceWindowShellSpec

use std::collections::BTreeSet;

use crate::zone_layout::ZoneLayoutSpec;

use super::{BigWindowShellSpecError, BigWorkspaceToolbarPlacement, BigWorkspaceToolbarPolicy};

/// A movable window-chrome band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigChromeBand {
    /// The window headerbar/titlebar band.
    Headerbar,
    /// The workspace tab strip band.
    TabStrip,
    /// The tool strip band.
    Toolbar,
    /// The footer/status band.
    FooterStatus,
}

/// Where a band sits, or that it is hidden.
///
/// Superset of the toolbar edges ([`Top`](Self::Top)/[`Bottom`](Self::Bottom)/
/// [`Start`](Self::Start)/[`End`](Self::End)) plus an in-headerbar slot and
/// [`Hidden`](Self::Hidden).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BigBandPlacement {
    /// Mounted inside the window headerbar.
    InHeaderbar,
    /// Above the workspace body.
    Top,
    /// Below the workspace body.
    Bottom,
    /// On the leading/start side.
    Start,
    /// On the trailing/end side.
    End,
    /// Not shown.
    Hidden,
}

impl From<BigWorkspaceToolbarPlacement> for BigBandPlacement {
    fn from(placement: BigWorkspaceToolbarPlacement) -> Self {
        match placement {
            BigWorkspaceToolbarPlacement::Top => Self::Top,
            BigWorkspaceToolbarPlacement::Bottom => Self::Bottom,
            BigWorkspaceToolbarPlacement::Start => Self::Start,
            BigWorkspaceToolbarPlacement::End => Self::End,
        }
    }
}

/// Per-band placement policy: which placements the user may choose, the default,
/// and whether the band may be hidden.
///
/// Generalizes [`BigWorkspaceToolbarPolicy`] to every [`BigChromeBand`].
///
/// [`BigWorkspaceToolbarPolicy`]: super::BigWorkspaceToolbarPolicy
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigChromeBandPolicy {
    /// Band this policy configures.
    pub band: BigChromeBand,
    /// Placements the user may choose for this band.
    pub allowed_placements: BTreeSet<BigBandPlacement>,
    /// Initial placement used before persisted settings apply.
    pub default_placement: BigBandPlacement,
    /// Whether the band may be hidden.
    pub can_hide: bool,
}

impl BigChromeBandPolicy {
    /// Create a band policy with `default_placement` as the only allowed
    /// placement and hiding disabled.
    #[must_use]
    pub fn new(band: BigChromeBand, default_placement: BigBandPlacement) -> Self {
        Self {
            band,
            allowed_placements: [default_placement].into_iter().collect(),
            default_placement,
            can_hide: false,
        }
    }

    /// Replace the allowed placements.
    #[must_use]
    pub fn allowed_placements(
        mut self,
        placements: impl IntoIterator<Item = BigBandPlacement>,
    ) -> Self {
        self.allowed_placements = placements.into_iter().collect();
        self
    }

    /// Allow or forbid hiding the band.
    #[must_use]
    pub fn can_hide(mut self, can_hide: bool) -> Self {
        self.can_hide = can_hide;
        self
    }

    pub(super) fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        if self.allowed_placements.is_empty() {
            return Err(BigWindowShellSpecError::EmptyBandPlacementSet);
        }
        if !self.allowed_placements.contains(&self.default_placement) {
            return Err(BigWindowShellSpecError::BandDefaultPlacementNotAllowed);
        }
        Ok(())
    }
}

impl From<&BigWorkspaceToolbarPolicy> for BigChromeBandPolicy {
    fn from(toolbar_policy: &BigWorkspaceToolbarPolicy) -> Self {
        Self {
            band: BigChromeBand::Toolbar,
            allowed_placements: toolbar_policy
                .allowed_placements
                .iter()
                .map(|&placement| BigBandPlacement::from(placement))
                .collect(),
            default_placement: BigBandPlacement::from(toolbar_policy.default_placement),
            can_hide: toolbar_policy.supports_visibility_toggles,
        }
    }
}

/// The app's whole declarative chrome-placement policy: one policy per movable
/// band plus the per-control catalog for zone-based bands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigChromePlacementPolicy {
    /// Per-band placement policies.
    pub bands: Vec<BigChromeBandPolicy>,
    /// Per-control catalog for zone-based bands.
    pub control_catalog: ZoneLayoutSpec,
}

impl BigChromePlacementPolicy {
    /// Create an empty chrome-placement policy: no bands and an empty control
    /// catalog.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bands: Vec::new(),
            control_catalog: ZoneLayoutSpec::new(Vec::new(), Vec::new()),
        }
    }

    /// Add or replace the policy for a band.
    ///
    /// When a policy for the same [`BigChromeBand`] already exists it is
    /// replaced in place; otherwise the new policy is appended.
    #[must_use]
    pub fn band(mut self, band_policy: BigChromeBandPolicy) -> Self {
        if let Some(existing) = self
            .bands
            .iter_mut()
            .find(|existing| existing.band == band_policy.band)
        {
            *existing = band_policy;
        } else {
            self.bands.push(band_policy);
        }
        self
    }

    /// Set the per-control catalog for zone-based bands.
    #[must_use]
    pub fn control_catalog(mut self, control_catalog: ZoneLayoutSpec) -> Self {
        self.control_catalog = control_catalog;
        self
    }

    pub(super) fn validate(&self) -> Result<(), BigWindowShellSpecError> {
        let mut seen_bands = BTreeSet::new();
        for band_policy in &self.bands {
            if !seen_bands.insert(band_policy.band) {
                return Err(BigWindowShellSpecError::DuplicateChromeBand);
            }
            band_policy.validate()?;
        }
        Ok(())
    }
}

impl Default for BigChromePlacementPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::window_shell::*;

    #[test]
    fn band_policy_rejects_empty_allowed_set() {
        let error = BigChromeBandPolicy::new(BigChromeBand::TabStrip, BigBandPlacement::Top)
            .allowed_placements([])
            .validate()
            .expect_err("empty allowed set must fail");

        assert_eq!(error, BigWindowShellSpecError::EmptyBandPlacementSet);
    }

    #[test]
    fn band_policy_rejects_default_not_in_allowed_set() {
        let error = BigChromeBandPolicy::new(BigChromeBand::Toolbar, BigBandPlacement::Top)
            .allowed_placements([BigBandPlacement::Bottom])
            .validate()
            .expect_err("default outside allowed set must fail");

        assert_eq!(
            error,
            BigWindowShellSpecError::BandDefaultPlacementNotAllowed
        );
    }

    #[test]
    fn placement_policy_accepts_distinct_bands() {
        BigChromePlacementPolicy::new()
            .band(BigChromeBandPolicy::new(
                BigChromeBand::Toolbar,
                BigBandPlacement::Top,
            ))
            .band(BigChromeBandPolicy::new(
                BigChromeBand::TabStrip,
                BigBandPlacement::Top,
            ))
            .validate()
            .expect("distinct bands validate");
    }

    #[test]
    fn band_appends_new_and_replaces_same_band_in_place() {
        let policy = BigChromePlacementPolicy::new()
            .band(BigChromeBandPolicy::new(
                BigChromeBand::Toolbar,
                BigBandPlacement::Top,
            ))
            .band(BigChromeBandPolicy::new(
                BigChromeBand::TabStrip,
                BigBandPlacement::Top,
            ))
            // Re-add the same band id: must replace in place, NOT append a third.
            .band(BigChromeBandPolicy::new(
                BigChromeBand::Toolbar,
                BigBandPlacement::Bottom,
            ));

        assert_eq!(
            policy.bands.len(),
            2,
            "re-adding an existing band replaces rather than appends"
        );
        // The replacement carries the new placement, at the original slot.
        assert_eq!(policy.bands[0].band, BigChromeBand::Toolbar);
        assert_eq!(policy.bands[0].default_placement, BigBandPlacement::Bottom);
        assert_eq!(policy.bands[1].band, BigChromeBand::TabStrip);
    }

    #[test]
    fn control_catalog_stores_the_supplied_catalog() {
        use crate::zone_layout::{BarSpec, ZoneControl, ZoneLayoutSpec, ZonePlacement};

        let catalog = ZoneLayoutSpec::new(
            vec![ZoneControl::new(
                "play",
                "Play",
                "media-playback-start",
                "bar:center",
            )],
            vec![BarSpec::new("bar", vec![ZonePlacement::Center])],
        );
        let policy = BigChromePlacementPolicy::new().control_catalog(catalog.clone());

        assert_eq!(policy.control_catalog, catalog);
        assert_eq!(policy.control_catalog.controls.len(), 1);
    }

    #[test]
    fn placement_policy_rejects_duplicate_band() {
        // The add-or-replace `band` builder collapses same-band policies, so a
        // duplicate is constructed through the raw field to exercise detection.
        let mut policy = BigChromePlacementPolicy::new();
        policy.bands.push(BigChromeBandPolicy::new(
            BigChromeBand::Toolbar,
            BigBandPlacement::Top,
        ));
        policy.bands.push(BigChromeBandPolicy::new(
            BigChromeBand::Toolbar,
            BigBandPlacement::Bottom,
        ));

        let error = policy.validate().expect_err("duplicate band must fail");
        assert_eq!(error, BigWindowShellSpecError::DuplicateChromeBand);
    }

    #[test]
    fn placement_policy_propagates_per_band_validation() {
        let mut policy = BigChromePlacementPolicy::new();
        policy.bands.push(
            BigChromeBandPolicy::new(BigChromeBand::Toolbar, BigBandPlacement::Top)
                .allowed_placements([BigBandPlacement::Bottom]),
        );

        let error = policy
            .validate()
            .expect_err("invalid band policy must propagate");
        assert_eq!(
            error,
            BigWindowShellSpecError::BandDefaultPlacementNotAllowed
        );
    }

    #[test]
    fn from_toolbar_policy_maps_placements_and_can_hide() {
        let toolbar_policy = BigWorkspaceToolbarPolicy::new(BigWorkspaceToolbarPlacement::Bottom)
            .allowed_placements([
                BigWorkspaceToolbarPlacement::Bottom,
                BigWorkspaceToolbarPlacement::Start,
                BigWorkspaceToolbarPlacement::End,
            ])
            .visibility_toggles(false);

        let band_policy = BigChromeBandPolicy::from(&toolbar_policy);

        assert_eq!(band_policy.band, BigChromeBand::Toolbar);
        assert_eq!(band_policy.default_placement, BigBandPlacement::Bottom);
        assert_eq!(
            band_policy.allowed_placements,
            [
                BigBandPlacement::Bottom,
                BigBandPlacement::Start,
                BigBandPlacement::End,
            ]
            .into_iter()
            .collect()
        );
        assert!(!band_policy.can_hide);

        let hideable = BigChromeBandPolicy::from(&BigWorkspaceToolbarPolicy::default());
        assert!(hideable.can_hide);
        assert_eq!(hideable.default_placement, BigBandPlacement::Top);
    }

    #[test]
    fn workspace_spec_round_trips_chrome_placement() {
        let policy = BigChromePlacementPolicy::new()
            .band(
                BigChromeBandPolicy::new(BigChromeBand::TabStrip, BigBandPlacement::Top)
                    .allowed_placements([BigBandPlacement::Top, BigBandPlacement::Bottom]),
            )
            .band(BigChromeBandPolicy::from(&BigWorkspaceToolbarPolicy::default()).can_hide(true));

        let resolved =
            BigWorkspaceWindowShellSpec::standard("br.com.biglinux.Chrome", "Chrome Test")
                .chrome_placement(policy.clone())
                .resolve()
                .expect("valid chrome placement resolves");

        assert_eq!(resolved.chrome_placement, Some(policy));
    }
}
