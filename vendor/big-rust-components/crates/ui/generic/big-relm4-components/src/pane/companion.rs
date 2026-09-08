// SPDX-License-Identifier: MIT

//! Generic companion pane surface for app-hosted workspace splits.

use std::any::Any;

use relm4::gtk;
use relm4::gtk::prelude::*;

use super::SplitPaneSurface;

/// Host-supplied companion pane mounted beside another workspace pane.
///
/// Apps use this when a workspace owns the tab/split mechanics, but a host
/// supplies a ready widget from another product plus an opaque lifetime guard.
/// The guard is held for the pane lifetime, and focus is delegated to the
/// supplied focus target.
pub struct CompanionPane {
    root: gtk::Widget,
    focus_target: gtk::Widget,
    _keep_alive: Box<dyn Any>,
}

impl CompanionPane {
    /// Create a companion pane from a host-owned widget and lifetime guard.
    #[must_use]
    pub fn new(root: gtk::Widget, focus_target: gtk::Widget, keep_alive: Box<dyn Any>) -> Self {
        Self::new_with_accessible_label(root, focus_target, keep_alive, None)
    }

    /// Create a companion pane with an explicit accessible wrapper label.
    #[must_use]
    pub fn new_with_accessible_label(
        root: gtk::Widget,
        focus_target: gtk::Widget,
        keep_alive: Box<dyn Any>,
        accessible_label: Option<&str>,
    ) -> Self {
        root.set_vexpand(true);
        root.set_hexpand(true);
        let pane_root = if let Some(accessible_label) = accessible_label {
            let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
            wrapper.set_accessible_role(gtk::AccessibleRole::Group);
            wrapper.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
            wrapper.append(&root);
            wrapper.upcast()
        } else {
            root
        };
        pane_root.set_vexpand(true);
        pane_root.set_hexpand(true);
        Self {
            root: pane_root,
            focus_target,
            _keep_alive: keep_alive,
        }
    }
}

impl SplitPaneSurface for CompanionPane {
    fn pane_root(&self) -> gtk::Widget {
        self.root.clone()
    }

    fn set_header_visible(&self, _visible: bool) {}

    fn grab_focus(&self) {
        self.focus_target.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::{CompanionPane, SplitPaneSurface};

    #[test]
    fn companion_pane_is_a_split_surface() {
        fn assert_split_surface<T: SplitPaneSurface>() {}

        assert_split_surface::<CompanionPane>();
    }
}
