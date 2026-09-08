// SPDX-License-Identifier: MIT

//! Loading-state spec.
//!
//! Per HIG: do not flash a spinner when work completes quickly.
//! [`BigLoadingSpinnerSpec`] declares a non-zero delay before the
//! spinner becomes visible. Consumers wire the delay into a
//! `glib::timeout` so brief operations remain spinner-free.

use core::time::Duration;

/// Loading-state spec with anti-flash delay.
///
/// # Examples
///
/// Build a spec, set a custom label and shorter delay:
///
/// ```
/// use core::time::Duration;
/// use big_relm4_components::state::loading::{BigLoadingSize, BigLoadingSpinnerSpec};
///
/// let spec = BigLoadingSpinnerSpec::new(BigLoadingSize::Primary)
///     .with_label("Loading library…")
///     .with_delay(Duration::from_millis(150));
///
/// assert_eq!(spec.size, BigLoadingSize::Primary);
/// assert_eq!(spec.appearance_delay_ms, 150);
/// ```
///
/// In a Relm4 component, the spec is the typed `Init`:
///
/// ```ignore
/// impl relm4::SimpleComponent for LoadingOverlay {
///     type Init = BigLoadingSpinnerSpec;
///     type Input = ();
///     type Output = ();
///     // model holds the spec; view defers spinner visibility by
///     // `spec.appearance_delay_ms` via `glib::timeout`.
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigLoadingSpinnerSpec {
    /// Logical size — used to pick a spinner asset / pixel size.
    pub size: BigLoadingSize,
    /// Visible label (e.g. "Loading library…"). Empty for chrome-less.
    pub label: String,
    /// Delay before the spinner becomes visible. Anti-flash default
    /// is 250 ms (GNOME HIG).
    pub appearance_delay_ms: u32,
}

/// Visual size of a loading indicator (inline, embedded, primary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigLoadingSize {
    /// 16 px — inline (next to a row label).
    Inline,
    /// 24 px — embedded (inside cards / panels).
    Embedded,
    /// 48 px — primary surface (full-page loading).
    Primary,
}

impl BigLoadingSpinnerSpec {
    /// Anti-flash delay recommended by GNOME HIG.
    pub const DEFAULT_APPEARANCE_DELAY_MS: u32 = 250;

    /// Creates a new instance.
    #[must_use]
    pub fn new(size: BigLoadingSize) -> Self {
        Self {
            size,
            label: String::new(),
            appearance_delay_ms: Self::DEFAULT_APPEARANCE_DELAY_MS,
        }
    }

    /// Builder: sets label.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Override the appearance delay. Use `Duration::ZERO` only when the
    /// caller is certain the work always takes longer than 1 frame.
    #[must_use]
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.appearance_delay_ms = u32::try_from(delay.as_millis()).unwrap_or(u32::MAX);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_delay_anti_flash() {
        let spec = BigLoadingSpinnerSpec::new(BigLoadingSize::Primary);
        assert_eq!(spec.appearance_delay_ms, 250);
    }

    #[test]
    fn label_attaches_to_spec() {
        let spec =
            BigLoadingSpinnerSpec::new(BigLoadingSize::Embedded).with_label("Loading library...");
        assert_eq!(spec.label, "Loading library...");
    }

    #[test]
    fn delay_override_takes_effect() {
        let spec = BigLoadingSpinnerSpec::new(BigLoadingSize::Inline)
            .with_delay(Duration::from_millis(100));
        assert_eq!(spec.appearance_delay_ms, 100);
    }
}
