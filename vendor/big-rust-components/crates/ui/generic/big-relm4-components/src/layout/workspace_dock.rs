// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared dock pane for workspace windows.
//!
//! This widget owns the reusable `GtkPaned` layout used when a workspace body
//! has an app-owned dock on any side. Consumers keep their own persistence keys,
//! domain widgets, and visibility policy; the framework owns orientation,
//! child ordering, resize flags, and panel-size calculations.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;

/// Side where a workspace dock is mounted relative to the primary content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigWorkspaceDockSide {
    /// Dock is above the primary content.
    Top,
    /// Dock is below the primary content.
    Bottom,
    /// Dock is left of the primary content.
    Left,
    /// Dock is right of the primary content.
    Right,
}

impl BigWorkspaceDockSide {
    /// Parse the stable lowercase value used by app settings.
    #[must_use]
    pub fn from_persisted_value(value: &str) -> Option<Self> {
        match value {
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    /// Stable lowercase value for app settings.
    #[must_use]
    pub fn as_persisted_value(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    fn orientation(self) -> gtk::Orientation {
        match self {
            Self::Top | Self::Bottom => gtk::Orientation::Vertical,
            Self::Left | Self::Right => gtk::Orientation::Horizontal,
        }
    }

    fn dock_is_start(self) -> bool {
        matches!(self, Self::Top | Self::Left)
    }
}

/// Reusable `GtkPaned` shell for a workspace body plus side dock.
#[derive(Clone)]
pub struct BigWorkspaceDockPane {
    inner: Rc<BigWorkspaceDockPaneInner>,
}

struct BigWorkspaceDockPaneInner {
    paned: gtk::Paned,
    content: gtk::Widget,
    dock: RefCell<gtk::Widget>,
    side: Cell<BigWorkspaceDockSide>,
    is_dock_visible: Cell<bool>,
}

impl BigWorkspaceDockPane {
    /// Build a dock pane with `dock` mounted on `side`.
    #[must_use]
    pub fn new(
        content: &impl IsA<gtk::Widget>,
        dock: &impl IsA<gtk::Widget>,
        side: BigWorkspaceDockSide,
    ) -> Self {
        let paned = gtk::Paned::builder()
            .orientation(side.orientation())
            .resize_start_child(true)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .css_classes(["big-workspace-dock-pane"])
            .build();
        let dock_pane = Self {
            inner: Rc::new(BigWorkspaceDockPaneInner {
                paned,
                content: content.clone().upcast(),
                dock: RefCell::new(dock.clone().upcast()),
                side: Cell::new(side),
                is_dock_visible: Cell::new(true),
            }),
        };
        dock_pane.apply_side(side);
        dock_pane
    }

    /// Borrow the underlying `GtkPaned` root.
    #[must_use]
    pub fn root_widget(&self) -> &gtk::Paned {
        &self.inner.paned
    }

    /// Current dock side.
    #[must_use]
    pub fn side(&self) -> BigWorkspaceDockSide {
        self.inner.side.get()
    }

    /// Move the dock to another side while preserving the same content widgets.
    pub fn set_side(&self, side: BigWorkspaceDockSide) {
        if self.inner.side.replace(side) == side {
            return;
        }
        self.apply_side(side);
    }

    /// Replace the app-owned dock widget while preserving side and visibility.
    pub fn set_dock_widget(&self, dock: &impl IsA<gtk::Widget>) {
        *self.inner.dock.borrow_mut() = dock.clone().upcast();
        self.apply_side(self.side());
    }

    /// Show or hide the dock region without destroying the app-owned widget.
    pub fn set_dock_visible(&self, is_visible: bool) {
        if self.inner.is_dock_visible.replace(is_visible) == is_visible {
            return;
        }
        self.apply_side(self.side());
    }

    /// Whether the dock region is currently mounted.
    #[must_use]
    pub fn is_dock_visible(&self) -> bool {
        self.inner.is_dock_visible.get()
    }

    /// Apply a persisted panel size, falling back to a legacy raw paned position.
    pub fn apply_saved_position(&self, panel_size: i32, legacy_position: i32) {
        match self.side() {
            BigWorkspaceDockSide::Top | BigWorkspaceDockSide::Left => {
                self.inner.paned.set_position(panel_size);
            }
            BigWorkspaceDockSide::Bottom => {
                let total_height = self.inner.paned.height();
                self.inner.paned.set_position(end_dock_position(
                    total_height,
                    panel_size,
                    legacy_position,
                ));
            }
            BigWorkspaceDockSide::Right => {
                let total_width = self.inner.paned.width();
                self.inner.paned.set_position(end_dock_position(
                    total_width,
                    panel_size,
                    legacy_position,
                ));
            }
        }
    }

    /// Apply a panel size along the current dock axis.
    pub fn apply_dock_size(&self, panel_size: i32) {
        match self.side() {
            BigWorkspaceDockSide::Top | BigWorkspaceDockSide::Left => {
                self.inner.paned.set_position(panel_size);
            }
            BigWorkspaceDockSide::Bottom | BigWorkspaceDockSide::Right => {
                let total_size = match self.side().orientation() {
                    gtk::Orientation::Horizontal => self.inner.paned.width(),
                    gtk::Orientation::Vertical => self.inner.paned.height(),
                    _ => 0,
                };
                if total_size <= 0 {
                    let paned_weak = self.inner.paned.downgrade();
                    let side = self.side();
                    gtk::glib::idle_add_local_once(move || {
                        if let Some(paned) = paned_weak.upgrade() {
                            let total_size = match side.orientation() {
                                gtk::Orientation::Horizontal => paned.width(),
                                gtk::Orientation::Vertical => paned.height(),
                                _ => 0,
                            };
                            if total_size > panel_size {
                                paned.set_position(total_size - panel_size);
                            }
                        }
                    });
                } else if total_size > panel_size {
                    self.inner.paned.set_position(total_size - panel_size);
                }
            }
        }
    }

    /// Calculate the current dock extent from the paned position.
    #[must_use]
    pub fn dock_size_from_position(&self, minimum_size: i32) -> Option<i32> {
        if !self.is_dock_visible() {
            return None;
        }
        let total_size = match self.side().orientation() {
            gtk::Orientation::Horizontal => self.inner.paned.width(),
            gtk::Orientation::Vertical => self.inner.paned.height(),
            _ => 0,
        };
        dock_size_from_position(
            self.side(),
            total_size,
            self.inner.paned.position(),
            minimum_size,
        )
    }

    fn apply_side(&self, side: BigWorkspaceDockSide) {
        self.inner.paned.set_start_child(gtk::Widget::NONE);
        self.inner.paned.set_end_child(gtk::Widget::NONE);
        self.inner.paned.set_orientation(side.orientation());
        self.inner.paned.set_shrink_start_child(false);
        self.inner.paned.set_shrink_end_child(false);
        self.inner
            .paned
            .set_resize_start_child(!side.dock_is_start());
        self.inner.paned.set_resize_end_child(side.dock_is_start());
        if !self.is_dock_visible() {
            self.inner.paned.set_start_child(Some(&self.inner.content));
            return;
        }
        let dock = self.inner.dock.borrow().clone();
        if side.dock_is_start() {
            self.inner.paned.set_start_child(Some(&dock));
            self.inner.paned.set_end_child(Some(&self.inner.content));
        } else {
            self.inner.paned.set_start_child(Some(&self.inner.content));
            self.inner.paned.set_end_child(Some(&dock));
        }
    }
}

fn end_dock_position(total_size: i32, panel_size: i32, legacy_position: i32) -> i32 {
    if total_size > panel_size {
        total_size - panel_size
    } else {
        legacy_position
    }
}

fn dock_size_from_position(
    side: BigWorkspaceDockSide,
    total_size: i32,
    position: i32,
    minimum_size: i32,
) -> Option<i32> {
    if total_size <= 0 {
        return None;
    }
    let dock_size = if side.dock_is_start() {
        position
    } else {
        total_size - position
    };
    (dock_size >= minimum_size).then_some(dock_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_side_maps_to_orientation_and_child_order() {
        assert_eq!(
            BigWorkspaceDockSide::Top.orientation(),
            gtk::Orientation::Vertical
        );
        assert_eq!(
            BigWorkspaceDockSide::Bottom.orientation(),
            gtk::Orientation::Vertical
        );
        assert_eq!(
            BigWorkspaceDockSide::Left.orientation(),
            gtk::Orientation::Horizontal
        );
        assert_eq!(
            BigWorkspaceDockSide::Right.orientation(),
            gtk::Orientation::Horizontal
        );
        assert!(BigWorkspaceDockSide::Top.dock_is_start());
        assert!(BigWorkspaceDockSide::Left.dock_is_start());
        assert!(!BigWorkspaceDockSide::Bottom.dock_is_start());
        assert!(!BigWorkspaceDockSide::Right.dock_is_start());
    }

    #[test]
    fn dock_side_uses_stable_persisted_values() {
        for (raw, side) in [
            ("top", BigWorkspaceDockSide::Top),
            ("bottom", BigWorkspaceDockSide::Bottom),
            ("left", BigWorkspaceDockSide::Left),
            ("right", BigWorkspaceDockSide::Right),
        ] {
            assert_eq!(BigWorkspaceDockSide::from_persisted_value(raw), Some(side));
            assert_eq!(side.as_persisted_value(), raw);
        }
        assert_eq!(BigWorkspaceDockSide::from_persisted_value("invalid"), None);
    }

    #[test]
    fn end_dock_position_prefers_panel_size_when_total_is_known() {
        assert_eq!(end_dock_position(720, 240, 100), 480);
        assert_eq!(end_dock_position(100, 240, 88), 88);
    }

    #[test]
    fn dock_size_from_position_handles_start_and_end_docks() {
        assert_eq!(
            dock_size_from_position(BigWorkspaceDockSide::Left, 1000, 250, 160),
            Some(250)
        );
        assert_eq!(
            dock_size_from_position(BigWorkspaceDockSide::Right, 1000, 760, 160),
            Some(240)
        );
        assert_eq!(
            dock_size_from_position(BigWorkspaceDockSide::Bottom, 1000, 900, 160),
            None
        );
        assert_eq!(
            dock_size_from_position(BigWorkspaceDockSide::Bottom, 0, 760, 160),
            None
        );
    }
}
