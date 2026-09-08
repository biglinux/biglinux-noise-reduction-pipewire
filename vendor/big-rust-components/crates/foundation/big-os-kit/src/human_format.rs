// SPDX-License-Identifier: MIT

//! Human-readable formatting of byte counts, transfer speeds, and durations.
//!
//! Binary (1024-based) units with one-decimal precision, matching the byte and
//! duration strings shown by BigLinux file-transfer progress UIs.

/// Format a raw quantity in binary units with a caller-supplied `suffix`
/// (e.g. `"B"` or `"B/s"`). `zero_check_lte` collapses non-positive values to
/// `0 {suffix}` (use `true` for rates, `false` for sizes); NaN also collapses
/// to zero. Values under 1 KiB render bare; larger ones use K/M/G/T prefixes.
#[must_use]
pub fn format_bytes_with_unit(value: f64, suffix: &str, zero_check_lte: bool) -> String {
    if value.is_nan() {
        return format!("0 {suffix}");
    }
    if (zero_check_lte && value <= 0.0) || (!zero_check_lte && value < 0.0) {
        return format!("0 {suffix}");
    }
    if value < 1024.0 {
        return small_bytes(value, suffix);
    }
    let kib = 1024.0_f64;
    let mib = kib * kib;
    let gib = mib * kib;
    let tib = gib * kib;
    if value < mib {
        return format!("{:.1} K{suffix}", value / kib);
    }
    if value < gib {
        return format!("{:.1} M{suffix}", value / mib);
    }
    if value < tib {
        return format!("{:.1} G{suffix}", value / gib);
    }
    format!("{:.1} T{suffix}", value / tib)
}
fn small_bytes(value: f64, suffix: &str) -> String {
    if value.fract() == 0.0 {
        // Safe: caller guards `value >= 0.0 && value < 1024.0`,
        // so `value as i64` never saturates.
        #[allow(clippy::cast_possible_truncation)]
        let whole = value as i64;
        return format!("{whole} {suffix}");
    }
    format!("{value:.1} {suffix}")
}
/// Format a file size in bytes as a human-readable string (`"1.5 MB"`).
#[must_use]
pub fn format_file_size(size_bytes: u64) -> String {
    // Precision loss above 2^52 bytes (~4 PB) is absorbed by the
    // one-decimal G-branch rounding — matches Python int→float.
    #[allow(clippy::cast_precision_loss)]
    let as_float = size_bytes as f64;
    format_bytes_with_unit(as_float, "B", false)
}
/// Format a transfer speed in bytes per second (`"2.5 MB/s"`).
#[must_use]
pub fn format_speed(bytes_per_second: f64) -> String {
    format_bytes_with_unit(bytes_per_second, "B/s", true)
}
/// Format a duration in seconds as `"45s"`, `"1m 30s"`, or `"1h 1m"`.
/// Non-finite or negative inputs collapse to `"0s"`.
#[must_use]
pub fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0s".to_owned();
    }
    // Safe: guarded `seconds >= 0.0 && is_finite`; matches
    // Python's `int(seconds)` truncation toward zero.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let total = seconds as u64;
    if total < 60 {
        return format!("{total}s");
    }
    let minutes = total / 60;
    let secs = total % 60;
    if minutes < 60 {
        return format!("{minutes}m {secs}s");
    }
    let hours = minutes / 60;
    let mins = minutes % 60;
    format!("{hours}h {mins}m")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_bytes_with_unit — zero / negative guards ─────────────

    #[test]
    fn zero_check_lte_false_allows_zero_through() {
        assert_eq!(format_bytes_with_unit(0.0, "B", false), "0 B");
    }

    #[test]
    fn zero_check_lte_true_shortcircuits_zero() {
        assert_eq!(format_bytes_with_unit(0.0, "B/s", true), "0 B/s");
    }

    #[test]
    fn zero_check_lte_false_rejects_negative() {
        assert_eq!(format_bytes_with_unit(-5.0, "B", false), "0 B");
    }

    #[test]
    fn zero_check_lte_true_rejects_negative() {
        assert_eq!(format_bytes_with_unit(-5.0, "B/s", true), "0 B/s");
    }

    #[test]
    fn nan_collapses_to_zero_string() {
        assert_eq!(format_bytes_with_unit(f64::NAN, "B", false), "0 B");
    }

    // ── format_bytes_with_unit — small-bytes divergence ────────────

    #[test]
    fn integral_small_value_emits_bare_integer() {
        assert_eq!(format_bytes_with_unit(123.0, "B", false), "123 B");
    }

    #[test]
    fn fractional_small_value_emits_one_decimal() {
        assert_eq!(format_bytes_with_unit(123.4, "B/s", true), "123.4 B/s");
    }

    #[test]
    fn just_under_kib_boundary_still_small_branch() {
        assert_eq!(format_bytes_with_unit(1023.0, "B", false), "1023 B");
    }

    // ── format_bytes_with_unit — K/M/G thresholds ──────────────────

    #[test]
    fn exactly_kib_crosses_into_k_branch() {
        assert_eq!(format_bytes_with_unit(1024.0, "B", false), "1.0 KB");
    }

    #[test]
    fn mib_minus_one_still_in_k_branch() {
        let v = 1024.0_f64 * 1024.0 - 1.0;
        let out = format_bytes_with_unit(v, "B", false);
        assert!(out.ends_with(" KB"), "got: {out}");
        assert!(out.starts_with("1024.0"), "got: {out}");
    }

    #[test]
    fn exactly_mib_crosses_into_m_branch() {
        let v = 1024.0_f64 * 1024.0;
        assert_eq!(format_bytes_with_unit(v, "B", false), "1.0 MB");
    }

    #[test]
    fn gib_minus_one_still_in_m_branch() {
        let v = 1024.0_f64.powi(3) - 1.0;
        let out = format_bytes_with_unit(v, "B", false);
        assert!(out.ends_with(" MB"), "got: {out}");
    }

    #[test]
    fn exactly_gib_crosses_into_g_branch() {
        let v = 1024.0_f64.powi(3);
        assert_eq!(format_bytes_with_unit(v, "B", false), "1.0 GB");
    }

    #[test]
    fn ten_gib_stays_in_g_branch() {
        let v = 10.0 * 1024.0_f64.powi(3);
        assert_eq!(format_bytes_with_unit(v, "B", false), "10.0 GB");
    }

    // ── format_file_size wrapper ───────────────────────────────────

    #[test]
    fn file_size_zero_renders_via_small_branch() {
        assert_eq!(format_file_size(0), "0 B");
    }

    #[test]
    fn file_size_small_integral_no_decimal() {
        assert_eq!(format_file_size(512), "512 B");
    }

    #[test]
    fn file_size_mib_scaled_with_decimal() {
        assert_eq!(format_file_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn file_size_mixed_mib_rounds_to_one_decimal() {
        assert_eq!(format_file_size(1024 * 1024 * 3 / 2), "1.5 MB");
    }

    // ── format_speed wrapper ───────────────────────────────────────

    #[test]
    fn speed_zero_shortcircuits_before_division() {
        assert_eq!(format_speed(0.0), "0 B/s");
    }

    #[test]
    fn speed_small_fractional_formats_with_decimal() {
        assert_eq!(format_speed(250.5), "250.5 B/s");
    }

    #[test]
    fn speed_small_integral_drops_decimal() {
        assert_eq!(format_speed(250.0), "250 B/s");
    }

    #[test]
    fn speed_mib_per_second() {
        let v = 2.5 * 1024.0 * 1024.0;
        assert_eq!(format_speed(v), "2.5 MB/s");
    }

    // ── format_duration ────────────────────────────────────────────

    #[test]
    fn duration_zero_seconds() {
        assert_eq!(format_duration(0.0), "0s");
    }

    #[test]
    fn duration_under_a_minute_uses_seconds_only() {
        assert_eq!(format_duration(45.0), "45s");
    }

    #[test]
    fn duration_exactly_one_minute_emits_m_and_zero_s() {
        assert_eq!(format_duration(60.0), "1m 0s");
    }

    #[test]
    fn duration_minute_and_a_half() {
        assert_eq!(format_duration(90.0), "1m 30s");
    }

    #[test]
    fn duration_exactly_one_hour_emits_h_and_zero_m() {
        assert_eq!(format_duration(3600.0), "1h 0m");
    }

    #[test]
    fn duration_hour_plus_one_minute_drops_seconds() {
        assert_eq!(format_duration(3661.0), "1h 1m");
    }

    #[test]
    fn duration_truncates_fractional_seconds() {
        assert_eq!(format_duration(59.9), "59s");
    }

    #[test]
    fn duration_negative_collapses_to_zero() {
        assert_eq!(format_duration(-10.0), "0s");
    }

    #[test]
    fn duration_nan_collapses_to_zero() {
        assert_eq!(format_duration(f64::NAN), "0s");
    }

    #[test]
    fn duration_infinite_collapses_to_zero() {
        assert_eq!(format_duration(f64::INFINITY), "0s");
    }
}
