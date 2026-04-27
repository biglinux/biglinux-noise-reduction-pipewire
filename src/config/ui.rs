//! UI-related settings (window geometry, advanced toggle).

use serde::{Deserialize, Serialize};

pub const WINDOW_WIDTH_DEFAULT: u32 = 720;
pub const WINDOW_HEIGHT_DEFAULT: u32 = 700;
pub const WINDOW_WIDTH_MIN: u32 = 400;
pub const WINDOW_HEIGHT_MIN: u32 = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: WINDOW_WIDTH_DEFAULT,
            height: WINDOW_HEIGHT_DEFAULT,
            maximized: false,
        }
    }
}

impl WindowConfig {
    /// Clamp stored dimensions to sane lower bounds before the window reads
    /// them. Guards against manually-edited settings files.
    #[must_use]
    pub fn sanitized(self) -> Self {
        Self {
            width: self.width.max(WINDOW_WIDTH_MIN),
            height: self.height.max(WINDOW_HEIGHT_MIN),
            maximized: self.maximized,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub show_advanced: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_defaults_are_reasonable() {
        let w = WindowConfig::default();
        assert!(w.width >= WINDOW_WIDTH_MIN);
        assert!(w.height >= WINDOW_HEIGHT_MIN);
        assert!(!w.maximized);
    }

    #[test]
    fn window_sanitize_fixes_tiny_dimensions() {
        let w = WindowConfig {
            width: 10,
            height: 10,
            maximized: false,
        }
        .sanitized();
        assert_eq!(w.width, WINDOW_WIDTH_MIN);
        assert_eq!(w.height, WINDOW_HEIGHT_MIN);
    }
}
