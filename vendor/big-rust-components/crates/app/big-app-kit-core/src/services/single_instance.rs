// SPDX-License-Identifier: MIT

//! Single-instance app contract.
//!
//! Most desktop apps must elect a primary process via D-Bus
//! `RequestName` and forward argv from secondary instances to it.
//! This module captures the policy (app id, behaviour on conflict).

/// Policy applied to a non-primary process that loses the D-Bus
/// `RequestName` race.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigSecondaryBehavior {
    /// Forward CLI args (positional file paths) to the primary and exit.
    ForwardArgv,
    /// Raise the primary window without forwarding anything.
    RaiseOnly,
    /// Allow multiple instances (opt-out of single-instance).
    AllowMultiple,
}

/// Display-free specification describing big single instance behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSingleInstanceSpec {
    /// FreeDesktop app id (`br.com.biglinux.MyApp`).
    pub app_id: String,
    /// Behaviour when this process is not the primary.
    pub on_secondary: BigSecondaryBehavior,
    /// `true` to register a `.open(uris)` D-Bus method (GLib pattern).
    pub register_open_method: bool,
    /// `true` to register an `.activate()` D-Bus method.
    pub register_activate_method: bool,
}

impl BigSingleInstanceSpec {
    /// Construct a [`BigSingleInstanceSpec`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            on_secondary: BigSecondaryBehavior::ForwardArgv,
            register_open_method: true,
            register_activate_method: true,
        }
    }

    /// Configure the `raise_only` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSingleInstanceSpec`].
    #[must_use]
    pub fn raise_only(mut self) -> Self {
        self.on_secondary = BigSecondaryBehavior::RaiseOnly;
        self
    }

    /// Configure the `allow_multiple` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSingleInstanceSpec`].
    #[must_use]
    pub fn allow_multiple(mut self) -> Self {
        self.on_secondary = BigSecondaryBehavior::AllowMultiple;
        self
    }

    /// Validate: reverse-DNS-ish app id (at least two segments).
    ///
    /// # Errors
    /// Returns a description string when validation fails.
    pub fn validate(&self) -> Result<(), String> {
        if self.app_id.is_empty() {
            return Err("app_id is empty".into());
        }
        if !self.app_id.contains('.') {
            return Err(format!(
                "app_id {} is not reverse-DNS (needs at least one dot)",
                self.app_id
            ));
        }
        Ok(())
    }
}

/// Trait implementations talk to the session bus.
pub trait BigSingleInstanceBackend {
    /// Try to become the primary instance. Returns `Ok(true)` when this
    /// process owns the well-known name, `Ok(false)` otherwise.
    ///
    /// # Errors
    /// Backend-defined.
    fn try_become_primary(&self, spec: &BigSingleInstanceSpec) -> Result<bool, String>;

    /// Forward argv to the existing primary instance.
    ///
    /// # Errors
    /// Backend-defined.
    fn forward(&self, spec: &BigSingleInstanceSpec, argv: &[String]) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_forwards_argv_with_open_and_activate() {
        let s = BigSingleInstanceSpec::new("br.com.biglinux.MyApp");
        assert_eq!(s.on_secondary, BigSecondaryBehavior::ForwardArgv);
        assert!(s.register_open_method);
        assert!(s.register_activate_method);
    }

    #[test]
    fn raise_only_disables_argv_forwarding_intent() {
        let s = BigSingleInstanceSpec::new("br.com.biglinux.MyApp").raise_only();
        assert_eq!(s.on_secondary, BigSecondaryBehavior::RaiseOnly);
    }

    #[test]
    fn allow_multiple_opts_out() {
        let s = BigSingleInstanceSpec::new("br.com.biglinux.MyApp").allow_multiple();
        assert_eq!(s.on_secondary, BigSecondaryBehavior::AllowMultiple);
    }

    #[test]
    fn validate_requires_reverse_dns() {
        assert!(BigSingleInstanceSpec::new("myapp").validate().is_err());
        assert!(BigSingleInstanceSpec::new("").validate().is_err());
        BigSingleInstanceSpec::new("br.com.biglinux.MyApp")
            .validate()
            .unwrap();
    }
}
