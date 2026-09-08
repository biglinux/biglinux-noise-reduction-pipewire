// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Reusable config-placed chrome-band host: the live GTK adopter of the F3.0
//! chrome-as-data band model
//! ([`BigChromeBand`](big_app_kit::window_shell::BigChromeBand) /
//! [`BigBandPlacement`]).
//!
//! An app owns an opaque band widget (a command toolbar, a status band, …) and
//! a "home" host box. This host reparents that band — leak-safely, following the
//! same unparent-before-append discipline as
//! [`zone_applier`](crate::layout::zone_applier) and
//! [`BigTabStripPlacement`](crate::layout::tab_strip_placement::BigTabStripPlacement)
//! — between the app-supplied home host, one pre-built dock per allowed window
//! edge, or nowhere (hidden). Orientation of the band's own inner layout is left
//! to the app through the placement-applied hook, so the host stays band-generic
//! (it never assumes the band is a horizontal toolbar).

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use big_app_kit::window_shell::BigBandPlacement;
use relm4::gtk;
use relm4::gtk::prelude::*;

/// Callback invoked after a placement is applied, with the new placement.
///
/// Implementations must capture GTK widgets weakly (or via `Rc<Cell>`): the
/// host stores this callback for the band's lifetime, so a strong widget
/// capture here would root the widget tree.
pub type BigChromeBandPlacementCallback = Rc<dyn Fn(BigBandPlacement)>;

/// One non-home band mount host at a single window edge.
#[derive(Clone)]
struct ChromeBandDock {
    /// Draggable dock root inserted into the window layout (hidden until the
    /// band is placed here).
    root: gtk::Widget,
    /// Content box the band widget is reparented into.
    content: gtk::Box,
}

/// The four optional per-edge docks. A dock is built only for an edge that is
/// both in the allowed set and not the home placement (the home edge mounts in
/// the app-supplied home host, never in a dock).
#[derive(Clone)]
struct ChromeBandDocks {
    top: Option<ChromeBandDock>,
    bottom: Option<ChromeBandDock>,
    start: Option<ChromeBandDock>,
    end: Option<ChromeBandDock>,
}

impl ChromeBandDocks {
    fn dock_for(&self, placement: BigBandPlacement) -> Option<&ChromeBandDock> {
        match placement {
            BigBandPlacement::Top => self.top.as_ref(),
            BigBandPlacement::Bottom => self.bottom.as_ref(),
            BigBandPlacement::Start => self.start.as_ref(),
            BigBandPlacement::End => self.end.as_ref(),
            _ => None,
        }
    }

    fn present(&self) -> impl Iterator<Item = &ChromeBandDock> {
        [&self.top, &self.bottom, &self.start, &self.end]
            .into_iter()
            .flatten()
    }
}

/// Owns the reusable per-band placement widgets and the leak-safe reparent
/// policy for one movable window-chrome band.
///
/// The band widget starts in the app-supplied `home_host`.
/// [`set_placement`](Self::set_placement) reparents it to the matching edge dock
/// (for a non-home edge), back to the home host (for the home placement), or
/// unparents and hides it ([`BigBandPlacement::Hidden`]).
#[derive(Clone)]
pub struct BigChromeBandHost {
    band: gtk::Widget,
    home_host: gtk::Box,
    home_placement: BigBandPlacement,
    docks: ChromeBandDocks,
    placement: Rc<Cell<BigBandPlacement>>,
    on_placement_applied: Rc<RefCell<Option<BigChromeBandPlacementCallback>>>,
}

