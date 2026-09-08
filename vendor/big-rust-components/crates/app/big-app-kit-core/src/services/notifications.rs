// SPDX-License-Identifier: MIT

//! FreeDesktop notification contracts with batching policy.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Maps onto the FreeDesktop notification urgency hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigNotificationUrgency {
    /// Background information; safe to coalesce or hide.
    Low,
    /// Default urgency for regular notifications.
    Normal,
    /// User must act; bypasses do-not-disturb.
    Critical,
}

/// One action button attached to a [`BigNotificationSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigNotificationAction {
    /// Action id passed back to the handler.
    pub id: String,
    /// Visible button label.
    pub label: String,
}

/// Display-free specification describing big notification behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigNotificationSpec {
    /// Human-readable title shown in UI.
    pub title: String,
    /// Secondary text shown under the title.
    pub body: String,
    /// Optional themed icon name shown by the notification daemon.
    pub icon_name: Option<String>,
    /// Urgency hint forwarded to the daemon.
    pub urgency: BigNotificationUrgency,
    /// Ordered list of actions entries.
    pub actions: Vec<BigNotificationAction>,
    /// Dedupe key. Identical keys collapse onto the same toast.
    pub idempotency_key: Option<String>,
    /// `Some(d)` enables transient notifications that auto-dismiss
    /// after `d`. `None` is persistent.
    pub timeout: Option<Duration>,
}

impl BigNotificationSpec {
    /// Build a notification spec with normal urgency, no icon, no
    /// actions, no idempotency, persistent timeout.
    #[must_use]
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            icon_name: None,
            urgency: BigNotificationUrgency::Normal,
            actions: Vec::new(),
            idempotency_key: None,
            timeout: None,
        }
    }

    /// Builder method returning `Self` with urgency set.
    #[must_use]
    pub fn with_urgency(mut self, urgency: BigNotificationUrgency) -> Self {
        self.urgency = urgency;
        self
    }

    /// Builder method returning `Self` with icon set.
    #[must_use]
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon_name = Some(icon.into());
        self
    }

    /// Builder method returning `Self` with action set.
    #[must_use]
    pub fn with_action(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.actions.push(BigNotificationAction {
            id: id.into(),
            label: label.into(),
        });
        self
    }

    /// Builder method returning `Self` with idempotency key set.
    #[must_use]
    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Builder method returning `Self` with timeout set.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// Trait implementations talk to the system bus.
pub trait BigNotificationBackend {
    /// Send (or coalesce) a notification.
    ///
    /// # Errors
    /// Backend-defined.
    fn send(&self, spec: &BigNotificationSpec) -> Result<(), String>;
}

/// Batching policy used by the in-process gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigNotificationBatchPolicy {
    /// Send every notification immediately.
    None,
    /// Drop notifications when more than `max_per_window` arrive within
    /// `window`. Useful for spam control on noisy event streams.
    RateLimit {
        /// Maximum notifications to deliver per window before dropping.
        max_per_window: u32,
    },
}

/// In-memory dedupe + rate-limit gate. Caller layers this between
/// producers and a real backend.
#[derive(Debug)]
pub struct BigNotificationGate {
    policy: BigNotificationBatchPolicy,
    window: Duration,
    seen: HashMap<String, Instant>,
    recent: Vec<Instant>,
}

impl BigNotificationGate {
    /// Build a gate. `window` controls both dedupe TTL and rate-limit
    /// horizon.
    #[must_use]
    pub fn new(policy: BigNotificationBatchPolicy, window: Duration) -> Self {
        Self {
            policy,
            window,
            seen: HashMap::new(),
            recent: Vec::new(),
        }
    }

    /// Returns true when the notification should reach the backend.
    pub fn should_emit(&mut self, spec: &BigNotificationSpec) -> bool {
        self.should_emit_at(spec, Instant::now())
    }

