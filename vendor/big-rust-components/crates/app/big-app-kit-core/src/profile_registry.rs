// SPDX-License-Identifier: MIT

//! Generic profile registry.
//!
//! Terminal color schemes, editor themes, audio EQ presets, browser
//! profiles, and similar named-configuration sets all follow the same
//! shape: a list of built-in profiles (immutable), a list of user
//! profiles (mutable), and a currently active profile id.
//!
//! [`crate::profile_registry::BigProfileRegistry`] captures that shape generically. Concrete
//! profile payloads live in the app/runtime crate.
//!
//! # Examples
//!
//! ```
//! use big_app_kit_core::profile_registry::{BigProfileRegistry, BigProfileEntry};
//!
//! #[derive(Clone, PartialEq, Debug)]
//! struct ColorScheme { fg: String }
//!
//! let mut reg: BigProfileRegistry<ColorScheme> = BigProfileRegistry::new(
//!     vec![
//!         BigProfileEntry::new("solarized-dark", ColorScheme { fg: "#fdf6e3".into() }),
//!         BigProfileEntry::new("nord",          ColorScheme { fg: "#eceff4".into() }),
//!     ],
//!     "solarized-dark",
//! ).unwrap();
//!
//! assert_eq!(reg.active().id, "solarized-dark");
//! reg.save_custom("my-warm", ColorScheme { fg: "#ffd9a0".into() }).unwrap();
//! reg.set_active("my-warm").unwrap();
//! assert_eq!(reg.active().payload.fg, "#ffd9a0");
//! ```

use std::collections::HashSet;

/// One profile entry: stable id + payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigProfileEntry<T> {
    /// Stable identifier (lowercase, kebab-case recommended).
    pub id: String,
    /// Payload type owned by the app.
    pub payload: T,
}

impl<T> BigProfileEntry<T> {
    /// Construct a [`BigProfileEntry`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn new(id: impl Into<String>, payload: T) -> Self {
        Self {
            id: id.into(),
            payload,
        }
    }
}

/// Failure modes when mutating a registry.
#[derive(Debug, PartialEq, Eq)]
pub enum BigProfileError {
    /// No registry can be empty.
    NoBuiltIns,
    /// `set_active` referenced a missing id.
    UnknownActive(String),
    /// `save_custom` clashed with a built-in id.
    BuiltInIdClash(String),
    /// Empty id supplied.
    InvalidId,
}

impl std::fmt::Display for BigProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBuiltIns => write!(f, "profile registry needs at least one built-in"),
            Self::UnknownActive(id) => write!(f, "unknown profile id: {id}"),
            Self::BuiltInIdClash(id) => write!(f, "id clashes with a built-in profile: {id}"),
            Self::InvalidId => write!(f, "profile id is empty"),
        }
    }
}

impl std::error::Error for BigProfileError {}

/// Profile registry: named configurations with built-in vs custom isolation and
/// a single active selection. Generic over the payload `T`.
///
/// # Capabilities
///
/// `profiles`, `profiles+activation`
///
/// # Archetypes
///
/// `terminal`, `editor`, `media-player`, `control-center`
#[derive(Debug, Clone)]
pub struct BigProfileRegistry<T> {
    built_in: Vec<BigProfileEntry<T>>,
    custom: Vec<BigProfileEntry<T>>,
    active: String,
}

impl<T: Clone> BigProfileRegistry<T> {
    /// Build a registry with `built_in` and the initial `active` id.
    ///
    /// # Errors
    /// Returns [`BigProfileError::NoBuiltIns`] when `built_in` is empty
    /// or [`BigProfileError::UnknownActive`] when `active` does not
    /// match any built-in id.
    pub fn new(
        built_in: Vec<BigProfileEntry<T>>,
        active: impl Into<String>,
    ) -> Result<Self, BigProfileError> {
        if built_in.is_empty() {
            return Err(BigProfileError::NoBuiltIns);
        }
        let active = active.into();
        if !built_in.iter().any(|e| e.id == active) {
            return Err(BigProfileError::UnknownActive(active));
        }
        Ok(Self {
            built_in,
            custom: Vec::new(),
            active,
        })
    }

