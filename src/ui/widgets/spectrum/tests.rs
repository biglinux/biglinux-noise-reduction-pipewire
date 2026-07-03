use super::*;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

fn assert_close_f32(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

fn render_to_pixels(width: i32, height: i32, render: impl FnOnce(&cairo::Context)) -> Vec<u8> {
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).expect("image surface");
    let cairo_context = cairo::Context::new(&surface).expect("cairo context");
    render(&cairo_context);
    drop(cairo_context);
    surface.flush();
    let pixels = surface.data().expect("surface pixels").to_vec();
    pixels
}

fn non_zero_byte_count(pixels: &[u8]) -> usize {
    pixels.iter().filter(|byte| **byte != 0).count()
}

fn row_byte_sum(pixels: &[u8], width: i32, y: i32) -> u64 {
    let stride = width as usize * 4;
    let start = y as usize * stride;
    pixels[start..start + stride]
        .iter()
        .map(|byte| u64::from(*byte))
        .sum()
}

fn rect_byte_sum(pixels: &[u8], width: i32, x: i32, y: i32, rect_width: i32, height: i32) -> u64 {
    let stride = width as usize * 4;
    let start_x = x as usize * 4;
    let end_x = (x + rect_width) as usize * 4;
    (y..y + height)
        .map(|row| {
            let row_start = row as usize * stride;
            pixels[row_start + start_x..row_start + end_x]
                .iter()
                .map(|byte| u64::from(*byte))
                .sum::<u64>()
        })
        .sum()
}

fn pixel_bgra(pixels: &[u8], width: i32, x: i32, y: i32) -> (u8, u8, u8, u8) {
    let offset = (y as usize * width as usize + x as usize) * 4;
    (
        pixels[offset],
        pixels[offset + 1],
        pixels[offset + 2],
        pixels[offset + 3],
    )
}

fn max_row_byte_sum_inclusive(pixels: &[u8], width: i32, start_y: i32, end_y: i32) -> u64 {
    (start_y..=end_y)
        .map(|y| row_byte_sum(pixels, width, y))
        .max()
        .expect("row range is not empty")
}

#[test]
fn animation_frame_period_matches_configured_fps() {
    assert_eq!(animation_frame_period_millis(), 33);
}

#[test]
fn animation_tick_runs_only_when_area_is_mapped() {
    assert!(should_skip_animation_tick(false));
    assert!(!should_skip_animation_tick(true));
}

