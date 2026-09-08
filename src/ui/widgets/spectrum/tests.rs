use super::*;

fn assert_close_f32(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn db_to_norm_maps_range_to_unit_interval() {
    assert!((db_to_norm(-60.0) - 0.0).abs() < 1e-6);
    assert!((db_to_norm(0.0) - 1.0).abs() < 1e-6);
    assert!((db_to_norm(-30.0) - 0.5).abs() < 1e-6);
}

#[test]
fn db_to_norm_clamps_out_of_range() {
    assert!((db_to_norm(-120.0) - 0.0).abs() < 1e-6);
    assert!((db_to_norm(20.0) - 1.0).abs() < 1e-6);
}

#[test]
fn resampled_targets_use_nearest_lower_source_band() {
    let targets = resampled_band_targets(&[-60.0, -30.0, 0.0]);

    assert_close_f32(targets[0], 0.0);
    assert_close_f32(targets[9], 0.0);
    assert_close_f32(targets[10], 0.5);
    assert_close_f32(targets[19], 0.5);
    assert_close_f32(targets[20], 1.0);
    assert_close_f32(targets[29], 1.0);
}

#[test]
fn update_targets_keeps_existing_bands_when_frame_has_no_bands() {
    let mut state = SpectrumState::default();
    state.target_bands[0] = 0.42;

    state.update_targets(&SpectrumFrame {
        seq: 1,
        bands_db: Vec::new(),
        rms_db: -30.0,
        peak_db: -30.0,
    });

    assert_close_f32(state.target_bands[0], 0.42);
    assert_close_f32(state.target_peak, 0.5);
}

#[test]
fn animation_advance_interpolates_bands_and_sets_peak_hold() {
    let mut state = SpectrumState::default();
    state.target_bands[0] = 1.0;
    state.target_peak = 0.5;

    assert!(state.advance_animation(true));

    assert_close_f32(state.bands[0], SMOOTH_FACTOR);
    assert_close_f32(state.peaks[0], SMOOTH_FACTOR);
    assert_eq!(state.band_peak_ticks[0], PEAK_HOLD_TICKS);
    assert_close_f32(state.peak_level, 0.5);
    assert_close_f32(state.peak_hold, 0.5);
    assert_eq!(state.meter_hold_ticks, METER_HOLD_TICKS);
}

#[test]
fn animation_advance_does_not_mutate_when_hidden() {
    let mut state = SpectrumState::default();
    state.target_bands[0] = 1.0;
    state.target_peak = 0.5;

    assert!(!state.advance_animation(false));

    assert_close_f32(state.bands[0], 0.0);
    assert_close_f32(state.peak_level, 0.0);
    assert_close_f32(state.peak_hold, 0.0);
}

#[test]
fn animation_advance_decays_meter_after_hold_ticks_expire() {
    let mut state = SpectrumState {
        peak_level: 0.5,
        peak_hold: 0.75,
        meter_hold_ticks: 0,
        ..SpectrumState::default()
    };

    assert!(state.advance_animation(true));

    assert_close_f32(state.peak_level, 0.5 - METER_PEAK_DECAY);
    assert_close_f32(state.peak_hold, 0.75 - METER_HOLD_DECAY);
}

#[test]
fn animation_advance_reports_peak_tail_until_noise_threshold_only() {
    let mut visible_tail = SpectrumState::default();
    visible_tail.peaks[0] = 0.04;

    assert!(visible_tail.advance_animation(true));
    assert_close_f32(visible_tail.peaks[0], 0.04 - super::constants::PEAK_DECAY);

    let mut threshold_tail = SpectrumState::default();
    threshold_tail.peaks[0] = 0.02;

    assert!(!threshold_tail.advance_animation(true));
    assert_close_f32(threshold_tail.peaks[0], 0.02 - super::constants::PEAK_DECAY);

    let mut exact_post_decay_threshold_tail = SpectrumState::default();
    exact_post_decay_threshold_tail.peaks[0] = 0.01 + super::constants::PEAK_DECAY;

    assert!(!exact_post_decay_threshold_tail.advance_animation(true));
    assert_close_f32(exact_post_decay_threshold_tail.peaks[0], 0.01);
}

#[test]
fn peak_meter_advance_uses_strict_growth_comparison() {
    let mut state = SpectrumState {
        target_peak: 0.5,
        peak_level: 0.5,
        peak_hold: 0.5,
        meter_hold_ticks: 0,
        ..SpectrumState::default()
    };

    assert!(state.advance_peak_meter());

    assert_close_f32(state.peak_level, 0.5 - METER_PEAK_DECAY);
    assert_close_f32(state.peak_hold, 0.5 - METER_HOLD_DECAY);
    assert_eq!(state.meter_hold_ticks, 0);
}

#[test]
fn peak_meter_advance_reports_visible_decay_above_noise_threshold() {
    let mut state = SpectrumState {
        peak_level: 0.04,
        peak_hold: 0.0,
        ..SpectrumState::default()
    };

    assert!(state.advance_peak_meter());

    assert_close_f32(state.peak_level, 0.04 - METER_PEAK_DECAY);
}

#[test]
fn peak_meter_advance_stops_reporting_decay_at_exact_noise_threshold() {
    let mut state = SpectrumState {
        peak_level: 0.01 + METER_PEAK_DECAY,
        peak_hold: 0.0,
        ..SpectrumState::default()
    };

    assert!(!state.advance_peak_meter());

    assert_close_f32(state.peak_level, 0.01);
}

#[test]
fn peak_meter_advance_decrements_hold_ticks_without_moving_hold_value() {
    let mut state = SpectrumState {
        peak_hold: 0.5,
        meter_hold_ticks: 2,
        ..SpectrumState::default()
    };

    assert!(state.advance_peak_meter());

    assert_close_f32(state.peak_hold, 0.5);
    assert_eq!(state.meter_hold_ticks, 1);
}

#[test]
fn peak_meter_advance_reports_hold_decay_until_noise_threshold_only() {
    let mut visible_decay = SpectrumState {
        peak_hold: 0.03,
        ..SpectrumState::default()
    };

    assert!(visible_decay.advance_peak_meter());
    assert_close_f32(visible_decay.peak_hold, 0.03 - METER_HOLD_DECAY);

    let mut threshold_decay = SpectrumState {
        peak_hold: 0.02,
        ..SpectrumState::default()
    };

    assert!(!threshold_decay.advance_peak_meter());
    assert_close_f32(threshold_decay.peak_hold, 0.02 - METER_HOLD_DECAY);
}

#[test]
fn band_advance_uses_strict_smoothing_threshold() {
    let mut state = SpectrumState::default();
    state.target_bands[0] = 0.001;

    assert!(!state.advance_bands());
    assert_close_f32(state.bands[0], 0.0);

    state.target_bands[0] = 0.002;

    assert!(state.advance_bands());
    assert_close_f32(state.bands[0], 0.002 * SMOOTH_FACTOR);
}

#[test]
fn band_advance_interpolates_toward_target_from_current_band() {
    let mut state = SpectrumState::default();
    state.bands[0] = 0.2;
    state.target_bands[0] = 0.4;

    assert!(state.advance_bands());

    assert_close_f32(state.bands[0], 0.2 + (0.4 - 0.2) * SMOOTH_FACTOR);
}

#[test]
fn band_advance_decrements_hold_ticks_before_peak_decay() {
    let mut state = SpectrumState::default();
    state.peaks[0] = 0.5;
    state.band_peak_ticks[0] = 2;

    assert!(state.advance_bands());

    assert_close_f32(state.peaks[0], 0.5);
    assert_eq!(state.band_peak_ticks[0], 1);
}

#[test]
fn band_advance_decays_peak_after_hold_ticks_expire() {
    let mut state = SpectrumState::default();
    state.peaks[0] = 0.5;

    assert!(!state.advance_bands());

    assert_close_f32(state.peaks[0], 0.5 - super::constants::PEAK_DECAY);
}

fn render_to_pixels(width: i32, height: i32, render: impl FnOnce(&cairo::Context)) -> Vec<u8> {
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).expect("image surface");
    let cairo_context = cairo::Context::new(&surface).expect("cairo context");
    render(&cairo_context);
    drop(cairo_context);
    surface.flush();
    surface.data().expect("surface pixels").to_vec()
}

fn non_zero_byte_count(pixels: &[u8]) -> usize {
    pixels.iter().filter(|byte| **byte != 0).count()
}

#[test]
#[cfg(not(miri))]
fn draw_full_spectrum_paints_background_tracks_and_active_content() {
    let mut state = SpectrumState::default();
    state.bands[0] = 0.5;
    state.bands[10] = 0.8;
    state.peaks[0] = 0.75;
    state.peak_level = 0.6;
    state.peak_hold = 0.9;

    let pixels = render_to_pixels(300, WIDGET_HEIGHT, |cairo_context| {
        draw(
            cairo_context,
            300,
            WIDGET_HEIGHT,
            &state,
            PEAK_METER_CAPTION_MSGID,
        );
    });

    assert!(non_zero_byte_count(&pixels) > 10_000);
}
