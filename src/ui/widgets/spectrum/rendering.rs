use super::constants::{
    BAND_COUNT, BAR_SPACING, BG_RADIUS, CORNER_RADIUS, DB_FLOOR, DB_SPAN, PEAK_METER_TICK_VALUES,
};
use super::state::SpectrumState;

pub(super) fn draw(
    cairo_context: &cairo::Context,
    width: i32,
    height: i32,
    state: &SpectrumState,
    peak_meter_caption: &str,
) {
    let padding = 12.0;
    let meter_height = 42.0;
    let labels_height = 18.0;
    let bars_y = padding + meter_height + 8.0;
    let content_width = (f64::from(width) - padding * 2.0).max(1.0);
    let bars_height = (f64::from(height) - bars_y - labels_height - padding).max(1.0);

    cairo_context.set_source_rgba(0.06, 0.06, 0.06, 1.0);
    rounded_rect(
        cairo_context,
        0.0,
        0.0,
        f64::from(width).max(1.0),
        f64::from(height).max(1.0),
        BG_RADIUS,
    );
    cairo_context.fill().ok();

    draw_peak_meter(
        cairo_context,
        padding,
        padding,
        content_width,
        state.peak_level,
        state.peak_hold,
        peak_meter_caption,
    );
    draw_bars(
        cairo_context,
        padding,
        bars_y,
        content_width,
        bars_height,
        state,
    );
    draw_frequency_labels(
        cairo_context,
        padding,
        content_width,
        bars_y + bars_height + 4.0,
    );
    draw_db_grid(cairo_context, padding, bars_y, content_width, bars_height);
}

fn draw_bars(
    cairo_context: &cairo::Context,
    area_x: f64,
    area_y: f64,
    area_width: f64,
    area_height: f64,
    state: &SpectrumState,
) {
    let background_gradient = zone_gradient_vertical(area_y + area_height, area_y, 0.2);
    let foreground_gradient = zone_gradient_vertical(area_y + area_height, area_y, 1.0);
    let bar_width = bar_width(area_width);

    for band_index in 0..BAND_COUNT {
        let x = area_x + band_index as f64 * (bar_width + BAR_SPACING);

        cairo_context.set_source(&background_gradient).ok();
        rounded_rect(
            cairo_context,
            x,
            area_y,
            bar_width,
            area_height,
            CORNER_RADIUS,
        );
        cairo_context.fill().ok();

        let level = state.bands[band_index];
        if level > 0.003 {
            let active_height = (area_height * f64::from(level)).max(2.0);
            cairo_context.set_source(&foreground_gradient).ok();
            rounded_rect(
                cairo_context,
                x,
                area_y + area_height - active_height,
                bar_width,
                active_height,
                CORNER_RADIUS,
            );
            cairo_context.fill().ok();
        }

        cairo_context.set_source_rgba(0.06, 0.06, 0.06, 1.0);
        cairo_context.set_line_width(1.0);
        for step in 1..6 {
            let cut_y = area_y + area_height * (1.0 - f64::from(step) / 6.0);
            cairo_context.move_to(x, cut_y);
            cairo_context.line_to(x + bar_width, cut_y);
            cairo_context.stroke().ok();
        }

        let peak = state.peaks[band_index];
        if peak > 0.02 {
            let peak_y = area_y + area_height - area_height * f64::from(peak);
            cairo_context.set_source_rgba(1.0, 1.0, 1.0, 0.9);
            cairo_context.rectangle(x, peak_y - 0.5, bar_width, 1.5);
            cairo_context.fill().ok();
        }
    }
}

