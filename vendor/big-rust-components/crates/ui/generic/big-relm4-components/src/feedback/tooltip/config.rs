// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Runtime configuration for BigLinux custom tooltips.

const DEFAULT_SHOW_DELAY_MS: u32 = 150;
const DEFAULT_FADE_OUT_MS: u32 = 300;
const DEFAULT_CSS_FADE_MS: u32 = 200;
const DEFAULT_MAX_WIDTH_CHARS: i32 = 80;

/// Runtime tooltip behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigTooltipConfig {
    /// Show delay ms.
    pub show_delay_ms: u32,
    /// Fade out ms.
    pub fade_out_ms: u32,
    /// Css fade ms.
    pub css_fade_ms: u32,
    /// Max width chars.
    pub max_width_chars: i32,
}

impl Default for BigTooltipConfig {
    fn default() -> Self {
        Self {
            show_delay_ms: DEFAULT_SHOW_DELAY_MS,
            fade_out_ms: DEFAULT_FADE_OUT_MS,
            css_fade_ms: DEFAULT_CSS_FADE_MS,
            max_width_chars: DEFAULT_MAX_WIDTH_CHARS,
        }
    }
}

impl BigTooltipConfig {
    /// Present the delay ms surface.
    #[must_use]
    pub fn show_delay_ms(mut self, value: u32) -> Self {
        self.show_delay_ms = value;
        self
    }

    /// Configure the `fade_out_ms` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigTooltipConfig`].
    #[must_use]
    pub fn fade_out_ms(mut self, value: u32) -> Self {
        self.fade_out_ms = value;
        self
    }

    /// Configure the `css_fade_ms` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigTooltipConfig`].
    #[must_use]
    pub fn css_fade_ms(mut self, value: u32) -> Self {
        self.css_fade_ms = value;
        self
    }

    /// Configure the `max_width_chars` setting and return the updated builder.
    ///
    /// The supplied `value` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigTooltipConfig`].
    #[must_use]
    pub fn max_width_chars(mut self, value: i32) -> Self {
        self.max_width_chars = value;
        self
    }
}