impl BigChromeBandHost {
    /// Build a band host around an app-owned `band` widget.
    ///
    /// - `home_host` is the app-owned box the band lives in at its
    ///   `home_placement` (kept app-local so app-specific chrome — e.g. a reveal
    ///   revealer — stays outside this generic host).
    /// - `home_placement` is the band's default placement; the band is mounted
    ///   into `home_host` immediately.
    /// - `allowed` gates which edge docks are built. The home edge never gets a
    ///   dock (it mounts in `home_host`); [`BigBandPlacement::Hidden`] and
    ///   [`BigBandPlacement::InHeaderbar`] never get an edge dock.
    /// - `accessible_names` labels the `[top, bottom, start, end]` edge docks.
    #[must_use]
    pub fn new(
        band: &impl IsA<gtk::Widget>,
        home_host: &gtk::Box,
        home_placement: BigBandPlacement,
        allowed: &BTreeSet<BigBandPlacement>,
        accessible_names: [&str; 4],
    ) -> Self {
        let band: gtk::Widget = band.clone().upcast();
        let build_edge = |placement: BigBandPlacement, vertical: bool, suffix: &str, name: &str| {
            (placement != home_placement && allowed.contains(&placement))
                .then(|| build_band_dock(vertical, suffix, name))
        };
        let docks = ChromeBandDocks {
            top: build_edge(BigBandPlacement::Top, false, "top", accessible_names[0]),
            bottom: build_edge(
                BigBandPlacement::Bottom,
                false,
                "bottom",
                accessible_names[1],
            ),
            start: build_edge(BigBandPlacement::Start, true, "start", accessible_names[2]),
            end: build_edge(BigBandPlacement::End, true, "end", accessible_names[3]),
        };
        reparent_band_into(&band, home_host);
        Self {
            band,
            home_host: home_host.clone(),
            home_placement,
            docks,
            placement: Rc::new(Cell::new(home_placement)),
            on_placement_applied: Rc::new(RefCell::new(None)),
        }
    }

    /// Current band placement.
    #[must_use]
    pub fn placement(&self) -> BigBandPlacement {
        self.placement.get()
    }

    /// Return the dock root for a non-home edge placement.
    ///
    /// `None` for the home placement, [`BigBandPlacement::Hidden`],
    /// [`BigBandPlacement::InHeaderbar`], and any edge not in the allowed set.
    #[must_use]
    pub fn dock_root(&self, placement: BigBandPlacement) -> Option<gtk::Widget> {
        self.docks.dock_for(placement).map(|dock| dock.root.clone())
    }

    /// Set a callback invoked after each successful placement change.
    pub fn set_on_placement_applied(&self, callback: BigChromeBandPlacementCallback) {
        *self.on_placement_applied.borrow_mut() = Some(callback);
    }

    /// Reparent the band to `placement`. Idempotent per placement; safe to call
    /// live. [`BigBandPlacement::Hidden`] unparents the band and hides every
    /// dock. The band's own inner-layout orientation is the app's responsibility
    /// via the placement-applied hook.
    pub fn set_placement(&self, placement: BigBandPlacement) {
        if self.placement.get() == placement {
            return;
        }
        self.apply_placement(placement);
        self.placement.set(placement);
        if let Some(callback) = self.on_placement_applied.borrow().as_ref() {
            callback(placement);
        }
    }

    fn apply_placement(&self, placement: BigBandPlacement) {
        match placement {
            BigBandPlacement::Hidden => {
                unparent_band(&self.band);
                self.band.set_visible(false);
                self.hide_all_docks();
            }
            _ if placement == self.home_placement => {
                self.band.set_visible(true);
                reparent_band_into(&self.band, &self.home_host);
                self.hide_all_docks();
            }
            _ => match self.docks.dock_for(placement) {
                Some(dock) => {
                    self.band.set_visible(true);
                    reparent_band_into(&self.band, &dock.content);
                    self.hide_all_docks();
                    dock.root.set_visible(true);
                }
                None => {
                    // Unsupported placement (neither home nor a built edge):
                    // fall back to the home host so the band never leaks
                    // unparented.
                    self.band.set_visible(true);
                    reparent_band_into(&self.band, &self.home_host);
                    self.hide_all_docks();
                }
            },
        }
    }

    fn hide_all_docks(&self) {
        for dock in self.docks.present() {
            dock.root.set_visible(false);
        }
    }
}

/// Build one hidden edge dock: a content box (horizontal for top/bottom,
/// vertical for start/end) inside a draggable `WindowHandle` root.
fn build_band_dock(vertical: bool, css_suffix: &str, accessible_name: &str) -> ChromeBandDock {
    let dock_class = format!("big-chrome-band-dock-{css_suffix}");
    let content = gtk::Box::builder()
        .orientation(if vertical {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        })
        .css_classes([dock_class.as_str(), "big-chrome-band-dock"])
        .build();
    if vertical {
        content.set_vexpand(true);
    } else {
        content.set_hexpand(true);
    }
    content.set_accessible_role(gtk::AccessibleRole::Group);
    content.update_property(&[gtk::accessible::Property::Label(accessible_name)]);
    let root = gtk::WindowHandle::builder()
        .child(&content)
        .css_classes(["big-chrome-band-dock"])
        .build();
    root.set_visible(false);
    ChromeBandDock {
        root: root.upcast(),
        content,
    }
}

