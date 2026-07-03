use super::constants::{BG_RADIUS, CORNER_RADIUS, PEAK_METER_TICK_VALUES};
use super::geometry::{
    bar_geometries, decibel_grid_lines, frequency_labels, gradient_stops, level_color, norm_to_db,
    peak_meter_geometry, rounded_corner_arcs, spectrum_layout, FrequencyLabel,
    PeakMeterTextMetrics, RectGeometry, TextPosition,
};
use super::state::SpectrumState;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct VerticalGradientRange {
    pub(super) y_start: f64,
    pub(super) y_end: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct HorizontalGradientRange {
    pub(super) x_start: f64,
    pub(super) x_end: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LineSegment {
    pub(super) x_start: f64,
    pub(super) y_start: f64,
    pub(super) x_end: f64,
    pub(super) y_end: f64,
}

pub(super) fn draw(cairo_context: &cairo::Context, width: i32, height: i32, state: &SpectrumState) {
    let layout = spectrum_layout(width, height);

    // Almost-black backdrop with rounded corners.
    cairo_context.set_source_rgba(0.06, 0.06, 0.06, 1.0);
    rounded_rect(
        cairo_context,
        layout.background.x,
        layout.background.y,
        layout.background.width,
        layout.background.height,
        BG_RADIUS,
    );
    cairo_context.fill().ok();

    draw_peak_meter(
        cairo_context,
        layout.peak_meter,
        state.peak_level,
        state.peak_hold,
    );
    draw_bars(cairo_context, layout.bars, state);
    draw_frequency_labels(cairo_context, &frequency_labels(layout));
    draw_db_grid(cairo_context, layout.bars);
}

pub(super) fn draw_bars(
    cairo_context: &cairo::Context,
    bars_area: RectGeometry,
    state: &SpectrumState,
) {
    let bar_gradient_range = vertical_gradient_range_for_rect(bars_area);
    let bg_gradient =
        zone_gradient_vertical(bar_gradient_range.y_start, bar_gradient_range.y_end, 0.2);
    let fg_gradient =
        zone_gradient_vertical(bar_gradient_range.y_start, bar_gradient_range.y_end, 1.0);

    for bar in bar_geometries(bars_area, state) {
        // Dark track
        cairo_context.set_source(&bg_gradient).ok();
        rounded_rect(
            cairo_context,
            bar.track.x,
            bar.track.y,
            bar.track.width,
            bar.track.height,
            CORNER_RADIUS,
        );
        cairo_context.fill().ok();

        // Active bar
        if let Some(active_bar) = bar.active_bar {
            cairo_context.set_source(&fg_gradient).ok();
            rounded_rect(
                cairo_context,
                active_bar.x,
                active_bar.y,
                active_bar.width,
                active_bar.height,
                CORNER_RADIUS,
            );
            cairo_context.fill().ok();
        }

        // Segment cuts every 10 dB (5 divisions inside a 60 dB range)
        cairo_context.set_source_rgba(0.06, 0.06, 0.06, 1.0);
        cairo_context.set_line_width(1.0);
        for cut_y in bar.segment_y_positions {
            let segment_line = horizontal_line_for_y(bar.track, cut_y);
            cairo_context.move_to(segment_line.x_start, segment_line.y_start);
            cairo_context.line_to(segment_line.x_end, segment_line.y_end);
            cairo_context.stroke().ok();
        }

        // Sticky peak tick
        if let Some(peak_y) = bar.peak_y {
            let peak_tick = peak_tick_rect(bar.track, peak_y);
            cairo_context.set_source_rgba(1.0, 1.0, 1.0, 0.9);
            cairo_context.rectangle(peak_tick.x, peak_tick.y, peak_tick.width, peak_tick.height);
            cairo_context.fill().ok();
        }
    }
}

pub(super) fn draw_peak_meter(
    cairo_context: &cairo::Context,
    meter_area: RectGeometry,
    peak_level: f32,
    peak_hold: f32,
) {
    let db_value = norm_to_db(peak_level);
    let db_hold = norm_to_db(peak_hold);

    // Numeric readout: "VAL / PEAK" label
    cairo_context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cairo_context.set_font_size(9.0);
    cairo_context.set_source_rgba(0.6, 0.6, 0.6, 1.0);
    cairo_context.move_to(meter_area.x, meter_area.y + 10.0);
    cairo_context.show_text("VAL / PEAK").ok();

    cairo_context.select_font_face(
        "monospace",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    cairo_context.set_font_size(15.0);

    let val_text = format!("{db_value:+.1}");
    let hold_text = format!("{db_hold:+.1} dB");
    let text_metrics = PeakMeterTextMetrics {
        value_advance: cairo_context
            .text_extents(&val_text)
            .map_or(0.0, |text_extents| text_extents.x_advance()),
        divider_advance: cairo_context
            .text_extents("|")
            .map_or(0.0, |text_extents| text_extents.x_advance()),
        hold_advance: cairo_context
            .text_extents(&hold_text)
            .map_or(0.0, |text_extents| text_extents.x_advance()),
        tick_label_widths: PEAK_METER_TICK_VALUES.map(|db| {
            cairo_context
                .text_extents(&db.to_string())
                .map_or(0.0, |text_extents| text_extents.width())
        }),
    };
    let geometry = peak_meter_geometry(meter_area, peak_level, peak_hold, text_metrics);

    cairo_context.set_source_rgba_tuple(level_color(db_value));
    cairo_context.move_to(
        geometry.value_position.x,
        geometry.value_position.baseline_y,
    );
    cairo_context.show_text(&val_text).ok();

    cairo_context.set_source_rgba(0.4, 0.4, 0.4, 1.0);
    cairo_context.move_to(
        geometry.divider_position.x,
        geometry.divider_position.baseline_y,
    );
    cairo_context.show_text("|").ok();

    cairo_context.set_source_rgba_tuple(level_color(db_hold));
    cairo_context.move_to(geometry.hold_position.x, geometry.hold_position.baseline_y);
    cairo_context.show_text(&hold_text).ok();

    let bg_g = zone_gradient_horizontal(
        horizontal_gradient_range_for_rect(geometry.track).x_start,
        horizontal_gradient_range_for_rect(geometry.track).x_end,
        0.2,
    );
    cairo_context.set_source(&bg_g).ok();
    rounded_rect(
        cairo_context,
        geometry.track.x,
        geometry.track.y,
        geometry.track.width,
        geometry.track.height,
        4.0,
    );
    cairo_context.fill().ok();

    if let Some(active) = geometry.active {
        let fg_g = zone_gradient_horizontal(
            horizontal_gradient_range_for_rect(geometry.track).x_start,
            horizontal_gradient_range_for_rect(geometry.track).x_end,
            1.0,
        );
        cairo_context.set_source(&fg_g).ok();
        rounded_rect(
            cairo_context,
            active.x,
            active.y,
            active.width,
            active.height,
            4.0,
        );
        cairo_context.fill().ok();
    }

    // Ruler: ticks + dB labels every 10 dB from −50 to −10.
    cairo_context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cairo_context.set_font_size(9.0);
    for tick in geometry.ticks {
        let tick_line = peak_meter_tick_line(geometry.track, tick.x);
        cairo_context.set_source_rgba(0.0, 0.0, 0.0, 0.5);
        cairo_context.move_to(tick_line.x_start, tick_line.y_start);
        cairo_context.line_to(tick_line.x_end, tick_line.y_end);
        cairo_context.stroke().ok();

        cairo_context.set_source_rgba(0.6, 0.6, 0.6, 0.8);
        cairo_context.move_to(tick.label_position.x, tick.label_position.baseline_y);
        cairo_context.show_text(&tick.db.to_string()).ok();
    }

    if let Some(hold_indicator) = geometry.hold_indicator {
        cairo_context.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        cairo_context.rectangle(
            hold_indicator.x,
            hold_indicator.y,
            hold_indicator.width,
            hold_indicator.height,
        );
        cairo_context.fill().ok();
    }
}

pub(super) fn draw_db_grid(cairo_context: &cairo::Context, bars_area: RectGeometry) {
    cairo_context.set_line_width(0.5);
    cairo_context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cairo_context.set_font_size(9.0);

    for line in decibel_grid_lines(bars_area) {
        let grid_line = horizontal_line_for_y(bars_area, line.y);
        cairo_context.set_source_rgba(1.0, 1.0, 1.0, 0.08);
        cairo_context.move_to(grid_line.x_start, grid_line.y_start);
        cairo_context.line_to(grid_line.x_end, grid_line.y_end);
        cairo_context.stroke().ok();

        if let Ok(ext) = cairo_context.text_extents(line.label) {
            let label_position = decibel_grid_label_position(bars_area, line.y, ext.width());
            cairo_context.set_source_rgba(0.6, 0.6, 0.6, 0.6);
            cairo_context.move_to(label_position.x, label_position.baseline_y);
            cairo_context.show_text(line.label).ok();
        }
    }
}

pub(super) fn draw_frequency_labels(cairo_context: &cairo::Context, labels: &[FrequencyLabel; 6]) {
    cairo_context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cairo_context.set_font_size(9.0);
    cairo_context.set_source_rgba(0.5, 0.5, 0.5, 0.9);

    for label in labels {
        if let Ok(ext) = cairo_context.text_extents(label.text) {
            let label_position =
                centered_text_position(label.center_x, label.baseline_y, ext.width());
            cairo_context.move_to(label_position.x, label_position.baseline_y);
            cairo_context.show_text(label.text).ok();
        }
    }
}

pub(super) fn vertical_gradient_range_for_rect(rect: RectGeometry) -> VerticalGradientRange {
    VerticalGradientRange {
        y_start: rect.y + rect.height,
        y_end: rect.y,
    }
}

pub(super) fn horizontal_gradient_range_for_rect(rect: RectGeometry) -> HorizontalGradientRange {
    HorizontalGradientRange {
        x_start: rect.x,
        x_end: rect.x + rect.width,
    }
}

pub(super) fn horizontal_line_for_y(rect: RectGeometry, y: f64) -> LineSegment {
    LineSegment {
        x_start: rect.x,
        y_start: y,
        x_end: rect.x + rect.width,
        y_end: y,
    }
}

pub(super) fn peak_tick_rect(track: RectGeometry, peak_y: f64) -> RectGeometry {
    RectGeometry {
        x: track.x,
        y: peak_y - 0.5,
        width: track.width,
        height: 1.5,
    }
}

pub(super) fn peak_meter_tick_line(track: RectGeometry, x: f64) -> LineSegment {
    LineSegment {
        x_start: x,
        y_start: track.y,
        x_end: x,
        y_end: track.y + track.height,
    }
}

pub(super) fn decibel_grid_label_position(
    bars_area: RectGeometry,
    line_y: f64,
    label_width: f64,
) -> TextPosition {
    TextPosition {
        x: bars_area.x + bars_area.width - label_width - 2.0,
        baseline_y: line_y - 2.0,
    }
}

pub(super) fn centered_text_position(
    center_x: f64,
    baseline_y: f64,
    text_width: f64,
) -> TextPosition {
    TextPosition {
        x: center_x - text_width / 2.0,
        baseline_y,
    }
}

pub(super) fn zone_gradient_vertical(
    y_start: f64,
    y_end: f64,
    alpha_mult: f64,
) -> cairo::LinearGradient {
    let g = cairo::LinearGradient::new(0.0, y_start, 0.0, y_end);
    apply_zone_stops(&g, alpha_mult);
    g
}

pub(super) fn zone_gradient_horizontal(
    x_start: f64,
    x_end: f64,
    alpha_mult: f64,
) -> cairo::LinearGradient {
    let g = cairo::LinearGradient::new(x_start, 0.0, x_end, 0.0);
    apply_zone_stops(&g, alpha_mult);
    g
}

/// Three colour zones mirroring pro peak meters: green → orange → red.
/// `alpha_mult` dims every stop so the same gradient doubles as a dark
/// "track" background.
pub(super) fn apply_zone_stops(g: &cairo::LinearGradient, alpha_mult: f64) {
    for stop in gradient_stops(alpha_mult) {
        g.add_color_stop_rgba(stop.offset, stop.red, stop.green, stop.blue, stop.alpha);
    }
}

pub(super) fn rounded_rect(cairo_context: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let rect = RectGeometry {
        x,
        y,
        width: w,
        height: h,
    };
    let Some(arcs) = rounded_corner_arcs(rect, r) else {
        cairo_context.rectangle(x, y, w, h);
        return;
    };

    cairo_context.new_sub_path();
    for arc in arcs {
        cairo_context.arc(
            arc.center_x,
            arc.center_y,
            arc.radius,
            arc.start_angle,
            arc.end_angle,
        );
    }
    cairo_context.close_path();
}

// Cairo context doesn't ship a tuple-taking `set_source_rgba`, so give
// ourselves one for the level colour helper.
pub(super) trait SetSourceRgbaTuple {
    fn set_source_rgba_tuple(&self, color: (f64, f64, f64, f64));
}

impl SetSourceRgbaTuple for cairo::Context {
    fn set_source_rgba_tuple(&self, (r, g, b, a): (f64, f64, f64, f64)) {
        self.set_source_rgba(r, g, b, a);
    }
}