    /// Built-in profiles (immutable).
    #[must_use]
    pub fn built_in(&self) -> &[BigProfileEntry<T>] {
        &self.built_in
    }

    /// User-saved profiles.
    #[must_use]
    pub fn custom(&self) -> &[BigProfileEntry<T>] {
        &self.custom
    }

    /// Currently active profile.
    #[must_use]
    pub fn active(&self) -> &BigProfileEntry<T> {
        self.lookup(&self.active)
            .expect("active id always points at a real entry")
    }

    fn lookup(&self, id: &str) -> Option<&BigProfileEntry<T>> {
        self.built_in
            .iter()
            .chain(self.custom.iter())
            .find(|e| e.id == id)
    }

    /// Switch active profile.
    ///
    /// # Errors
    /// Returns [`BigProfileError::UnknownActive`] when `id` is missing.
    pub fn set_active(&mut self, id: &str) -> Result<(), BigProfileError> {
        if self.lookup(id).is_none() {
            return Err(BigProfileError::UnknownActive(id.to_string()));
        }
        self.active = id.to_string();
        Ok(())
    }

    /// Insert or replace a user profile. Returns the previous payload
    /// when the id already existed in the custom list.
    ///
    /// # Errors
    /// Returns [`BigProfileError::InvalidId`] on empty id or
    /// [`BigProfileError::BuiltInIdClash`] when `id` is a built-in id.
    pub fn save_custom(
        &mut self,
        id: impl Into<String>,
        payload: T,
    ) -> Result<Option<T>, BigProfileError> {
        let id = id.into();
        if id.is_empty() {
            return Err(BigProfileError::InvalidId);
        }
        if self.built_in.iter().any(|e| e.id == id) {
            return Err(BigProfileError::BuiltInIdClash(id));
        }
        let mut prev = None;
        if let Some(existing) = self.custom.iter_mut().find(|e| e.id == id) {
            prev = Some(existing.payload.clone());
            existing.payload = payload;
        } else {
            self.custom.push(BigProfileEntry { id, payload });
        }
        Ok(prev)
    }

    /// Remove a custom profile. If the active profile is deleted, the
    /// active id falls back to the first built-in.
    ///
    /// # Errors
    /// Returns [`BigProfileError::UnknownActive`] when `id` is not a
    /// custom profile.
    pub fn delete_custom(&mut self, id: &str) -> Result<T, BigProfileError> {
        let Some(idx) = self.custom.iter().position(|e| e.id == id) else {
            return Err(BigProfileError::UnknownActive(id.to_string()));
        };
        let removed = self.custom.remove(idx);
        if self.active == id {
            self.active = self.built_in[0].id.clone();
        }
        Ok(removed.payload)
    }

    /// All ids in deterministic order: built-in first, then custom.
    #[must_use]
    pub fn ids(&self) -> Vec<String> {
        self.built_in
            .iter()
            .chain(self.custom.iter())
            .map(|e| e.id.clone())
            .collect()
    }

    /// Active id (cheap, no payload clone).
    #[must_use]
    pub fn active_id(&self) -> &str {
        &self.active
    }

