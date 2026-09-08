// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Safe casts for GTK signal list factory callbacks.
//!
//! GTK hands factory callbacks generic `glib::Object` values. Shared apps should
//! guard those casts and return from the callback when GTK provides an
//! unexpected object, instead of panicking in recycled rows.

use gtk::glib;
use gtk::prelude::*;

/// Downcast a factory callback value to the [`gtk::ListItem`] it should carry.
#[must_use]
pub fn list_item_from_factory_value(
    factory_callback_value: &glib::Object,
) -> Option<&gtk::ListItem> {
    factory_callback_value.downcast_ref::<gtk::ListItem>()
}

/// Downcast the model object currently bound to a [`gtk::ListItem`].
#[must_use]
pub fn bound_model_as<BoundModel>(list_item: &gtk::ListItem) -> Option<BoundModel>
where
    BoundModel: IsA<glib::Object>,
{
    list_item.item().and_downcast::<BoundModel>()
}

/// Downcast the widget child installed by a [`gtk::ListItem`] setup callback.
#[must_use]
pub fn child_as<ChildWidget>(list_item: &gtk::ListItem) -> Option<ChildWidget>
where
    ChildWidget: IsA<gtk::Widget>,
{
    list_item.child().and_downcast::<ChildWidget>()
}

#[cfg(test)]
mod tests {
    use super::*;

    // GObject downcasting depends on GLib's runtime type registry; ordinary
    // tests cover the GTK boundary while Miri stays on Rust-owned invariants.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn plain_gobject_is_not_a_list_item() {
        let factory_callback_value = glib::Object::new::<glib::Object>();

        assert!(list_item_from_factory_value(&factory_callback_value).is_none());
    }
}
