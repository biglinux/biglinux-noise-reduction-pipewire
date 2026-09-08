// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Application and window action contracts (display-free specs).
//!
//! The GIO installers (`install_action`, `install_static_accels`, …) that turn
//! these specs into live `gio::SimpleAction`s live in `big_app_kit::actions`.

/// GIO action prefix selecting which action map a [`BigActionSpec`]
/// installs into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigActionScope {
    /// Installs on `gio::Application`; activated as `app.<name>`.
    App,
    /// Installs on a `gtk::Window`; activated as `win.<name>`.
    Window,
}

/// Declarative description of a `GAction` the app wants to expose.
///
/// The spec is the inert source of truth; adapters use it to install a
/// `gio::SimpleAction` (see `install_action`) and bind it to menu
/// items, buttons, and shortcuts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigActionSpec {
    /// Action name without scope prefix (`"open"`, `"toggle-fullscreen"`).
    pub name: String,
    /// Localized label shown in menu rows and shortcut dialogs.
    pub label: String,
    /// Whether the action belongs to the application or the window.
    pub scope: BigActionScope,
    /// Optional GTK accelerator string (`"<Primary>O"`) that should
    /// trigger the action.
    pub shortcut: Option<String>,
    /// Optional tooltip shown on toolbar buttons bound to the action.
    pub tooltip: Option<String>,
}

impl BigActionSpec {
    /// Build a spec with no shortcut and no tooltip. Use
    /// [`BigActionSpec::shortcut`] and [`BigActionSpec::tooltip`] to add
    /// them.
    #[must_use]
    pub fn new(name: impl Into<String>, label: impl Into<String>, scope: BigActionScope) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            scope,
            shortcut: None,
            tooltip: None,
        }
    }

    /// Attach a GTK accelerator string. The shell layer feeds this into
    /// `install_static_accels`.
    #[must_use]
    pub fn shortcut(mut self, value: impl Into<String>) -> Self {
        self.shortcut = Some(value.into());
        self
    }

    /// Attach a localized tooltip used on toolbar buttons bound to the
    /// action.
    #[must_use]
    pub fn tooltip(mut self, value: impl Into<String>) -> Self {
        self.tooltip = Some(value.into());
        self
    }

    /// Compose the scoped action name (`"app.open"`, `"win.quit"`) used
    /// when binding the action to menu items and accelerators.
    #[must_use]
    pub fn detailed_name(&self) -> String {
        match self.scope {
            BigActionScope::App => format!("app.{}", self.name),
            BigActionScope::Window => format!("win.{}", self.name),
        }
    }
}

/// Ordered collection of [`BigActionSpec`] used as the source of truth
/// when building menus, accelerator tables, and the shortcuts dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BigActionRegistry {
    /// Action specs in declaration order.
    pub actions: Vec<BigActionSpec>,
}

impl BigActionRegistry {
    /// Build an empty registry; chain [`BigActionRegistry::action`] for
    /// each spec.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `action` to the registry and return it.
    #[must_use]
    pub fn action(mut self, action: BigActionSpec) -> Self {
        self.actions.push(action);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{BigActionRegistry, BigActionScope, BigActionSpec};

    #[test]
    fn action_scope_resolves_detailed_name_from_canonical_module() {
        assert_eq!(
            BigActionSpec::new("open", "Open", BigActionScope::Window).detailed_name(),
            "win.open"
        );
    }

    #[test]
    fn action_registry_appends_specs_in_declaration_order_from_canonical_module() {
        let registry = BigActionRegistry::new()
            .action(BigActionSpec::new("open", "Open", BigActionScope::App))
            .action(BigActionSpec::new("close", "Close", BigActionScope::Window));

        assert_eq!(
            registry
                .actions
                .iter()
                .map(BigActionSpec::detailed_name)
                .collect::<Vec<_>>(),
            vec!["app.open", "win.close"]
        );
    }
}
