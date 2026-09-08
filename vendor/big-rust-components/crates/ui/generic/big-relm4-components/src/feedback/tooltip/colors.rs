// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Tooltip color policy.

pub(super) fn theme_colors() -> (&'static str, &'static str) {
    if adw::is_initialized() {
        let style = adw::StyleManager::default();
        if style.is_dark() {
            ("#1a1a1a", "#ffffff")
        } else {
            ("#fafafa", "#2e2e2e")
        }
    } else {
        ("#2a2a2a", "#ffffff")
    }
}

pub(super) fn adjust_tooltip_background(bg: &str) -> String {
    let hex = bg.trim_start_matches('#');
    if hex.len() < 6 {
        return bg.to_string();
    }
    let Ok(r) = u8::from_str_radix(&hex[0..2], 16) else {
        return bg.to_string();
    };
    let Ok(g) = u8::from_str_radix(&hex[2..4], 16) else {
        return bg.to_string();
    };
    let Ok(b) = u8::from_str_radix(&hex[4..6], 16) else {
        return bg.to_string();
    };

    let luminance = (0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)) / 255.0;
    let (r, g, b) = if luminance < 0.5 {
        (
            (u16::from(r) + 40).min(255) as u8,
            (u16::from(g) + 40).min(255) as u8,
            (u16::from(b) + 40).min(255) as u8,
        )
    } else {
        (
            r.saturating_sub(20),
            g.saturating_sub(20),
            b.saturating_sub(20),
        )
    };
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub(super) fn is_dark_color(color: &str) -> bool {
    let hex = color.trim_start_matches('#');
    if hex.len() < 6 {
        return true;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    let luminance = (0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)) / 255.0;
    luminance < 0.5
}