#[test]
fn push_frame_to_state_updates_target_peak_and_bands() {
    let state = Rc::new(RefCell::new(SpectrumState::default()));

    push_frame_to_state(
        &state,
        &SpectrumFrame {
            seq: 1,
            bands_db: vec![-60.0, -30.0, 0.0],
            rms_db: -20.0,
            peak_db: -30.0,
        },
    );

    let borrowed_state = state.borrow();
    assert_close_f32(borrowed_state.target_peak, 0.5);
    assert_close_f32(borrowed_state.target_bands[0], 0.0);
    assert_close_f32(borrowed_state.target_bands[10], 0.5);
    assert_close_f32(borrowed_state.target_bands[20], 1.0);
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
fn norm_to_db_inverse_of_db_to_norm() {
    for db in [-60.0, -30.0, -10.0, -3.0, 0.0] {
        let round = norm_to_db(db_to_norm(db));
        assert!((round - db).abs() < 1e-4, "roundtrip {db} vs {round}");
    }
}

#[test]
fn level_color_zones_match_db_thresholds() {
    assert_eq!(level_color(-30.0).0 as i32, 0); // green
    assert_eq!(level_color(-12.0).0, 0.2); // green still
    assert_eq!(level_color(-10.0), (0.2, 0.7, 0.2, 1.0)); // exact threshold remains green
    assert_eq!(level_color(-6.0), (1.0, 0.7, 0.0, 1.0)); // orange
    assert_eq!(level_color(-3.0), (1.0, 0.7, 0.0, 1.0)); // exact threshold remains orange
    assert_eq!(level_color(-1.0), (1.0, 0.2, 0.2, 1.0)); // red
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
    assert_close_f32(visible_tail.peaks[0], 0.04 - PEAK_DECAY);

    let mut threshold_tail = SpectrumState::default();
    threshold_tail.peaks[0] = 0.02;

    assert!(!threshold_tail.advance_animation(true));
    assert_close_f32(threshold_tail.peaks[0], 0.02 - PEAK_DECAY);

    let mut exact_post_decay_threshold_tail = SpectrumState::default();
    exact_post_decay_threshold_tail.peaks[0] = 0.01 + PEAK_DECAY;

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

    assert_close_f32(state.peaks[0], 0.5 - PEAK_DECAY);
}

#[test]
fn spectrum_layout_keeps_expected_sections_for_default_height() {
    let layout = spectrum_layout(300, WIDGET_HEIGHT);

    assert_eq!(
        layout.background,
        RectGeometry {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 200.0,
        }
    );
    assert_eq!(
        layout.peak_meter,
        RectGeometry {
            x: 12.0,
            y: 12.0,
            width: 276.0,
            height: 42.0,
        }
    );
    assert_eq!(
        layout.bars,
        RectGeometry {
            x: 12.0,
            y: 62.0,
            width: 276.0,
            height: 108.0,
        }
    );
    assert_close(layout.frequency_labels_y, 174.0);
}

#[test]
fn bar_geometry_maps_levels_to_active_rectangles_and_peak_ticks() {
    let bars_area = RectGeometry {
        x: 12.0,
        y: 62.0,
        width: 276.0,
        height: 108.0,
    };
    let mut state = SpectrumState::default();
    state.bands[0] = 0.5;
    state.peaks[0] = 0.75;

    let bars = bar_geometries(bars_area, &state);

    assert_eq!(
        bars[0].track,
        RectGeometry {
            x: 12.0,
            y: 62.0,
            width: 6.3,
            height: 108.0,
        }
    );
    assert_eq!(
        bars[0].active_bar,
        Some(RectGeometry {
            x: 12.0,
            y: 116.0,
            width: 6.3,
            height: 54.0,
        })
    );
    assert_close(bars[0].peak_y.expect("peak tick"), 89.0);
    assert_eq!(bars[1].active_bar, None);
    assert_close(bars[29].track.x, 281.7);
    assert_close(bars[29].track.width, 6.3);
}

#[test]
fn peak_tick_starts_only_above_noise_threshold() {
    let bars_area = RectGeometry {
        x: 0.0,
        y: 10.0,
        width: 100.0,
        height: 50.0,
    };

    assert_eq!(peak_tick_y(bars_area, 0.02), None);
    assert_close(peak_tick_y(bars_area, 0.03).expect("peak tick"), 58.5);
}

#[test]
fn segment_lines_split_sixty_decibels_into_six_equal_ranges() {
    let lines = segment_y_positions(RectGeometry {
        x: 0.0,
        y: 60.0,
        width: 60.0,
        height: 120.0,
    });

    assert_eq!(lines, [160.0, 140.0, 120.0, 100.0, 80.0]);
}

#[test]
fn decibel_grid_lines_map_labels_to_y_positions() {
    let lines = decibel_grid_lines(RectGeometry {
        x: 0.0,
        y: 60.0,
        width: 60.0,
        height: 120.0,
    });

    assert_eq!(
        lines,
        [
            DecibelGridLine {
                label: "-20",
                y: 100.0,
            },
            DecibelGridLine {
                label: "-40",
                y: 140.0,
            },
        ]
    );
}

#[test]
fn frequency_labels_stay_centered_on_legacy_marker_bands() {
    let labels = frequency_labels(spectrum_layout(300, WIDGET_HEIGHT));

    assert_eq!(labels[0].text, "63 Hz");
    assert_close(labels[0].center_x, 61.65);
    assert_eq!(labels[5].text, "9.5 kHz");
    assert_close(labels[5].center_x, 284.85);
    assert_close(labels[5].baseline_y, 186.0);
}

#[test]
fn peak_meter_geometry_maps_text_bar_ticks_and_hold() {
    assert_eq!(PEAK_METER_TICK_VALUES, [-50, -40, -30, -20, -10]);

    let geometry = peak_meter_geometry(
        RectGeometry {
            x: 12.0,
            y: 12.0,
            width: 276.0,
            height: 42.0,
        },
        0.5,
        0.75,
        PeakMeterTextMetrics {
            value_advance: 40.0,
            divider_advance: 6.0,
            hold_advance: 55.0,
            tick_label_widths: [18.0; PEAK_METER_TICK_VALUES.len()],
        },
    );

    assert_eq!(
        geometry.value_position,
        TextPosition {
            x: 12.0,
            baseline_y: 40.0,
        }
    );
    assert_eq!(
        geometry.divider_position,
        TextPosition {
            x: 57.0,
            baseline_y: 40.0,
        }
    );
    assert_eq!(
        geometry.hold_position,
        TextPosition {
            x: 68.0,
            baseline_y: 40.0,
        }
    );
    assert_eq!(
        geometry.track,
        RectGeometry {
            x: 143.0,
            y: 24.0,
            width: 140.0,
            height: 8.0,
        }
    );
    assert_eq!(
        geometry.active,
        Some(RectGeometry {
            x: 143.0,
            y: 24.0,
            width: 70.0,
            height: 8.0,
        })
    );
    assert_eq!(geometry.ticks[0].db, -50);
    assert_close(geometry.ticks[0].x, 143.0 + 140.0 / 6.0);
    assert_close(
        geometry.ticks[0].label_position.x,
        143.0 + 140.0 / 6.0 - 9.0,
    );
    assert_close(geometry.ticks[0].label_position.baseline_y, 42.0);
    assert_close(geometry.ticks[4].x, 143.0 + 140.0 * 5.0 / 6.0);
    assert_eq!(
        geometry.hold_indicator,
        Some(RectGeometry {
            x: 247.0,
            y: 22.0,
            width: 2.0,
            height: 12.0,
        })
    );
}

#[test]
fn peak_meter_geometry_uses_minimum_width_and_hides_tiny_levels() {
    let geometry = peak_meter_geometry(
        RectGeometry {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 42.0,
        },
        0.01,
        0.01,
        PeakMeterTextMetrics {
            value_advance: 60.0,
            divider_advance: 10.0,
            hold_advance: 80.0,
            tick_label_widths: [0.0; PEAK_METER_TICK_VALUES.len()],
        },
    );

    assert_close(geometry.track.width, 40.0);
    assert_eq!(geometry.active, None);
    assert_eq!(geometry.hold_indicator, None);
}

#[test]
fn peak_meter_geometry_hides_active_bar_at_exact_one_pixel_threshold() {
    let geometry = peak_meter_geometry(
        RectGeometry {
            x: 0.0,
            y: 0.0,
            width: 249.0,
            height: 42.0,
        },
        0.015_625,
        0.0,
        PeakMeterTextMetrics {
            value_advance: 60.0,
            divider_advance: 10.0,
            hold_advance: 80.0,
            tick_label_widths: [0.0; PEAK_METER_TICK_VALUES.len()],
        },
    );

    assert_close(geometry.track.width, 64.0);
    assert_eq!(geometry.active, None);
}

#[test]
fn gradient_stops_scale_color_channels_but_not_alpha() {
    let stops = gradient_stops(0.2);

    assert_eq!(stops.len(), 6);
    assert_eq!(stops[0].offset, 0.0);
    assert_close(stops[0].green, 0.12);
    assert_eq!(stops[1].offset, 0.667);
    assert_close(stops[1].green, 0.12);
    assert_eq!(stops[2].offset, 0.6671);
    assert_close(stops[2].red, 0.2);
    assert_close(stops[2].green, 0.12);
    assert_eq!(stops[3].offset, 0.833);
    assert_close(stops[3].red, 0.2);
    assert_close(stops[3].green, 0.12);
    assert_eq!(stops[4].offset, 0.8331);
    assert_close(stops[4].red, 0.2);
    assert_eq!(stops[5].offset, 1.0);
    assert_close(stops[5].red, 0.2);
    assert!(stops.iter().all(|stop| stop.alpha == 1.0));
}

#[test]
fn rendering_coordinate_contracts_match_cairo_draw_positions() {
    let rect = RectGeometry {
        x: 12.0,
        y: 62.0,
        width: 276.0,
        height: 108.0,
    };

    assert_eq!(
        vertical_gradient_range_for_rect(rect),
        VerticalGradientRange {
            y_start: 170.0,
            y_end: 62.0,
        }
    );
    assert_eq!(
        horizontal_gradient_range_for_rect(rect),
        HorizontalGradientRange {
            x_start: 12.0,
            x_end: 288.0,
        }
    );
    assert_eq!(
        horizontal_line_for_y(rect, 100.0),
        LineSegment {
            x_start: 12.0,
            y_start: 100.0,
            x_end: 288.0,
            y_end: 100.0,
        }
    );
    assert_eq!(
        peak_tick_rect(rect, 89.0),
        RectGeometry {
            x: 12.0,
            y: 88.5,
            width: 276.0,
            height: 1.5,
        }
    );
    assert_eq!(
        peak_meter_tick_line(rect, 140.0),
        LineSegment {
            x_start: 140.0,
            y_start: 62.0,
            x_end: 140.0,
            y_end: 170.0,
        }
    );
    assert_eq!(
        decibel_grid_label_position(rect, 100.0, 18.0),
        TextPosition {
            x: 268.0,
            baseline_y: 98.0,
        }
    );
    assert_eq!(
        centered_text_position(120.0, 186.0, 40.0),
        TextPosition {
            x: 100.0,
            baseline_y: 186.0,
        }
    );
}

#[test]
#[cfg(not(miri))]
fn cairo_zone_gradients_keep_expected_axes_and_color_stops() {
    let vertical = zone_gradient_vertical(120.0, 12.0, 0.2);
    assert_eq!(
        vertical.linear_points().expect("vertical gradient points"),
        (0.0, 120.0, 0.0, 12.0)
    );
    assert_eq!(vertical.color_stop_count().expect("vertical stop count"), 6);
    assert_eq!(
        vertical.color_stop_rgba(0).expect("first vertical stop"),
        (0.0, 0.0, 0.12, 0.0, 1.0)
    );

    let horizontal = zone_gradient_horizontal(40.0, 220.0, 1.0);
    assert_eq!(
        horizontal
            .linear_points()
            .expect("horizontal gradient points"),
        (40.0, 0.0, 220.0, 0.0)
    );
    assert_eq!(
        horizontal
            .color_stop_count()
            .expect("horizontal stop count"),
        6
    );
    assert_eq!(
        horizontal.color_stop_rgba(5).expect("last horizontal stop"),
        (1.0, 1.0, 0.0, 0.0, 1.0)
    );
}

#[test]
fn corner_radius_never_exceeds_half_of_shortest_side() {
    assert_close(clamped_corner_radius(20.0, 10.0, 12.0), 5.0);
    assert_close(clamped_corner_radius(8.0, 20.0, 12.0), 4.0);
    assert_close(clamped_corner_radius(20.0, 10.0, 2.0), 2.0);
}

#[test]
fn rounded_corner_arcs_clamp_radius_and_follow_clockwise_order() {
    let arcs = rounded_corner_arcs(
        RectGeometry {
            x: 2.0,
            y: 4.0,
            width: 20.0,
            height: 10.0,
        },
        12.0,
    )
    .expect("rounded arcs");

    assert_close(arcs[0].center_x, 17.0);
    assert_close(arcs[0].center_y, 9.0);
    assert_close(arcs[0].radius, 5.0);
    assert_close(arcs[0].start_angle, -std::f64::consts::PI / 2.0);
    assert_close(arcs[0].end_angle, 0.0);
    assert_close(arcs[1].center_x, 17.0);
    assert_close(arcs[1].center_y, 9.0);
    assert_close(arcs[1].end_angle, std::f64::consts::PI / 2.0);
    assert_close(arcs[2].center_x, 7.0);
    assert_close(arcs[2].center_y, 9.0);
    assert_close(arcs[2].start_angle, std::f64::consts::PI / 2.0);
    assert_close(arcs[3].center_x, 7.0);
    assert_close(arcs[3].center_y, 9.0);
    assert_close(arcs[3].end_angle, 3.0 * std::f64::consts::PI / 2.0);
}

#[test]
fn rounded_corner_arcs_keep_exact_one_pixel_radius_rounded() {
    let arcs = rounded_corner_arcs(
        RectGeometry {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        },
        1.0,
    )
    .expect("one pixel radius still has arcs");

    assert_close(arcs[0].radius, 1.0);
}

#[test]
fn rounded_corner_arcs_return_none_for_square_corners() {
    assert_eq!(
        rounded_corner_arcs(
            RectGeometry {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 10.0,
            },
            0.5,
        ),
        None
    );
}

#[test]
#[cfg(not(miri))]
fn rounded_rect_paints_pixels_on_image_surface() {
    let pixels = render_to_pixels(24, 16, |cairo_context| {
        cairo_context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        rounded_rect(cairo_context, 2.0, 2.0, 20.0, 12.0, 4.0);
        cairo_context.fill().expect("fill rounded rectangle");
    });

    assert!(non_zero_byte_count(&pixels) > 100);
}

#[test]
#[cfg(not(miri))]
fn set_source_rgba_tuple_paints_requested_source() {
    let pixels = render_to_pixels(16, 16, |cairo_context| {
        cairo_context.set_source_rgba_tuple((1.0, 0.0, 0.0, 1.0));
        cairo_context.rectangle(4.0, 4.0, 8.0, 8.0);
        cairo_context.fill().expect("fill tuple source rectangle");
    });

    assert!(rect_byte_sum(&pixels, 16, 4, 4, 8, 8) > 8_000);
    assert_eq!(rect_byte_sum(&pixels, 16, 0, 0, 2, 2), 0);
    let (blue, green, red, alpha) = pixel_bgra(&pixels, 16, 8, 8);
    assert!(red > 200);
    assert!(green < 10);
    assert!(blue < 10);
    assert_eq!(alpha, 255);
}

#[test]
#[cfg(not(miri))]
fn draw_bars_paints_tracks_active_bars_segments_and_peak_ticks() {
    let width = 320;
    let mut state = SpectrumState::default();
    state.bands[0] = 0.5;
    state.bands[1] = 0.8;
    state.peaks[0] = 0.75;

    let pixels = render_to_pixels(width, 140, |cairo_context| {
        draw_bars(
            cairo_context,
            RectGeometry {
                x: 12.0,
                y: 12.0,
                width: 276.0,
                height: 108.0,
            },
            &state,
        );
    });

    assert!(rect_byte_sum(&pixels, width, 12, 12, 7, 108) > 5_000);
    assert!(rect_byte_sum(&pixels, width, 12, 66, 7, 54) > 4_000);
    assert!(rect_byte_sum(&pixels, width, 12, 38, 7, 3) > 1_000);
    assert_eq!(rect_byte_sum(&pixels, width, 0, 0, 8, 8), 0);
}

#[test]
#[cfg(not(miri))]
fn draw_db_grid_marks_expected_rows_on_image_surface() {
    let width = 120;
    let pixels = render_to_pixels(width, 180, |cairo_context| {
        draw_db_grid(
            cairo_context,
            RectGeometry {
                x: 0.0,
                y: 60.0,
                width: 120.0,
                height: 120.0,
            },
        );
    });

    assert!(max_row_byte_sum_inclusive(&pixels, width, 98, 102) > 0);
    assert!(max_row_byte_sum_inclusive(&pixels, width, 138, 142) > 0);
    assert!(rect_byte_sum(&pixels, width, 90, 90, 28, 16) > 0);
    assert!(rect_byte_sum(&pixels, width, 90, 130, 28, 16) > 0);
    assert_eq!(max_row_byte_sum_inclusive(&pixels, width, 20, 30), 0);
}

#[test]
#[cfg(not(miri))]
fn draw_peak_meter_paints_text_bar_ticks_and_hold_indicator() {
    let width = 320;
    let pixels = render_to_pixels(width, 80, |cairo_context| {
        draw_peak_meter(
            cairo_context,
            RectGeometry {
                x: 12.0,
                y: 12.0,
                width: 276.0,
                height: 42.0,
            },
            0.6,
            0.9,
        );
    });

    assert!(non_zero_byte_count(&pixels) > 2_000);
    assert!(max_row_byte_sum_inclusive(&pixels, width, 22, 34) > 0);
    assert_eq!(max_row_byte_sum_inclusive(&pixels, width, 70, 79), 0);
}

#[test]
#[cfg(not(miri))]
fn draw_frequency_labels_paints_text_on_image_surface() {
    let labels = frequency_labels(spectrum_layout(300, WIDGET_HEIGHT));
    let pixels = render_to_pixels(300, WIDGET_HEIGHT, |cairo_context| {
        draw_frequency_labels(cairo_context, &labels);
    });

    assert!(non_zero_byte_count(&pixels) > 200);
    assert!(rect_byte_sum(&pixels, 300, 42, 174, 40, 18) > 0);
    assert!(rect_byte_sum(&pixels, 300, 250, 174, 48, 18) > 0);
    assert_eq!(rect_byte_sum(&pixels, 300, 0, 0, 20, 20), 0);
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
        draw(cairo_context, 300, WIDGET_HEIGHT, &state);
    });

    assert!(non_zero_byte_count(&pixels) > 10_000);
}
