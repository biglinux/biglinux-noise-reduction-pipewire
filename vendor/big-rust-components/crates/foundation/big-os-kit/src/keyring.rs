// SPDX-License-Identifier: MIT

//! Keyring secret-store contracts.
//!
//! [`crate::keyring::BigKeyringSecretStore`] is the typed trait every BigLinux app uses
//! when it must persist credentials (SSH keys, API tokens, VPN
//! passwords, AI provider tokens). The trait is display-free and
//! testable; production code uses the system Secret Service via a
//! separate implementation crate, while tests use [`crate::keyring::BigInMemoryKeyring`].
//!
//! # Examples
//!
//! ```
//! use big_os_kit::keyring::{BigInMemoryKeyring, BigKeyringSecretStore};
//!
//! let kr = BigInMemoryKeyring::default();
//! kr.save("github-token", b"ghp_secret").unwrap();
//! assert_eq!(kr.load("github-token").unwrap().as_deref(), Some(&b"ghp_secret"[..]));
//! kr.delete("github-token").unwrap();
//! assert!(kr.load("github-token").unwrap().is_none());
//! ```

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// Failure modes for a keyring backend.
#[derive(Debug)]
pub enum BigKeyringError {
    /// Backend communication failure (D-Bus, IPC, etc.).
    Backend(String),
    /// Caller-supplied label/key was rejected.
    InvalidLabel(String),
    /// Internal lock poisoning (in-memory backend).
    Poisoned,
}

impl std::fmt::Display for BigKeyringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(msg) => write!(f, "keyring backend error: {msg}"),
            Self::InvalidLabel(l) => write!(f, "invalid keyring label: {l}"),
            Self::Poisoned => write!(f, "keyring lock poisoned"),
        }
    }
}

impl std::error::Error for BigKeyringError {}

impl<T> From<PoisonError<T>> for BigKeyringError {
    fn from(_: PoisonError<T>) -> Self {
        Self::Poisoned
    }
}

/// Trait for storing and retrieving small secret blobs under a label.
///
/// Implementations must never log secret contents and should redact
/// secret bytes from error messages.
///
/// # Capabilities
///
/// `keyring`, `secrets`, `credentials`
///
/// # Archetypes
///
/// `terminal`, `network-manager`, `pkg-manager`, `control-center`, `container-manager`
///
/// # Examples
///
/// ```
/// use big_os_kit::keyring::{BigKeyringSecretStore, BigInMemoryKeyring};
///
/// let store = BigInMemoryKeyring::default();
/// store.save("br.com.biglinux.term.api.openai", b"sk-...").unwrap();
/// let loaded = store.load("br.com.biglinux.term.api.openai").unwrap();
/// assert_eq!(loaded, Some(b"sk-...".to_vec()));
/// ```
pub trait BigKeyringSecretStore: Send + Sync {
    /// Write `secret` under `label`, replacing any prior value.
    ///
    /// # Errors
    /// Returns [`BigKeyringError::Backend`] on IPC failure or
    /// [`BigKeyringError::InvalidLabel`] when the label is empty.
    fn save(&self, label: &str, secret: &[u8]) -> Result<(), BigKeyringError>;

    /// Read the value stored under `label`, or `Ok(None)` if missing.
    ///
    /// # Errors
    /// Returns [`BigKeyringError::Backend`] on IPC failure or
    /// [`BigKeyringError::InvalidLabel`] when the label is empty.
    fn load(&self, label: &str) -> Result<Option<Vec<u8>>, BigKeyringError>;

    /// Remove the value stored under `label`. Missing labels are
    /// reported as success.
    ///
    /// # Errors
    /// Returns [`BigKeyringError::Backend`] on IPC failure or
    /// [`BigKeyringError::InvalidLabel`] when the label is empty.
    fn delete(&self, label: &str) -> Result<(), BigKeyringError>;

    /// Enumerate currently stored labels. Order is unspecified.
    ///
    /// # Errors
    /// Returns [`BigKeyringError::Backend`] on IPC failure.
    fn list_labels(&self) -> Result<Vec<String>, BigKeyringError>;
}

fn validate_label(label: &str) -> Result<(), BigKeyringError> {
    if label.is_empty() {
        Err(BigKeyringError::InvalidLabel(label.to_string()))
    } else {
        Ok(())
    }
}

/// In-memory implementation. Use in tests and for ephemeral session
/// storage. **Not** for production secret persistence.
#[derive(Debug, Default)]
pub struct BigInMemoryKeyring {
    store: Mutex<HashMap<String, Vec<u8>>>,
}

impl BigKeyringSecretStore for BigInMemoryKeyring {
    fn save(&self, label: &str, secret: &[u8]) -> Result<(), BigKeyringError> {
        validate_label(label)?;
        self.store
            .lock()?
            .insert(label.to_string(), secret.to_vec());
        Ok(())
    }

    fn load(&self, label: &str) -> Result<Option<Vec<u8>>, BigKeyringError> {
        validate_label(label)?;
        Ok(self.store.lock()?.get(label).cloned())
    }

