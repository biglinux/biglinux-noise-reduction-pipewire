// SPDX-License-Identifier: MIT

//! Conversation history store for the AI assistant.
//!
//! Decouples the chat UI from how messages are persisted. The trait is
//! display-free and the in-memory implementation is enough for tests
//! and ephemeral sessions; production wires it to
//! [`big_os_kit::storage::BigVersionedJsonStore`] on disk.

use std::sync::Mutex;

/// One conversation turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAiMessage {
    /// Role of the speaker.
    pub role: BigAiRole,
    /// Plain-text content (markdown allowed).
    pub content: String,
    /// Optional provider id when the turn came from a specific backend.
    pub provider_id: Option<String>,
    /// Unix-seconds timestamp when the turn was created.
    pub timestamp_unix: i64,
}

/// Who sent the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigAiRole {
    /// End user.
    User,
    /// AI assistant.
    Assistant,
    /// System / pre-prompt.
    System,
}

/// One persisted conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAiConversation {
    /// Stable id (uuid-shaped).
    pub id: String,
    /// User-facing label.
    pub title: String,
    /// Turns in chronological order.
    pub messages: Vec<BigAiMessage>,
}

impl BigAiConversation {
    /// Build an empty conversation with the given id and title.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            messages: Vec::new(),
        }
    }
}

/// Trait implemented by backends. Apps swap implementations between
/// in-memory (tests) and file-backed (production).
///
/// # Capabilities
///
/// `ai`, `ai+history`, `persistence`
///
/// # Archetypes
///
/// `terminal`, `editor`, `control-center`
///
/// # Examples
///
/// ```
/// use big_app_kit_core::ai_history::{BigAiHistoryStore, BigInMemoryAiHistory,
///     BigAiConversation, BigAiMessage, BigAiRole};
///
/// let store = BigInMemoryAiHistory::default();
/// let mut conv = BigAiConversation::new("c1", "Refactor auth");
/// conv.messages.push(BigAiMessage {
///     role: BigAiRole::User,
///     content: "Hi".into(),
///     provider_id: Some("anthropic".into()),
///     timestamp_unix: 0,
/// });
/// store.save(&conv).unwrap();
/// assert!(store.load("c1").unwrap().is_some());
/// ```
pub trait BigAiHistoryStore: Send + Sync {
    /// List all conversation ids, newest-first ordering encouraged.
    ///
    /// # Errors
    /// Backend-defined.
    fn list(&self) -> Result<Vec<String>, String>;

    /// Load a conversation by id. Returns `Ok(None)` when missing.
    ///
    /// # Errors
    /// Backend-defined.
    fn load(&self, id: &str) -> Result<Option<BigAiConversation>, String>;

    /// Save (insert or replace) a conversation.
    ///
    /// # Errors
    /// Backend-defined.
    fn save(&self, conversation: &BigAiConversation) -> Result<(), String>;

    /// Delete by id; missing ids are reported as success.
    ///
    /// # Errors
    /// Backend-defined.
    fn delete(&self, id: &str) -> Result<(), String>;
}

/// In-memory implementation. Use in tests and for ephemeral sessions.
#[derive(Debug, Default)]
pub struct BigInMemoryAiHistory {
    inner: Mutex<Vec<BigAiConversation>>,
}

impl BigAiHistoryStore for BigInMemoryAiHistory {
    fn list(&self) -> Result<Vec<String>, String> {
        let guard = self.inner.lock().map_err(|_| "lock poisoned".to_string())?;
        Ok(guard.iter().map(|c| c.id.clone()).collect())
    }

    fn load(&self, id: &str) -> Result<Option<BigAiConversation>, String> {
        let guard = self.inner.lock().map_err(|_| "lock poisoned".to_string())?;
        Ok(guard.iter().find(|c| c.id == id).cloned())
    }

    fn save(&self, conversation: &BigAiConversation) -> Result<(), String> {
        let mut guard = self.inner.lock().map_err(|_| "lock poisoned".to_string())?;
        if let Some(existing) = guard.iter_mut().find(|c| c.id == conversation.id) {
            *existing = conversation.clone();
        } else {
            guard.push(conversation.clone());
        }
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        let mut guard = self.inner.lock().map_err(|_| "lock poisoned".to_string())?;
        guard.retain(|c| c.id != id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> BigInMemoryAiHistory {
        BigInMemoryAiHistory::default()
    }

    fn conv(id: &str) -> BigAiConversation {
        BigAiConversation {
            id: id.into(),
            title: "Test".into(),
            messages: vec![BigAiMessage {
                role: BigAiRole::User,
                content: "hello".into(),
                provider_id: Some("openai".into()),
                timestamp_unix: 1,
            }],
        }
    }

    #[test]
    fn save_and_load_round_trip() {
        let s = store();
        s.save(&conv("c1")).unwrap();
        let loaded = s.load("c1").unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 1);
    }

    #[test]
    fn missing_returns_none() {
        let s = store();
        assert!(s.load("nope").unwrap().is_none());
    }

    #[test]
    fn save_replaces_existing() {
        let s = store();
        s.save(&conv("c1")).unwrap();
        let mut v2 = conv("c1");
        v2.title = "Updated".into();
        s.save(&v2).unwrap();
        assert_eq!(s.load("c1").unwrap().unwrap().title, "Updated");
    }

    #[test]
    fn delete_removes_value_and_is_idempotent() {
        let s = store();
        s.save(&conv("c1")).unwrap();
        s.delete("c1").unwrap();
        assert!(s.load("c1").unwrap().is_none());
        s.delete("c1").unwrap();
    }

    #[test]
    fn list_returns_saved_ids() {
        let s = store();
        s.save(&conv("a")).unwrap();
        s.save(&conv("b")).unwrap();
        let mut ids = s.list().unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a", "b"]);
    }
}
