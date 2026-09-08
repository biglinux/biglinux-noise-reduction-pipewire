// SPDX-License-Identifier: MIT

//! Retry policy.
//!
//! [`BigRetryPolicy`] computes the wait duration between successive
//! attempts of a transient operation (network fetch, DB connect, IPC
//! probe). Supports exponential or linear backoff, an absolute cap,
//! and optional jitter.

use core::time::Duration;

/// Shape of the backoff curve used by [`BigRetryPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigBackoffShape {
    /// Delay grows multiplicatively (`base * factor^attempt`).
    Exponential,
    /// Delay grows additively (`base + factor * attempt`).
    Linear,
}

/// Retry policy used by network/IPC clients to compute backoff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BigRetryPolicy {
    /// Base wait between attempts.
    pub base: Duration,
    /// Cap on the wait duration (after backoff is applied).
    pub max: Duration,
    /// Maximum total attempts (including the first one).
    pub max_attempts: u32,
    /// Backoff shape.
    pub shape: BigBackoffShape,
    /// Multiplier applied per attempt (for exponential) or added per
    /// attempt (for linear). For exponential the typical value is 2.0.
    pub factor: f32,
    /// Jitter ratio in `0.0..=1.0`. `0.25` means ±25 % of the computed
    /// delay. Set to `0.0` to disable.
    pub jitter: f32,
}

impl BigRetryPolicy {
    /// Recommended exponential default: base 200 ms, factor 2.0,
    /// cap 8 s, jitter ±25 %, up to 5 attempts.
    #[must_use]
    pub fn exponential() -> Self {
        Self {
            base: Duration::from_millis(200),
            max: Duration::from_secs(8),
            max_attempts: 5,
            shape: BigBackoffShape::Exponential,
            factor: 2.0,
            jitter: 0.25,
        }
    }

    /// Linear default: base 500 ms, +500 ms per attempt, cap 5 s.
    #[must_use]
    pub fn linear() -> Self {
        Self {
            base: Duration::from_millis(500),
            max: Duration::from_secs(5),
            max_attempts: 5,
            shape: BigBackoffShape::Linear,
            factor: 1.0,
            jitter: 0.0,
        }
    }

    /// Computed delay before attempt `n` (1-indexed). Returns `None`
    /// when `n` is beyond `max_attempts` (caller should give up).
    ///
    /// The jitter component uses a deterministic pseudo-random offset
    /// derived from `n` so identical schedules reproduce in tests.
    #[must_use]
    pub fn delay_before(&self, attempt: u32) -> Option<Duration> {
        if attempt == 0 || attempt > self.max_attempts {
            return None;
        }
        // First attempt is immediate.
        if attempt == 1 {
            return Some(Duration::ZERO);
        }
        let step = attempt - 1; // 0-indexed for the formula below
        let base_ms = self.base.as_millis() as f64;
        let computed_ms = match self.shape {
            BigBackoffShape::Exponential => base_ms * f64::from(self.factor).powi(step as i32),
            BigBackoffShape::Linear => base_ms + base_ms * f64::from(step) * f64::from(self.factor),
        };
        let capped_ms = computed_ms.min(self.max.as_millis() as f64).max(0.0);
        let jittered_ms = apply_deterministic_jitter(capped_ms, self.jitter, attempt);
        Some(Duration::from_millis(jittered_ms.round() as u64))
    }
}

fn apply_deterministic_jitter(value_ms: f64, jitter_ratio: f32, salt: u32) -> f64 {
    if jitter_ratio <= 0.0 {
        return value_ms;
    }
    // Cheap deterministic offset in -1.0..=1.0 from a salted hash.
    let h = salt.wrapping_mul(2_654_435_761) ^ 0x9E37_79B9;
    let normalised = (h % 2001) as f64 / 1000.0 - 1.0;
    value_ms + value_ms * f64::from(jitter_ratio) * normalised
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_attempt_is_immediate() {
        let p = BigRetryPolicy::exponential();
        assert_eq!(p.delay_before(1).unwrap(), Duration::ZERO);
    }

    #[test]
    fn beyond_max_attempts_returns_none() {
        let p = BigRetryPolicy::exponential();
        assert!(p.delay_before(p.max_attempts + 1).is_none());
        assert!(p.delay_before(0).is_none());
    }

    #[test]
    fn exponential_grows_then_caps() {
        let mut p = BigRetryPolicy::exponential();
        p.jitter = 0.0;
        let d2 = p.delay_before(2).unwrap();
        let d3 = p.delay_before(3).unwrap();
        assert!(d2 < d3);
        let d_far = p
            .delay_before(p.max_attempts)
            .unwrap()
            .max(p.delay_before(p.max_attempts - 1).unwrap());
        assert!(d_far <= p.max);
    }

    #[test]
    fn linear_step_uses_factor() {
        let mut p = BigRetryPolicy::linear();
        p.jitter = 0.0;
        let d2 = p.delay_before(2).unwrap();
        let d3 = p.delay_before(3).unwrap();
        // Linear: each step adds `base` (factor=1.0).
        assert_eq!(d3 - d2, p.base);
    }

    #[test]
    fn jitter_is_deterministic_for_same_attempt() {
        let p = BigRetryPolicy::exponential();
        let a = p.delay_before(3).unwrap();
        let b = p.delay_before(3).unwrap();
        assert_eq!(a, b);
    }
}
