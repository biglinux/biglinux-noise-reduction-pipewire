// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! GTK adapter primitives for the app bootstrap contract.
//!
//! The display-free spec lives in [`big_app_kit_core::app_shell`]; the
//! top-level `run` orchestrator lives in `big_relm4_components::app_shell`
//! (it also needs the shared resource bundle and theme helpers). This module
//! holds the reusable, component-free GTK glue every adopter uses:
//!
//! - `AppShellContext` — the handle an app's window build receives (the
//!   `adw::Application`, the settings store, the optional suite store, and the
//!   window-state spec).
//! - `BigAppWindow` — the app's window type: it builds the window and, via
//!   default hooks, reacts to file arguments and dark-mode changes.
//! - `register_shell_actions_on` — install the `GAction`s named by a
//!   window-shell spec's action ids, wiring accelerators from the same specs so
//!   an app never repeats an action id. `AppShellContext::register_shell_actions`
//!   is the thin `AppShellContext` form; the free function serves window build
//!   paths that never receive a context (a big-host embed, a runtime tab-detach).
//! - `AppShellContext::bind_window_state` — restore and persist durable window
//!   geometry using the `BigWindowStateKeys` derived from the spec.

use std::collections::HashMap;

use gtk::gio;
use gtk::prelude::*;
use relm4::gtk;
use serde_json::Value;

use crate::actions::install_action;
use crate::settings_store::BigSettingsStore;
use crate::window_shell::{BigWindowActionSpec, BigWindowStatePersistenceSpec};

#[doc(inline)]
pub use big_app_kit_core::app_shell::{
    AppShellSpec, AppShellSpecError, BigAppearanceBoot, BigLegacyImport, BigSettingsLocation,
    BigWindowStateKeys,
};

/// Everything an app's window build needs from the running shell.
///
/// Constructed by the `run` orchestrator and handed to [`BigAppWindow::build`].
/// It is the documented escape hatch: an app can read the application and its
/// stores and do everything else itself.
pub struct AppShellContext<'a> {
    /// The running libadwaita application.
    pub app: &'a adw::Application,
    /// The app settings store, already opened, defaulted, and version-stamped.
    pub settings: &'a BigSettingsStore,
    /// The shared suite store, when the spec requested it.
    pub suite: Option<&'a BigSettingsStore>,
    /// The durable window-state spec, when the app requested persistence.
    pub window_state: Option<&'a BigWindowStatePersistenceSpec>,
}

/// One activation handler bound to a window-shell action id.
///
/// The `action_id` matches a [`BigWindowActionSpec::action_id`] (for example
/// `"win.about"`); [`AppShellContext::register_shell_actions`] installs the bare
/// `GAction` name and, from the same spec, the accelerator — so the id lives in
/// exactly one place.
pub struct BigShellActionHandler {
    action_id: String,
    activate: Box<dyn Fn() + 'static>,
}

impl BigShellActionHandler {
    /// Bind `activate` to the action named `action_id`.
    #[must_use]
    pub fn new(action_id: impl Into<String>, activate: impl Fn() + 'static) -> Self {
        Self {
            action_id: action_id.into(),
            activate: Box::new(activate),
        }
    }
}

/// Install the `GAction`s described by `actions` on `action_map`, one per
/// matching handler, binding each action's accelerator (from the same spec) on
/// `app`.
///
/// The action id in the spec (`"win.about"`) is both the `GAction` name (its
/// bare part, `"about"`, added to `action_map`) and the accelerator target
/// (`"win.about"`, set on `app`). Every id therefore lives in the spec alone;
/// this call is the single wiring point. A spec without a handler, or a handler
/// without a spec, is logged as an error and skipped — a wiring bug, never a
/// silent drop.
///
/// This is the context-free form: it takes the application explicitly so a
/// window build path that never receives an [`AppShellContext`] (a big-host
/// embed, a runtime tab-detach) registers the same actions the AppShell primary
/// does through [`AppShellContext::register_shell_actions`].
pub fn register_shell_actions_on(
    app: &impl IsA<gtk::Application>,
    action_map: &impl IsA<gio::ActionMap>,
    actions: &[BigWindowActionSpec],
    handlers: Vec<BigShellActionHandler>,
) {
    let mut handlers: HashMap<String, Box<dyn Fn() + 'static>> = handlers
        .into_iter()
        .map(|handler| (handler.action_id, handler.activate))
        .collect();

    for action in actions {
        let Some(activate) = handlers.remove(&action.action_id) else {
            log::error!(
                "app shell: shell action `{}` has no handler; button will do nothing",
                action.action_id
            );
            continue;
        };
        install_action(action_map, bare_action_name(&action.action_id), move || {
            activate();
        });
        if let Some(accelerator) = &action.accelerator {
            app.set_accels_for_action(&action.action_id, &[accelerator.as_str()]);
        }
    }

    for orphan_action_id in handlers.keys() {
        log::error!("app shell: handler `{orphan_action_id}` matches no shell action spec");
    }
}

