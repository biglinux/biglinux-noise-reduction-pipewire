// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Scale trough CSS helpers.

use relm4::gtk;

/// Pick the trough fill colour for a volume slider that may exceed
/// 100 %. Normal range stays accent-coloured; the slider warms toward
/// warning then error as the value approaches `max_value`.
#[must_use]
pub fn volume_trough_color(value: f64, max_value: f64) -> String {
    let mid = f64::midpoint(100.0, max_value);
    if value <= 100.0 {
        "@accent_bg_color".to_string()
    } else if value <= mid {
        let t = (value - 100.0) / (mid - 100.0);
        format!("mix(@accent_bg_color, @warning_bg_color, {t:.2})")
    } else {
        let t = ((value - mid) / (max_value - mid)).min(1.0);
        format!("mix(@warning_bg_color, @error_bg_color, {t:.2})")
    }
}

/// Pick the trough fill colour for a voice-volume slider where 50 %
/// is the recommended ceiling. Warmer colours warn the user above
/// 50 % and become an error past 80 %.
#[must_use]
pub fn voice_trough_color(value: f64) -> String {
    if value <= 50.0 {
        "@accent_bg_color".to_string()
    } else if value <= 80.0 {
        let t = (value - 50.0) / 30.0;
        format!("mix(@accent_bg_color, @warning_bg_color, {t:.2})")
    } else {
        let t = (value - 80.0) / 50.0;
        format!("mix(@warning_bg_color, @error_bg_color, {t:.2})")
    }
}

/// Push a new scale trough value through the live state.
pub fn update_scale_trough(provider: &gtk::CssProvider, css_class: &str, color: &str) {
    let css = format!(".{css_class} trough highlight {{ background: {color}; }}");
    provider.load_from_string(&css);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_color_uses_accent_at_normal_volume() {
        assert_eq!(volume_trough_color(0.0, 200.0), "@accent_bg_color");
        assert_eq!(volume_trough_color(100.0, 200.0), "@accent_bg_color");
    }

    #[test]
    fn volume_color_blends_to_warning_then_error() {
        assert!(
            volume_trough_color(150.0, 200.0).contains("mix(@accent_bg_color, @warning_bg_color,")
        );
        assert!(
            volume_trough_color(175.0, 200.0).contains("mix(@warning_bg_color, @error_bg_color,")
        );
        assert!(
            volume_trough_color(200.0, 200.0)
                .contains("mix(@warning_bg_color, @error_bg_color, 1.00)")
        );
    }

    #[test]
    fn voice_color_blends_by_known_ranges() {
        assert_eq!(voice_trough_color(50.0), "@accent_bg_color");
        assert!(
            voice_trough_color(65.0).contains("mix(@accent_bg_color, @warning_bg_color, 0.50)")
        );
        assert!(
            voice_trough_color(105.0).contains("mix(@warning_bg_color, @error_bg_color, 0.50)")
        );
        assert!(
            voice_trough_color(130.0).contains("mix(@warning_bg_color, @error_bg_color, 1.00)")
        );
    }
}