    fn delete(&self, label: &str) -> Result<(), BigKeyringError> {
        validate_label(label)?;
        self.store.lock()?.remove(label);
        Ok(())
    }

    fn list_labels(&self) -> Result<Vec<String>, BigKeyringError> {
        Ok(self.store.lock()?.keys().cloned().collect())
    }
}

#[cfg(any(
    feature = "libsecret-system-backend",
    feature = "secret-service-backend"
))]
pub use libsecret_backend::{BigLibsecretKeyring, BigSecretServiceKeyring};

/// Production [`BigKeyringSecretStore`] backed by the distro `libsecret-1.so`.
/// Enabled by the `libsecret-system-backend` feature; requires a running
/// Secret Service daemon (gnome-keyring, KWallet's Secret Service,
/// KeePassXC, ...).
#[cfg(any(
    feature = "libsecret-system-backend",
    feature = "secret-service-backend"
))]
mod libsecret_backend {
    use std::collections::HashMap;

    use glib::translate::from_glib_full;
    use libsecret::prelude::RetrievableExtManual;

    use super::{BigKeyringError, BigKeyringSecretStore, validate_label};

    /// Default `xdg:schema` for greenfield [`BigLibsecretKeyring::new`]
    /// keyrings, so a label search never collides with secrets from other
    /// schemas.
    const SCHEMA: &str = "br.com.biglinux.app-kit.Secret";
    /// Default attribute carrying the caller's logical label.
    const LABEL_ATTR: &str = "big_label";
    const CONTENT_TYPE: &[u8] = b"application/octet-stream\0";
    const DEFAULT_COLLECTION: &str = "default";

    /// System libsecret keyring.
    ///
    /// Two modes:
    /// - [`Self::new`]: greenfield apps. Items are tagged
    ///   `{xdg:schema = <shared>, app_id = <app>, big_label = <label>}` so two
    ///   apps sharing the login keyring never clash on the same label.
    /// - [`Self::with_schema`]: interop. Items are tagged
    ///   `{xdg:schema = <schema>, <attribute> = <label>}` with **no** `app_id`,
    ///   matching how another client (e.g. a libsecret app) stored them — so
    ///   the same secrets are read/written transparently.
    pub struct BigLibsecretKeyring {
        schema: String,
        attribute: String,
        app_id: Option<String>,
    }

    /// Compatibility alias for older consumers that still use the Secret
    /// Service name. The implementation is libsecret-backed.
    pub type BigSecretServiceKeyring = BigLibsecretKeyring;

    impl BigLibsecretKeyring {
        /// Greenfield keyring namespaced by `app_id` (typically the desktop
        /// app-id, e.g. `br.com.biglinux.big-terminal`) under the shared schema.
        #[must_use]
        pub fn new(app_id: impl Into<String>) -> Self {
            Self {
                schema: SCHEMA.to_owned(),
                attribute: LABEL_ATTR.to_owned(),
                app_id: Some(app_id.into()),
            }
        }

        /// Interop keyring matching items written by another Secret Service
        /// client: each item is keyed by `{xdg:schema = schema, attribute =
        /// label}` with no `app_id` stamp. Use this to read/write secrets that
        /// a different app (or a previous version) created with its own schema
        /// and a single lookup attribute (e.g. libsecret's
        /// `"org.example.App.Password"` + `"account"`).
        #[must_use]
        pub fn with_schema(schema: impl Into<String>, attribute: impl Into<String>) -> Self {
            Self {
                schema: schema.into(),
                attribute: attribute.into(),
                app_id: None,
            }
        }

        fn schema(&self) -> libsecret::Schema {
            let mut attrs = HashMap::new();
            if self.app_id.is_some() {
                attrs.insert("app_id", libsecret::SchemaAttributeType::String);
            }
            attrs.insert(
                self.attribute.as_str(),
                libsecret::SchemaAttributeType::String,
            );
            libsecret::Schema::new(&self.schema, libsecret::SchemaFlags::NONE, attrs)
        }

        fn secret_record_attributes<'a>(&'a self, label: &'a str) -> HashMap<&'a str, &'a str> {
            let mut attributes = self.app_attrs();
            attributes.insert(self.attribute.as_str(), label);
            attributes
        }

        fn app_attrs(&self) -> HashMap<&str, &str> {
            let mut attrs = HashMap::new();
            if let Some(app_id) = &self.app_id {
                attrs.insert("app_id", app_id.as_str());
            }
            attrs
        }