impl AppShellContext<'_> {
    /// Install the shell `GAction`s and their accelerators for this context.
    ///
    /// Thin wrapper over [`register_shell_actions_on`] that supplies the
    /// context's application; see it for the id/accelerator/wiring contract.
    pub fn register_shell_actions(
        &self,
        action_map: &impl IsA<gio::ActionMap>,
        actions: &[BigWindowActionSpec],
        handlers: Vec<BigShellActionHandler>,
    ) {
        register_shell_actions_on(self.app, action_map, actions, handlers);
    }

    /// Restore durable geometry, then keep it persisted for this window.
    ///
    /// No-op when the spec carries no [`AppShellContext::window_state`]. Restore
    /// honors each `should_restore_*` flag and only overrides the window's own
    /// defaults for keys that are actually stored. Persistence rides the settings
    /// store's own debounce on every geometry change and flushes on close, so no
    /// separate timer is needed.
    pub fn bind_window_state(&self, window: &impl IsA<gtk::Window>) {
        let Some(persistence) = self.window_state else {
            return;
        };
        let keys = BigWindowStateKeys::for_spec(persistence);

        if persistence.should_restore_geometry
            && let (Some(width), Some(height)) = (
                stored_dimension(self.settings, &keys.width),
                stored_dimension(self.settings, &keys.height),
            )
        {
            window.set_default_size(width, height);
        }
        if persistence.should_restore_maximized && self.settings.bool_or(&keys.maximized, false) {
            window.maximize();
        }
        if persistence.should_restore_fullscreen && self.settings.bool_or(&keys.fullscreen, false) {
            window.fullscreen();
        }

        // Persist on every geometry change (debounced by the store). `default-*`
        // keeps the un-maximized restore size while maximized, so it is saved
        // unconditionally.
        window.connect_default_width_notify({
            let settings = self.settings.clone();
            let key = keys.width.clone();
            move |window| settings.set(&key, Value::from(window.default_width()))
        });
        window.connect_default_height_notify({
            let settings = self.settings.clone();
            let key = keys.height.clone();
            move |window| settings.set(&key, Value::from(window.default_height()))
        });
        window.connect_maximized_notify({
            let settings = self.settings.clone();
            let key = keys.maximized.clone();
            move |window| settings.set(&key, Value::from(window.is_maximized()))
        });
        if persistence.should_restore_fullscreen {
            window.connect_fullscreened_notify({
                let settings = self.settings.clone();
                let key = keys.fullscreen.clone();
                move |window| settings.set(&key, Value::from(window.is_fullscreen()))
            });
        }
        // Flush the debounced geometry before the window (and its store) go away.
        window.connect_close_request({
            let settings = self.settings.clone();
            move |_| {
                if let Err(error) = settings.flush_now() {
                    log::warn!("app shell: window-state flush on close failed: {error}");
                }
                gtk::glib::Propagation::Proceed
            }
        });
    }
}

/// The app's main window type: how AppShell builds and drives it.
///
/// Implementors build their window inside [`build`](BigAppWindow::build) and
/// name its concrete type via [`Window`](BigAppWindow::Window). The type is any
/// `IsA<gtk::Window>` — `adw::ApplicationWindow` for an application-bound
/// window, or a plain `adw::Window` for a window whose Wayland app-id is bound
/// per-surface (the shared file-manager/host-module toplevel). `run` associates
/// whatever is built with `cx.app`, so both participate in single-instance and
/// activation. The default hooks cover the optional behaviors:
/// [`open_paths`](BigAppWindow::open_paths) receives file arguments when the
/// spec sets `handle_files`, and [`on_dark_changed`](BigAppWindow::on_dark_changed)
/// runs when the spec sets `watch_dark` and the system light/dark preference flips.
pub trait BigAppWindow {
    /// The concrete window type this app builds.
    type Window: IsA<gtk::Window>;

    /// Build the window from the shell context.
    fn build(cx: &AppShellContext) -> Self::Window;

    /// Handle a command-line activation (from `HANDLES_COMMAND_LINE`). Each
    /// element is a raw path-or-URI string, so remote URIs (`smb://`, `sftp://`)
    /// survive intact. Called on EVERY command-line activation, including one
    /// with no arguments — a multi-window app can open a fresh tab/window on a
    /// bare reactivation instead of only presenting the existing window. The run
    /// loop presents the active window afterwards. Default: ignore.
    fn open_paths(_file_args: Vec<String>) {}

    /// React to a libadwaita dark-state change. Default: ignore.
    fn on_dark_changed(_is_dark: bool) {}
}

fn bare_action_name(action_id: &str) -> &str {
    action_id
        .split_once('.')
        .map_or(action_id, |(_group, name)| name)
}

fn stored_dimension(settings: &BigSettingsStore, key: &str) -> Option<i32> {
    let value = settings.value(key)?.as_i64()?;
    i32::try_from(value).ok().filter(|dimension| *dimension > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_action_name_strips_group_prefix() {
        assert_eq!(bare_action_name("win.about"), "about");
        assert_eq!(bare_action_name("app.quit"), "quit");
    }

    #[test]
    fn bare_action_name_keeps_ungrouped_id() {
        assert_eq!(bare_action_name("about"), "about");
    }
}
