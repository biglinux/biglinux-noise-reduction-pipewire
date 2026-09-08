// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Application and window action contracts.
//!
//! The display-free specs (`BigActionScope`, `BigActionSpec`,
//! `BigActionRegistry`) live in `big_app_kit_core::actions` and are
//! re-exported below; the GIO installers stay here where gtk-rs is available.

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use relm4::gtk;

#[doc(inline)]
pub use big_app_kit_core::actions::{BigActionRegistry, BigActionScope, BigActionSpec};

/// Register the standard `app.quit` action on `app`. Activating it
/// terminates the GApplication loop.
pub fn install_quit_action<A>(app: &A)
where
    A: IsA<gio::Application> + IsA<gio::ActionMap> + Clone + 'static,
{
    let quit_app = app.clone();
    let quit = gio::ActionEntry::builder("quit")
        .activate(move |_, _, _| quit_app.quit())
        .build();
    app.add_action_entries([quit]);
}

/// Bind each `(action, [accelerators])` pair on `app` so the listed
/// keystrokes trigger the matching action.
pub fn install_static_accels<A>(app: &A, accels: &[(&str, &[&str])])
where
    A: IsA<gtk::Application>,
{
    for (action, bindings) in accels {
        app.set_accels_for_action(action, bindings);
    }
}

/// Register a parameterless [`gio::SimpleAction`] (enabled by default)
/// on `action_map`. Convenience wrapper over
/// [`install_action_enabled`].
pub fn install_action<M>(
    action_map: &M,
    name: &str,
    on_activate: impl Fn() + 'static,
) -> gio::SimpleAction
where
    M: IsA<gio::ActionMap>,
{
    install_action_enabled(action_map, name, true, on_activate)
}

/// Register a [`gio::SimpleAction`] on `action_map` with an initial enabled flag.
///
/// Toggling [`gio::SimpleAction::set_enabled`] on the returned action drives
/// the sensitivity of every widget bound to `<group>.<name>` through the
/// [`gtk::Widget`] `action-name` property, so callers can grey-out menus
/// and buttons without tracking individual widget references.
pub fn install_action_enabled<M>(
    action_map: &M,
    name: &str,
    enabled: bool,
    on_activate: impl Fn() + 'static,
) -> gio::SimpleAction
where
    M: IsA<gio::ActionMap>,
{
    let action = gio::SimpleAction::new(name, None);
    action.set_enabled(enabled);
    action.connect_activate(move |_, _| on_activate());
    action_map.add_action(&action);
    action
}

/// Register an action that takes an `i32` parameter and forwards it as
/// a `usize` to `on_activate`. Negative or out-of-range values are
/// silently ignored so callers can wire numeric menus without
/// defensive parsing.
pub fn install_i32_action<M>(
    action_map: &M,
    name: &str,
    on_activate: impl Fn(usize) + 'static,
) -> gio::SimpleAction
where
    M: IsA<gio::ActionMap>,
{
    let action = gio::SimpleAction::new(name, Some(&i32::static_variant_type()));
    action.connect_activate(move |_, param| {
        let Some(idx_i32) = param.and_then(glib::Variant::get::<i32>) else {
            return;
        };
        let Ok(idx) = usize::try_from(idx_i32) else {
            return;
        };
        on_activate(idx);
    });
    action_map.add_action(&action);
    action
}

/// Register an action that takes a `String` parameter and forwards it
/// to `on_activate`. Non-string variants are dropped silently.
pub fn install_string_action<M>(
    action_map: &M,
    name: &str,
    on_activate: impl Fn(String) + 'static,
) -> gio::SimpleAction
where
    M: IsA<gio::ActionMap>,
{
    let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
    action.connect_activate(move |_, param| {
        let Some(value) = param.and_then(glib::Variant::get::<String>) else {
            return;
        };
        on_activate(value);
    });
    action_map.add_action(&action);
    action
}

/// Register a stateful boolean [`gio::SimpleAction`] on `action_map`.
///
/// Activation toggles the current state; `change-state` writes the new
/// value and invokes `on_change` with the freshly applied boolean. The
/// state type is `b`, matching `<menuitem action="..."/>` checkboxes.
pub fn install_toggle_action<M, F>(
    action_map: &M,
    name: &str,
    initial_state: bool,
    on_change: F,
) -> gio::SimpleAction
where
    M: IsA<gio::ActionMap>,
    F: Fn(&gio::SimpleAction, bool) + 'static,
{
    let action = gio::SimpleAction::new_stateful(name, None, &initial_state.to_variant());
    action.connect_activate(|action, _| {
        let current = action
            .state()
            .and_then(|s| s.get::<bool>())
            .unwrap_or(false);
        action.change_state(&(!current).to_variant());
    });
    action.connect_change_state(move |action, value| {
        let Some(new_state) = value.and_then(glib::Variant::get::<bool>) else {
            return;
        };
        action.set_state(&new_state.to_variant());
        on_change(action, new_state);
    });
    action_map.add_action(&action);
    action
}

/// Register a stateful string [`gio::SimpleAction`] modelling a radio
/// group. Activation parameter is the chosen value (`s` type); state
/// tracks the currently selected entry. `initial_value` must be one of
/// `choices` — matches the silent-drop convention of
/// [`install_i32_action`] / [`install_string_action`] by ignoring the
/// install when validation fails (debug builds panic via
/// `debug_assert!`).
pub fn install_radio_action<M, F>(
    action_map: &M,
    name: &str,
    initial_value: &str,
    choices: &[&str],
    on_change: F,
) -> gio::SimpleAction
where
    M: IsA<gio::ActionMap>,
    F: Fn(&gio::SimpleAction, &str) + 'static,
{
    debug_assert!(
        choices.contains(&initial_value),
        "install_radio_action: initial_value `{initial_value}` not in choices"
    );

    let action = gio::SimpleAction::new_stateful(
        name,
        Some(glib::VariantTy::STRING),
        &initial_value.to_variant(),
    );
    let allowed: Vec<String> = choices.iter().map(|s| (*s).to_string()).collect();
    action.connect_activate(|action, param| {
        if let Some(param) = param {
            action.change_state(param);
        }
    });
    action.connect_change_state(move |action, value| {
        let Some(new_state) = value.and_then(glib::Variant::get::<String>) else {
            return;
        };
        if !allowed.iter().any(|c| c == &new_state) {
            return;
        }
        action.set_state(&new_state.to_variant());
        on_change(action, &new_state);
    });
    action_map.add_action(&action);
    action
}