        fn value_from_secret(secret: &[u8]) -> Result<libsecret::Value, BigKeyringError> {
            let secret_len = isize::try_from(secret.len())
                .map_err(|_| BigKeyringError::Backend("secret too large".to_owned()))?;
            let value = unsafe {
                // SAFETY: `secret_value_new` copies exactly `secret_len` bytes
                // from `secret`, so embedded NUL bytes are data, not
                // terminators. `CONTENT_TYPE` is a static NUL-terminated C
                // string. The returned full reference is wrapped immediately.
                libsecret::ffi::secret_value_new(
                    secret.as_ptr().cast(),
                    secret_len,
                    CONTENT_TYPE.as_ptr().cast(),
                )
            };
            if value.is_null() {
                Err(BigKeyringError::Backend(
                    "libsecret returned null SecretValue".to_owned(),
                ))
            } else {
                Ok(unsafe {
                    // SAFETY: `secret_value_new` returns a full SecretValue
                    // reference on success, transferred to the wrapper.
                    from_glib_full(value)
                })
            }
        }
    }

    impl BigKeyringSecretStore for BigLibsecretKeyring {
        fn save(&self, label: &str, secret: &[u8]) -> Result<(), BigKeyringError> {
            validate_label(label)?;
            let schema = self.schema();
            let value = Self::value_from_secret(secret)?;
            libsecret::password_store_binary_sync(
                Some(&schema),
                self.secret_record_attributes(label),
                Some(DEFAULT_COLLECTION),
                label,
                &value,
                None::<&gio::Cancellable>,
            )
            .map_err(|e| BigKeyringError::Backend(format!("libsecret store: {e}")))?;
            Ok(())
        }

        fn load(&self, label: &str) -> Result<Option<Vec<u8>>, BigKeyringError> {
            validate_label(label)?;
            let schema = self.schema();
            let secret = libsecret::password_lookup_binary_sync(
                Some(&schema),
                self.secret_record_attributes(label),
                None::<&gio::Cancellable>,
            )
            .map_err(|e| BigKeyringError::Backend(format!("libsecret lookup: {e}")))?;
            Ok(secret.map(|value| value.get()))
        }

        fn delete(&self, label: &str) -> Result<(), BigKeyringError> {
            validate_label(label)?;
            let schema = self.schema();
            libsecret::password_clear_sync(
                Some(&schema),
                self.secret_record_attributes(label),
                None::<&gio::Cancellable>,
            )
            .map_err(|e| BigKeyringError::Backend(format!("libsecret clear: {e}")))?;
            Ok(())
        }

        fn list_labels(&self) -> Result<Vec<String>, BigKeyringError> {
            let schema = self.schema();
            let items = libsecret::password_search_sync(
                Some(&schema),
                self.app_attrs(),
                libsecret::SearchFlags::ALL,
                None::<&gio::Cancellable>,
            )
            .map_err(|e| BigKeyringError::Backend(format!("libsecret search: {e}")))?;
            let mut labels = Vec::new();
            for item in items {
                if let Some(label) = item.attributes().get(self.attribute.as_str()) {
                    labels.push(label.clone());
                }
            }
            Ok(labels)
        }
    }
}

#[cfg(all(
    test,
    any(
        feature = "libsecret-system-backend",
        feature = "secret-service-backend"
    )
))]
mod libsecret_backend_tests {
    use super::{BigLibsecretKeyring, BigSecretServiceKeyring};

    // Behaviour tests need a live Secret Service daemon, so they are not run
    // in the headless gate; this only checks the backend constructs and the
    // feature compiles against the libsecret API surface.
    #[test]
    fn constructs_with_app_id() {
        let _kr = BigLibsecretKeyring::new("br.com.biglinux.test");
    }

    #[test]
    fn constructs_with_interop_schema() {
        let _kr = BigSecretServiceKeyring::with_schema("org.example.App.Password", "account");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kr() -> BigInMemoryKeyring {
        BigInMemoryKeyring::default()
    }

    #[test]
    fn save_and_load_round_trip() {
        let k = kr();
        k.save("token", b"abc").unwrap();
        assert_eq!(k.load("token").unwrap().as_deref(), Some(&b"abc"[..]));
    }

    #[test]
    fn missing_label_returns_none() {
        let k = kr();
        assert!(k.load("nope").unwrap().is_none());
    }

    #[test]
    fn save_replaces_prior_value() {
        let k = kr();
        k.save("token", b"v1").unwrap();
        k.save("token", b"v2").unwrap();
        assert_eq!(k.load("token").unwrap().as_deref(), Some(&b"v2"[..]));
    }

    #[test]
    fn delete_removes_value_and_is_idempotent() {
        let k = kr();
        k.save("token", b"abc").unwrap();
        k.delete("token").unwrap();
        assert!(k.load("token").unwrap().is_none());
        k.delete("token").unwrap(); // idempotent
    }

    #[test]
    fn empty_label_rejected() {
        let k = kr();
        let err = k.save("", b"x").unwrap_err();
        assert!(matches!(err, BigKeyringError::InvalidLabel(_)));
    }

    #[test]
    fn list_labels_returns_all_keys() {
        let k = kr();
        k.save("a", b"x").unwrap();
        k.save("b", b"y").unwrap();
        let mut labels = k.list_labels().unwrap();
        labels.sort();
        assert_eq!(labels, vec!["a".to_string(), "b".to_string()]);
    }
}