fn draw_peak_meter(
    cairo_context: &cairo::Context,
    area_x: f64,
    area_y: f64,
    area_width: f64,
    peak_level: f32,
    peak_hold: f32,
    caption: &str,
) {
    let db_value = norm_to_db(peak_level);
    let db_hold = norm_to_db(peak_hold);

    cairo_context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cairo_context.set_font_size(9.0);
    cairo_context.set_source_rgba(0.6, 0.6, 0.6, 1.0);
    cairo_context.move_to(area_x, area_y + 10.0);
    cairo_context.show_text(caption).ok();

    cairo_context.select_font_face(
        "monospace",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    cairo_context.set_font_size(15.0);

    let value_text = format!("{db_value:+.1}");
    let hold_text = format!("{db_hold:+.1} dB");
    let value_advance = cairo_context
        .text_extents(&value_text)
        .map_or(0.0, |extents| extents.x_advance());
    let divider_x = area_x + value_advance + 5.0;
    let divider_advance = cairo_context
        .text_extents("|")
        .map_or(0.0, |extents| extents.x_advance());
    let hold_x = divider_x + divider_advance + 5.0;
    let hold_advance = cairo_context
        .text_extents(&hold_text)
        .map_or(0.0, |extents| extents.x_advance());
    let tick_label_widths = PEAK_METER_TICK_VALUES.map(|db| {
        cairo_context
            .text_extents(&db.to_string())
            .map_or(0.0, |extents| extents.width())
    });
    let text_baseline = area_y + 28.0;
    let track_x = hold_x + hold_advance + 20.0;
    let track_y = area_y + 12.0;
    let track_width = (area_x + area_width - track_x - 5.0).max(40.0);
    let track_height = 8.0;

    set_level_color(cairo_context, db_value);
    cairo_context.move_to(area_x, text_baseline);
    cairo_context.show_text(&value_text).ok();

    cairo_context.set_source_rgba(0.4, 0.4, 0.4, 1.0);
    cairo_context.move_to(divider_x, text_baseline);
    cairo_context.show_text("|").ok();

    set_level_color(cairo_context, db_hold);
    cairo_context.move_to(hold_x, text_baseline);
    cairo_context.show_text(&hold_text).ok();

    cairo_context
        .set_source(zone_gradient_horizontal(
            track_x,
            track_x + track_width,
            0.2,
        ))
        .ok();
    rounded_rect(
        cairo_context,
        track_x,
        track_y,
        track_width,
        track_height,
        4.0,
    );
    cairo_context.fill().ok();

    let active_width = (f64::from(peak_level) * track_width).clamp(0.0, track_width);
    if active_width > 1.0 {
        cairo_context
            .set_source(zone_gradient_horizontal(
                track_x,
                track_x + track_width,
                1.0,
            ))
            .ok();
        rounded_rect(
            cairo_context,
            track_x,
            track_y,
            active_width,
            track_height,
            4.0,
        );
        cairo_context.fill().ok();
    }

    cairo_context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cairo_context.set_font_size(9.0);
    for (tick_index, db) in PEAK_METER_TICK_VALUES.into_iter().enumerate() {
        let ratio = (f64::from(db) - f64::from(DB_FLOOR)) / f64::from(DB_SPAN);
        let x = track_x + ratio * track_width;
        cairo_context.set_source_rgba(0.0, 0.0, 0.0, 0.5);
        cairo_context.move_to(x, track_y);
        cairo_context.line_to(x, track_y + track_height);
        cairo_context.stroke().ok();

        let label = db.to_string();
        cairo_context.set_source_rgba(0.6, 0.6, 0.6, 0.8);
        cairo_context.move_to(
            x - tick_label_widths[tick_index] / 2.0,
            track_y + track_height + 10.0,
        );
        cairo_context.show_text(&label).ok();
    }

    if peak_hold > 0.01 {
        let hold_x = track_x + (f64::from(peak_hold) * track_width).clamp(0.0, track_width) - 1.0;
        cairo_context.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        cairo_context.rectangle(hold_x, area_y + 10.0, 2.0, 12.0);
        cairo_context.fill().ok();
    }
}

fn draw_db_grid(
    cairo_context: &cairo::Context,
    area_x: f64,
    area_y: f64,
    area_width: f64,
    area_height: f64,
) {
    cairo_context.set_line_width(0.5);
    cairo_context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cairo_context.set_font_size(9.0);

    for (label, db) in [("-20", -20.0), ("-40", -40.0)] {
        let ratio = (db - f64::from(DB_FLOOR)) / f64::from(DB_SPAN);
        let y = area_y + area_height * (1.0 - ratio);
        cairo_context.set_source_rgba(1.0, 1.0, 1.0, 0.08);
        cairo_context.move_to(area_x, y);
        cairo_context.line_to(area_x + area_width, y);
        cairo_context.stroke().ok();

        if let Ok(extents) = cairo_context.text_extents(label) {
            cairo_context.set_source_rgba(0.6, 0.6, 0.6, 0.6);
            cairo_context.move_to(area_x + area_width - extents.width() - 2.0, y - 2.0);
            cairo_context.show_text(label).ok();
        }
    }
}

fn draw_frequency_labels(
    cairo_context: &cairo::Context,
    area_x: f64,
    area_width: f64,
    labels_y: f64,
) {
    const MARKERS: [(usize, &str); 6] = [
        (5, "63 Hz"),
        (10, "180 Hz"),
        (15, "500 Hz"),
        (20, "1.5 kHz"),
        (25, "4 kHz"),
        (29, "9.5 kHz"),
    ];

    cairo_context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cairo_context.set_font_size(9.0);
    cairo_context.set_source_rgba(0.5, 0.5, 0.5, 0.9);
    let bar_width = bar_width(area_width);

    for (band_index, label) in MARKERS {
        if let Ok(extents) = cairo_context.text_extents(label) {
            let center_x = area_x + band_index as f64 * (bar_width + BAR_SPACING) + bar_width / 2.0;
            cairo_context.move_to(center_x - extents.width() / 2.0, labels_y + 12.0);
            cairo_context.show_text(label).ok();
        }
    }
}

fn norm_to_db(norm: f32) -> f32 {
    (DB_FLOOR + norm * DB_SPAN).clamp(DB_FLOOR, 0.0)
}

/// Width of one band's bar. The bars and the frequency labels underneath
/// have to land on the same grid, and each computed it separately.
fn bar_width(area_width: f64) -> f64 {
    let total_spacing = BAR_SPACING * (BAND_COUNT as f64 - 1.0);
    ((area_width - total_spacing) / BAND_COUNT as f64).max(2.0)
}

fn set_level_color(cairo_context: &cairo::Context, db: f32) {
    let color = if db > -3.0 {
        (1.0, 0.2, 0.2)
    } else if db > -10.0 {
        (1.0, 0.7, 0.0)
    } else {
        (0.2, 0.7, 0.2)
    };
    cairo_context.set_source_rgba(color.0, color.1, color.2, 1.0);
}

fn zone_gradient_vertical(y_start: f64, y_end: f64, intensity: f64) -> cairo::LinearGradient {
    let gradient = cairo::LinearGradient::new(0.0, y_start, 0.0, y_end);
    add_zone_stops(&gradient, intensity);
    gradient
}

fn zone_gradient_horizontal(x_start: f64, x_end: f64, intensity: f64) -> cairo::LinearGradient {
    let gradient = cairo::LinearGradient::new(x_start, 0.0, x_end, 0.0);
    add_zone_stops(&gradient, intensity);
    gradient
}

fn add_zone_stops(gradient: &cairo::LinearGradient, intensity: f64) {
    gradient.add_color_stop_rgba(0.0, 0.0, 0.6 * intensity, 0.0, 1.0);
    gradient.add_color_stop_rgba(0.667, 0.0, 0.6 * intensity, 0.0, 1.0);
    gradient.add_color_stop_rgba(0.6671, intensity, 0.6 * intensity, 0.0, 1.0);
    gradient.add_color_stop_rgba(0.833, intensity, 0.6 * intensity, 0.0, 1.0);
    gradient.add_color_stop_rgba(0.8331, intensity, 0.0, 0.0, 1.0);
    gradient.add_color_stop_rgba(1.0, intensity, 0.0, 0.0, 1.0);
}

fn rounded_rect(
    cairo_context: &cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    if radius < 1.0 {
        cairo_context.rectangle(x, y, width, height);
        return;
    }

    let pi = std::f64::consts::PI;
    cairo_context.new_sub_path();
    cairo_context.arc(x + width - radius, y + radius, radius, -pi / 2.0, 0.0);
    cairo_context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        pi / 2.0,
    );
    cairo_context.arc(x + radius, y + height - radius, radius, pi / 2.0, pi);
    cairo_context.arc(x + radius, y + radius, radius, pi, 3.0 * pi / 2.0);
    cairo_context.close_path();
}
