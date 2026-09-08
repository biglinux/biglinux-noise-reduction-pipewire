// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared media and dialog time formatting.
//!
//! Pure helpers used by transports, queue rows, file-info dialogs, and editor
//! controls. All formatters tolerate `NaN`, infinities, and negative input by
//! returning a stable placeholder instead of panicking.

/// Format `total_secs` for a media library row, e.g. `"1h 05min"` or `"5 min"`.
///
/// Floors to minute precision and omits the hours field when the duration is
/// under an hour.
///
/// # Examples
///
/// ```
/// use big_relm4_components::time::duration_hm;
///
/// assert_eq!(duration_hm(300.0), "5 min");
/// assert_eq!(duration_hm(3900.0), "1h 05min");
/// ```
#[must_use]
pub fn duration_hm(total_secs: f64) -> String {
    let total_mins = (total_secs / 60.0) as u64;
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if hours > 0 {
        format!("{hours}h {mins:02}min")
    } else {
        format!("{mins} min")
    }
}

/// Format `total_secs` as `H:MM:SS` or `M:SS`; returns an empty string for invalid input.
///
/// Use when the empty string should hide the label entirely. For a placeholder
/// string, see [`duration_or_placeholder`].
#[must_use]
pub fn duration_hms(total_secs: f64) -> String {
    if total_secs <= 0.0 || !total_secs.is_finite() {
        return String::new();
    }
    hms(total_secs as u64)
}

/// Format current playback position; falls back to `"0:00"` for invalid input.
#[must_use]
pub fn playback_time(secs: f64) -> String {
    if secs < 0.0 || !secs.is_finite() {
        return "0:00".to_string();
    }
    hms(secs as u64)
}

/// Format a duration with the `"--:--"` placeholder for unknown values.
///
/// Rounds `secs` to the nearest whole second. Use on queue rows where an empty
/// string would collapse the column.
#[must_use]
pub fn duration_or_placeholder(secs: f64) -> String {
    if secs <= 0.0 || !secs.is_finite() {
        return "--:--".to_string();
    }
    hms(secs.round() as u64)
}

/// Format `secs` as `H:MM:SS.mmm` with millisecond precision.
///
/// Use in editor controls where the user needs sub-second feedback.
#[must_use]
pub fn precise_time_ms(secs: f64) -> String {
    if secs < 0.0 || !secs.is_finite() {
        return "0:00:00.000".to_string();
    }
    let total_ms = (secs * 1000.0) as u64;
    let ms = total_ms % 1000;
    let total_secs = total_ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h}:{m:02}:{s:02}.{ms:03}")
}

/// Parse a user-entered time string into seconds.
///
/// Accepts plain decimal seconds (`"123.5"`), `M:SS` form (`"2:30"`), and
/// `H:MM:SS` form (`"1:02:03"`). Negative input is clamped to zero. Any other
/// shape returns [`None`].
#[must_use]
pub fn parse_playback_time(input: &str) -> Option<f64> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Ok(secs) = input.parse::<f64>() {
        return Some(secs.max(0.0));
    }
    let parts: Vec<&str> = input.split(':').collect();
    match parts.len() {
        2 => {
            let minutes = parts[0].parse::<f64>().ok()?;
            let seconds = parts[1].parse::<f64>().ok()?;
            Some((minutes * 60.0 + seconds).max(0.0))
        }
        3 => {
            let hours = parts[0].parse::<f64>().ok()?;
            let minutes = parts[1].parse::<f64>().ok()?;
            let seconds = parts[2].parse::<f64>().ok()?;
            Some((hours * 3600.0 + minutes * 60.0 + seconds).max(0.0))
        }
        _ => None,
    }
}

/// Format a countdown label as zero-padded `MM:SS`.
#[must_use]
pub fn countdown_mm_ss(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Render elapsed wall-clock time for completion summaries.
#[must_use]
pub fn elapsed_compact(duration: std::time::Duration) -> String {
    let secs = duration.as_secs_f64();
    if secs < 60.0 {
        if secs < 10.0 {
            format!("{secs:.1} s")
        } else {
            format!("{secs:.0} s")
        }
    } else if secs < 3600.0 {
        let mins = (secs / 60.0) as u64;
        let rem = (secs - (mins as f64 * 60.0)).round() as u64;
        format!("{mins} min {rem} s")
    } else {
        let hours = (secs / 3600.0) as u64;
        let rem = ((secs - (hours as f64 * 3600.0)) / 60.0).round() as u64;
        format!("{hours}h {rem}min")
    }
}

fn hms(total: u64) -> String {
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_hm_formats_library_totals() {
        assert_eq!(duration_hm(0.0), "0 min");
        assert_eq!(duration_hm(300.0), "5 min");
        assert_eq!(duration_hm(3900.0), "1h 05min");
    }

    #[test]
    fn duration_hms_hides_invalid_values() {
        assert_eq!(duration_hms(0.0), "");
        assert_eq!(duration_hms(185.0), "3:05");
        assert_eq!(duration_hms(3661.0), "1:01:01");
        assert_eq!(duration_hms(f64::NAN), "");
    }

    #[test]
    fn playback_time_has_visible_zero_fallback() {
        assert_eq!(playback_time(0.0), "0:00");
        assert_eq!(playback_time(125.0), "2:05");
        assert_eq!(playback_time(3661.0), "1:01:01");
        assert_eq!(playback_time(f64::INFINITY), "0:00");
    }

    #[test]
    fn duration_or_placeholder_rounds_and_marks_unknown() {
        assert_eq!(duration_or_placeholder(0.0), "--:--");
        assert_eq!(duration_or_placeholder(61.6), "1:02");
        assert_eq!(duration_or_placeholder(f64::NAN), "--:--");
    }

    #[test]
    fn precise_time_ms_preserves_milliseconds() {
        assert_eq!(precise_time_ms(0.0), "0:00:00.000");
        assert_eq!(precise_time_ms(61.234), "0:01:01.234");
        assert_eq!(precise_time_ms(3661.999), "1:01:01.999");
        assert_eq!(precise_time_ms(-1.0), "0:00:00.000");
    }

    #[test]
    fn parse_playback_time_accepts_seconds_minutes_and_hours() {
        assert_eq!(parse_playback_time("123.5"), Some(123.5));
        assert_eq!(parse_playback_time("2:30"), Some(150.0));
        assert_eq!(parse_playback_time("1:02:03"), Some(3723.0));
        assert_eq!(parse_playback_time("-5"), Some(0.0));
        assert_eq!(parse_playback_time("abc:def"), None);
        assert_eq!(parse_playback_time(""), None);
    }

    #[test]
    fn countdown_mm_ss_zero_pads_minutes_and_seconds() {
        assert_eq!(countdown_mm_ss(0), "00:00");
        assert_eq!(countdown_mm_ss(65), "01:05");
        assert_eq!(countdown_mm_ss(3600), "60:00");
    }

    #[test]
    fn elapsed_compact_formats_completion_summaries() {
        assert_eq!(
            elapsed_compact(std::time::Duration::from_millis(400)),
            "0.4 s"
        );
        assert_eq!(elapsed_compact(std::time::Duration::from_secs(12)), "12 s");
        assert_eq!(
            elapsed_compact(std::time::Duration::from_secs(83)),
            "1 min 23 s"
        );
        assert_eq!(
            elapsed_compact(std::time::Duration::from_secs(7_500)),
            "2h 5min"
        );
    }
}