    /// Audit invariant for tests/CI: no duplicate ids across built_in + custom.
    #[must_use]
    pub fn has_unique_ids(&self) -> bool {
        let mut seen = HashSet::new();
        for e in self.built_in.iter().chain(self.custom.iter()) {
            if !seen.insert(e.id.clone()) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Scheme(&'static str);

    fn registry() -> BigProfileRegistry<Scheme> {
        BigProfileRegistry::new(
            vec![
                BigProfileEntry::new("a", Scheme("a-payload")),
                BigProfileEntry::new("b", Scheme("b-payload")),
            ],
            "a",
        )
        .unwrap()
    }

    #[test]
    fn empty_builtins_rejected() {
        let err = BigProfileRegistry::<Scheme>::new(vec![], "a").unwrap_err();
        assert_eq!(err, BigProfileError::NoBuiltIns);
    }

    #[test]
    fn unknown_active_rejected() {
        let err =
            BigProfileRegistry::new(vec![BigProfileEntry::new("only", Scheme("x"))], "missing")
                .unwrap_err();
        assert!(matches!(err, BigProfileError::UnknownActive(_)));
    }

    #[test]
    fn active_starts_at_constructor_id() {
        let reg = registry();
        assert_eq!(reg.active_id(), "a");
        assert_eq!(reg.active().payload, Scheme("a-payload"));
    }

    #[test]
    fn built_in_returns_constructor_profiles_in_order() {
        let reg = registry();

        assert_eq!(
            reg.built_in(),
            &[
                BigProfileEntry::new("a", Scheme("a-payload")),
                BigProfileEntry::new("b", Scheme("b-payload")),
            ]
        );
    }

    #[test]
    fn profile_errors_display_actionable_messages() {
        assert_eq!(
            BigProfileError::NoBuiltIns.to_string(),
            "profile registry needs at least one built-in"
        );
        assert_eq!(
            BigProfileError::UnknownActive("missing".into()).to_string(),
            "unknown profile id: missing"
        );
        assert_eq!(
            BigProfileError::BuiltInIdClash("a".into()).to_string(),
            "id clashes with a built-in profile: a"
        );
        assert_eq!(
            BigProfileError::InvalidId.to_string(),
            "profile id is empty"
        );
    }

    #[test]
    fn save_custom_appends_and_replaces() {
        let mut reg = registry();
        let prev = reg.save_custom("warm", Scheme("warm-1")).unwrap();
        assert!(prev.is_none());
        let prev = reg.save_custom("warm", Scheme("warm-2")).unwrap();
        assert_eq!(prev, Some(Scheme("warm-1")));
        assert_eq!(reg.custom().len(), 1);
    }

    #[test]
    fn save_custom_rejects_builtin_id() {
        let mut reg = registry();
        let err = reg.save_custom("a", Scheme("x")).unwrap_err();
        assert!(matches!(err, BigProfileError::BuiltInIdClash(_)));
    }

    #[test]
    fn save_custom_rejects_empty_id() {
        let mut reg = registry();
        let err = reg.save_custom("", Scheme("x")).unwrap_err();
        assert_eq!(err, BigProfileError::InvalidId);
    }

    #[test]
    fn set_active_to_custom_works() {
        let mut reg = registry();
        reg.save_custom("warm", Scheme("warm")).unwrap();
        reg.set_active("warm").unwrap();
        assert_eq!(reg.active_id(), "warm");
    }

    #[test]
    fn delete_active_custom_falls_back_to_first_builtin() {
        let mut reg = registry();
        reg.save_custom("warm", Scheme("warm")).unwrap();
        reg.set_active("warm").unwrap();
        reg.delete_custom("warm").unwrap();
        assert_eq!(reg.active_id(), "a");
    }

    #[test]
    fn ids_lists_builtins_then_custom() {
        let mut reg = registry();
        reg.save_custom("z-custom", Scheme("z")).unwrap();
        assert_eq!(reg.ids(), vec!["a", "b", "z-custom"]);
    }

    #[test]
    fn invariant_unique_ids_holds() {
        let mut reg = registry();
        reg.save_custom("warm", Scheme("warm")).unwrap();
        assert!(reg.has_unique_ids());
    }

    #[test]
    fn invariant_rejects_duplicate_builtin_and_custom_ids() {
        let reg = BigProfileRegistry {
            built_in: vec![BigProfileEntry::new("a", Scheme("builtin"))],
            custom: vec![BigProfileEntry::new("a", Scheme("custom"))],
            active: "a".into(),
        };

        assert!(!reg.has_unique_ids());
    }
}