    /// Internal variant that takes the timestamp from the caller for
    /// deterministic tests.
    pub fn should_emit_at(&mut self, spec: &BigNotificationSpec, now: Instant) -> bool {
        // Dedupe by idempotency key.
        if let Some(key) = &spec.idempotency_key {
            if let Some(last) = self.seen.get(key)
                && now.duration_since(*last) < self.window
            {
                return false;
            }
            self.seen.insert(key.clone(), now);
        }
        // Rate-limit window.
        match self.policy {
            BigNotificationBatchPolicy::None => true,
            BigNotificationBatchPolicy::RateLimit { max_per_window } => {
                self.recent.retain(|t| now.duration_since(*t) < self.window);
                if self.recent.len() as u32 >= max_per_window {
                    return false;
                }
                self.recent.push(now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification_with_key(key: &str) -> BigNotificationSpec {
        BigNotificationSpec::new("t", "b").with_idempotency_key(key)
    }

    #[test]
    fn dedupe_blocks_repeat_within_window() {
        let mut g =
            BigNotificationGate::new(BigNotificationBatchPolicy::None, Duration::from_secs(60));
        let now = Instant::now();
        assert!(g.should_emit_at(&notification_with_key("k"), now));
        assert!(!g.should_emit_at(&notification_with_key("k"), now));
    }

    #[test]
    fn dedupe_clears_after_window() {
        let mut g =
            BigNotificationGate::new(BigNotificationBatchPolicy::None, Duration::from_millis(10));
        let t0 = Instant::now();
        assert!(g.should_emit_at(&notification_with_key("k"), t0));
        let t1 = t0 + Duration::from_millis(100);
        assert!(g.should_emit_at(&notification_with_key("k"), t1));
    }

    #[test]
    fn dedupe_clears_at_exact_window_boundary() {
        let mut g =
            BigNotificationGate::new(BigNotificationBatchPolicy::None, Duration::from_millis(250));
        let t0 = Instant::now();
        assert!(g.should_emit_at(&notification_with_key("k"), t0));
        let t1 = t0 + Duration::from_millis(250);
        assert!(g.should_emit_at(&notification_with_key("k"), t1));
    }

    #[test]
    fn should_emit_uses_current_time_and_stored_state() {
        let mut g =
            BigNotificationGate::new(BigNotificationBatchPolicy::None, Duration::from_secs(60));
        assert!(g.should_emit(&notification_with_key("k")));
        assert!(!g.should_emit(&notification_with_key("k")));
    }

    #[test]
    fn rate_limit_drops_overflow() {
        let mut g = BigNotificationGate::new(
            BigNotificationBatchPolicy::RateLimit { max_per_window: 2 },
            Duration::from_secs(60),
        );
        let t = Instant::now();
        assert!(g.should_emit_at(&BigNotificationSpec::new("a", "1"), t));
        assert!(g.should_emit_at(&BigNotificationSpec::new("a", "2"), t));
        assert!(!g.should_emit_at(&BigNotificationSpec::new("a", "3"), t));
    }

    #[test]
    fn rate_limit_window_recovers() {
        let mut g = BigNotificationGate::new(
            BigNotificationBatchPolicy::RateLimit { max_per_window: 1 },
            Duration::from_millis(10),
        );
        let t0 = Instant::now();
        assert!(g.should_emit_at(&BigNotificationSpec::new("a", "1"), t0));
        let t1 = t0 + Duration::from_millis(100);
        assert!(g.should_emit_at(&BigNotificationSpec::new("a", "2"), t1));
    }

    #[test]
    fn rate_limit_recovers_at_exact_window_boundary() {
        let mut g = BigNotificationGate::new(
            BigNotificationBatchPolicy::RateLimit { max_per_window: 1 },
            Duration::from_millis(250),
        );
        let t0 = Instant::now();
        assert!(g.should_emit_at(&BigNotificationSpec::new("a", "1"), t0));
        let t1 = t0 + Duration::from_millis(250);
        assert!(g.should_emit_at(&BigNotificationSpec::new("a", "2"), t1));
    }

    #[test]
    fn spec_carries_actions_and_urgency() {
        let spec = BigNotificationSpec::new("t", "b")
            .with_urgency(BigNotificationUrgency::Critical)
            .with_action("retry", "Try again");
        assert_eq!(spec.urgency, BigNotificationUrgency::Critical);
        assert_eq!(spec.actions.len(), 1);
        assert_eq!(spec.actions[0].id, "retry");
    }
}