/// Leak-safe reparent: no-op if `band` is already a child of `target`; else
/// unparent it from its old parent before appending to `target`.
fn reparent_band_into(band: &gtk::Widget, target: &gtk::Box) {
    if let Some(parent) = band.parent() {
        if &parent == target.upcast_ref::<gtk::Widget>() {
            return;
        }
        band.unparent();
    }
    target.append(band);
}

/// Unparent the band from any current parent (for the hidden placement).
fn unparent_band(band: &gtk::Widget) {
    if band.parent().is_some() {
        band.unparent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed_all() -> BTreeSet<BigBandPlacement> {
        [
            BigBandPlacement::Top,
            BigBandPlacement::Bottom,
            BigBandPlacement::Start,
            BigBandPlacement::End,
            BigBandPlacement::Hidden,
        ]
        .into_iter()
        .collect()
    }

    fn names() -> [&'static str; 4] {
        ["Top band", "Bottom band", "Start band", "End band"]
    }

    fn gtk_ready() -> bool {
        gtk::init().is_ok()
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn host_starts_in_home_and_exposes_non_home_edge_docks() {
        if !gtk_ready() {
            return;
        }
        let band = gtk::Button::new();
        let home_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let host = BigChromeBandHost::new(
            &band,
            &home_host,
            BigBandPlacement::Top,
            &allowed_all(),
            names(),
        );

        assert_eq!(host.placement(), BigBandPlacement::Top);
        assert_eq!(home_host.first_child(), Some(band.clone().upcast()));
        // Top is the home placement → no dock; Hidden → no dock.
        assert!(host.dock_root(BigBandPlacement::Top).is_none());
        assert!(host.dock_root(BigBandPlacement::Hidden).is_none());
        // The other allowed edges each get a dock.
        assert!(host.dock_root(BigBandPlacement::Bottom).is_some());
        assert!(host.dock_root(BigBandPlacement::Start).is_some());
        assert!(host.dock_root(BigBandPlacement::End).is_some());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn set_placement_reparents_to_edge_dock_and_fires_hook() {
        if !gtk_ready() {
            return;
        }
        let band = gtk::Button::new();
        let home_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let host = BigChromeBandHost::new(
            &band,
            &home_host,
            BigBandPlacement::Top,
            &allowed_all(),
            names(),
        );
        let applied = Rc::new(Cell::new(None));
        let applied_for_hook = applied.clone();
        host.set_on_placement_applied(Rc::new(move |placement| {
            applied_for_hook.set(Some(placement));
        }));

        host.set_placement(BigBandPlacement::Bottom);

        assert_eq!(host.placement(), BigBandPlacement::Bottom);
        assert_eq!(applied.get(), Some(BigBandPlacement::Bottom));
        // Band left the home host and landed in the bottom dock content.
        assert!(home_host.first_child().is_none());
        assert_eq!(
            host.docks.bottom.as_ref().unwrap().content.first_child(),
            Some(band.clone().upcast())
        );
        assert!(
            host.dock_root(BigBandPlacement::Bottom)
                .unwrap()
                .is_visible()
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn hidden_unparents_band_and_hides_docks() {
        if !gtk_ready() {
            return;
        }
        let band = gtk::Button::new();
        let home_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let host = BigChromeBandHost::new(
            &band,
            &home_host,
            BigBandPlacement::Top,
            &allowed_all(),
            names(),
        );
        // Move to an edge first, then hide, to prove hide unparents from a dock.
        host.set_placement(BigBandPlacement::Start);
        host.set_placement(BigBandPlacement::Hidden);

        assert_eq!(host.placement(), BigBandPlacement::Hidden);
        assert!(band.parent().is_none());
        assert!(!band.is_visible());
        for placement in [
            BigBandPlacement::Bottom,
            BigBandPlacement::Start,
            BigBandPlacement::End,
        ] {
            assert!(!host.dock_root(placement).unwrap().is_visible());
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn returning_home_reparents_band_back_into_home_host() {
        if !gtk_ready() {
            return;
        }
        let band = gtk::Button::new();
        let home_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let host = BigChromeBandHost::new(
            &band,
            &home_host,
            BigBandPlacement::Top,
            &allowed_all(),
            names(),
        );

        host.set_placement(BigBandPlacement::End);
        host.set_placement(BigBandPlacement::Top);

        assert_eq!(host.placement(), BigBandPlacement::Top);
        assert_eq!(home_host.first_child(), Some(band.clone().upcast()));
        assert!(!host.dock_root(BigBandPlacement::End).unwrap().is_visible());
    }
}
