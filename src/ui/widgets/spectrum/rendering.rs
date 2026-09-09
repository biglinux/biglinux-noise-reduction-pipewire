//! Theme-aware spectrum bars; all text is rendered by native GTK labels.
use super::constants::{BAND_COUNT, BAR_SPACING, CORNER_RADIUS};
use super::state::SpectrumState;

pub(super) fn draw(
    context: &cairo::Context,
    width: i32,
    height: i32,
    state: &SpectrumState,
    foreground: (f64, f64, f64),
) {
    let width = f64::from(width).max(0.0);
    let height = f64::from(height).max(0.0);
    if width == 0.0 || height == 0.0 {
        return;
    }
    // Never overflow a narrow allocation. GTK owns the background and colors.
    let spacing = BAR_SPACING.min(width / (BAND_COUNT * 2) as f64);
    let bar_width = ((width - spacing * (BAND_COUNT - 1) as f64) / BAND_COUNT as f64).max(0.0);
    let (red, green, blue) = foreground;
    for index in 0..BAND_COUNT {
        let x = index as f64 * (bar_width + spacing);
        context.set_source_rgba(red, green, blue, 0.12);
        rounded_rect(context, x, 0.0, bar_width, height, CORNER_RADIUS);
        let _ = context.fill();
        let level = f64::from(state.bands[index]).clamp(0.0, 1.0);
        if level > 0.0 {
            context.set_source_rgba(red, green, blue, 0.85);
            let filled = height * level;
            rounded_rect(
                context,
                x,
                height - filled,
                bar_width,
                filled,
                CORNER_RADIUS,
            );
            let _ = context.fill();
        }
        let peak = f64::from(state.peaks[index]).clamp(0.0, 1.0);
        if peak > 0.0 {
            context.set_source_rgba(red, green, blue, 1.0);
            context.rectangle(
                x,
                (height * (1.0 - peak)).clamp(0.0, height - 1.0),
                bar_width,
                1.0,
            );
            let _ = context.fill();
        }
    }
}

fn rounded_rect(context: &cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    if radius < 1.0 {
        context.rectangle(x, y, width, height);
        return;
    }
    let pi = std::f64::consts::PI;
    context.new_sub_path();
    context.arc(x + width - radius, y + radius, radius, -pi / 2.0, 0.0);
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        pi / 2.0,
    );
    context.arc(x + radius, y + height - radius, radius, pi / 2.0, pi);
    context.arc(x + radius, y + radius, radius, pi, 3.0 * pi / 2.0);
    context.close_path();
}
