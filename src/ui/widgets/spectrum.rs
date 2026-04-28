//! Premium spectrum analyser widget.
//!
//! Ported from the BigLinux Microphone Python legacy. Key design
//! decisions that give it the "award-winning" feel:
//!
//! * **30 bands, 3-zone gradient** — green (−60 … −20 dB), orange
//!   (−20 … −10 dB), red (−10 … 0 dB). A dark-shade version of the
//!   same gradient renders as a background track so empty bars stay
//!   readable and keep the zone hint visible.
//! * **Sticky per-band peaks** — each bar draws a thin peak tick that
//!   holds for ~0.7 s then decays linearly.
//! * **60 Hz smoothing loop** — audio-monitor frames arrive at ~94 Hz;
//!   the widget interpolates current → target at 45 % per frame so
//!   sliders/slider-like transients feel organic.
//! * **Horizontal peak meter** — VAL / PEAK numeric readout plus a
//!   bar with ruler marks every 10 dB and its own peak-hold indicator.
//! * **Segmented bars** — each column is cut every 10 dB, giving the
//!   classic LED-stack look without actually running many widgets.
//!
//! The widget owns its animation timer through an internal
//! `Rc<RefCell<State>>`, so multiple [`Spectrum::push_frame`] calls
//! only update `target_*` fields while the timer handles the rest.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use glib::SourceId;
use gtk::prelude::*;

use crate::services::audio_monitor::SpectrumFrame;

/// Number of bars rendered. Kept at 30 to match the Python widget so the
/// hard-coded frequency labels land on their expected columns.
pub const BAND_COUNT: usize = 30;

/// Drawing area height in logical pixels.
const WIDGET_HEIGHT: i32 = 200;

/// Animation hz. 30 Hz keeps the bars smooth to the eye while halving
/// the redraw + interpolation cost compared to the previous 60 Hz timer.
const ANIMATION_FPS: u32 = 30;
/// Catch-up rate per tick. Doubled from the 60 Hz version (0.45) so the
/// bars still reach a new target in roughly the same wall-clock time
/// at half the tick rate.
const SMOOTH_FACTOR: f32 = 0.7;

/// Per-band peak behaviour. Tick budgets are scaled to 30 Hz so the
/// hold/decay timings stay close to the original feel.
const PEAK_HOLD_TICKS: u16 = 20; // ≈ 0.7 s at 30 fps
const PEAK_DECAY: f32 = 0.02;

/// Overall peak meter behaviour.
const METER_HOLD_TICKS: u16 = 30; // ≈ 1 s
const METER_PEAK_DECAY: f32 = 0.02;
const METER_HOLD_DECAY: f32 = 0.01;

const BAR_SPACING: f64 = 3.0;
const CORNER_RADIUS: f64 = 2.0;
const BG_RADIUS: f64 = 12.0;

const DB_FLOOR: f32 = -60.0;

#[derive(Default)]
struct State {
    bands: [f32; BAND_COUNT],
    target_bands: [f32; BAND_COUNT],
    peaks: [f32; BAND_COUNT],
    band_peak_ticks: [u16; BAND_COUNT],

    /// Overall peak meter, normalised 0..=1.
    peak_level: f32,
    target_peak: f32,
    peak_hold: f32,
    meter_hold_ticks: u16,
}

/// Public handle. Hold one per window.
pub struct Spectrum {
    area: gtk::DrawingArea,
    state: Rc<RefCell<State>>,
    timer: Cell<Option<SourceId>>,
}

impl Spectrum {
    #[must_use]
    pub fn new() -> Rc<Self> {
        let area = gtk::DrawingArea::builder()
            .content_height(WIDGET_HEIGHT)
            .hexpand(true)
            .build();
        area.set_size_request(-1, WIDGET_HEIGHT);

        let state = Rc::new(RefCell::new(State::default()));

        // Draw callback reads the interpolated state.
        let draw_state = Rc::clone(&state);
        area.set_draw_func(move |_, ctx, w, h| {
            draw(ctx, w, h, &draw_state.borrow());
        });

        let widget = Rc::new(Self {
            area,
            state,
            timer: Cell::new(None),
        });
        widget.start_animation();
        widget
    }

    /// GTK widget handle for embedding in a container.
    #[must_use]
    pub fn widget(&self) -> &gtk::Widget {
        self.area.upcast_ref()
    }

    /// Push a new frame from the audio monitor. Only stores the target
    /// values — the 60 Hz timer drives the interpolation.
    pub fn push_frame(&self, frame: &SpectrumFrame) {
        let mut state = self.state.borrow_mut();

        // Resample the incoming band list to BAND_COUNT so the widget
        // can consume analysers with any output count without caring.
        let input = &frame.bands_db;
        if !input.is_empty() {
            for (i, slot) in state.target_bands.iter_mut().enumerate() {
                let src_idx = (i * input.len()) / BAND_COUNT;
                let src_idx = src_idx.min(input.len() - 1);
                *slot = db_to_norm(input[src_idx]);
            }
        }

        state.target_peak = db_to_norm(frame.peak_db);
    }

    /// Install the 60 Hz interpolation timer. Called once from `new`.
    fn start_animation(self: &Rc<Self>) {
        let period = Duration::from_millis(u64::from(1000 / ANIMATION_FPS.max(1)));
        let me = Rc::clone(self);
        let id = glib::timeout_add_local(period, move || {
            me.tick();
            glib::ControlFlow::Continue
        });
        self.timer.set(Some(id));
    }

    fn tick(&self) {
        // Skip the entire animation pass while the widget is off-screen
        // (different page in the view stack, window minimised, etc.).
        // The pw-cat capture path is paused in tandem from window.rs so
        // there is nothing meaningful to interpolate towards anyway.
        if !self.area.is_mapped() {
            return;
        }
        let mut state = self.state.borrow_mut();
        let mut changed = false;

        // ── Overall peak meter ───────────────────────────────────────
        if state.target_peak > state.peak_level {
            state.peak_level = state.target_peak;
            changed = true;
        } else {
            state.peak_level = (state.peak_level - METER_PEAK_DECAY).max(0.0);
            if state.peak_level > 0.01 {
                changed = true;
            }
        }

        if state.target_peak > state.peak_hold {
            state.peak_hold = state.target_peak;
            state.meter_hold_ticks = METER_HOLD_TICKS;
            changed = true;
        } else if state.meter_hold_ticks > 0 {
            state.meter_hold_ticks -= 1;
            changed = true;
        } else {
            state.peak_hold = (state.peak_hold - METER_HOLD_DECAY).max(0.0);
            if state.peak_hold > 0.01 {
                changed = true;
            }
        }

        // ── Per-band interpolation + peak hold ───────────────────────
        for i in 0..BAND_COUNT {
            let diff = state.target_bands[i] - state.bands[i];
            if diff.abs() > 0.001 {
                state.bands[i] += diff * SMOOTH_FACTOR;
                changed = true;
            }

            if state.bands[i] > state.peaks[i] {
                state.peaks[i] = state.bands[i];
                state.band_peak_ticks[i] = PEAK_HOLD_TICKS;
                changed = true;
            } else if state.band_peak_ticks[i] > 0 {
                state.band_peak_ticks[i] -= 1;
                changed = true;
            } else {
                state.peaks[i] = (state.peaks[i] - PEAK_DECAY).max(0.0);
            }
        }

        if changed || state.peaks.iter().any(|p| *p > 0.01) {
            self.area.queue_draw();
        }
    }
}

impl Drop for Spectrum {
    fn drop(&mut self) {
        if let Some(id) = self.timer.take() {
            id.remove();
        }
    }
}

// ── Rendering ────────────────────────────────────────────────────────

fn draw(ctx: &cairo::Context, width: i32, height: i32, state: &State) {
    // Almost-black backdrop with rounded corners.
    ctx.set_source_rgba(0.06, 0.06, 0.06, 1.0);
    rounded_rect(
        ctx,
        0.0,
        0.0,
        f64::from(width),
        f64::from(height),
        BG_RADIUS,
    );
    ctx.fill().ok();

    // Layout: peak meter on top, bars in the middle, frequency labels below.
    let padding = 12.0;
    let meter_height = 42.0;
    let freq_height = 18.0;

    let spectrum_y = padding + meter_height + 8.0;
    let spectrum_width = f64::from(width) - padding * 2.0;
    let spectrum_height = f64::from(height) - spectrum_y - freq_height - padding;

    draw_peak_meter(
        ctx,
        padding,
        padding,
        spectrum_width,
        meter_height,
        state.peak_level,
        state.peak_hold,
    );
    draw_bars(
        ctx,
        padding,
        spectrum_y,
        spectrum_width,
        spectrum_height,
        state,
    );
    draw_frequency_labels(
        ctx,
        padding,
        spectrum_y + spectrum_height + 4.0,
        spectrum_width,
    );
    draw_db_grid(ctx, padding, spectrum_y, spectrum_width, spectrum_height);
}

fn draw_bars(
    ctx: &cairo::Context,
    padding: f64,
    spectrum_y: f64,
    spectrum_width: f64,
    spectrum_height: f64,
    state: &State,
) {
    let total_spacing = BAR_SPACING * (BAND_COUNT as f64 - 1.0);
    let bar_width = ((spectrum_width - total_spacing) / BAND_COUNT as f64).max(2.0);

    let bg_gradient = zone_gradient_vertical(spectrum_y + spectrum_height, spectrum_y, 0.2);
    let fg_gradient = zone_gradient_vertical(spectrum_y + spectrum_height, spectrum_y, 1.0);

    for i in 0..BAND_COUNT {
        let x = padding + i as f64 * (bar_width + BAR_SPACING);
        let level = state.bands[i];
        let peak = state.peaks[i];

        // Dark track
        ctx.set_source(&bg_gradient).ok();
        rounded_rect(
            ctx,
            x,
            spectrum_y,
            bar_width,
            spectrum_height,
            CORNER_RADIUS,
        );
        ctx.fill().ok();

        // Active bar
        if level > 0.003 {
            let bar_h = (spectrum_height * f64::from(level)).max(2.0);
            let y = spectrum_y + spectrum_height - bar_h;
            ctx.set_source(&fg_gradient).ok();
            rounded_rect(ctx, x, y, bar_width, bar_h, CORNER_RADIUS);
            ctx.fill().ok();
        }

        // Segment cuts every 10 dB (5 divisions inside a 60 dB range)
        ctx.set_source_rgba(0.06, 0.06, 0.06, 1.0);
        ctx.set_line_width(1.0);
        for step in 1..6_i32 {
            let ratio = f64::from(step) / 6.0;
            let cut_y = spectrum_y + spectrum_height * (1.0 - ratio);
            ctx.move_to(x, cut_y);
            ctx.line_to(x + bar_width, cut_y);
            ctx.stroke().ok();
        }

        // Sticky peak tick
        if peak > 0.02 {
            let peak_y = spectrum_y + spectrum_height - (spectrum_height * f64::from(peak));
            ctx.set_source_rgba(1.0, 1.0, 1.0, 0.9);
            ctx.rectangle(x, peak_y - 0.5, bar_width, 1.5);
            ctx.fill().ok();
        }
    }
}

fn draw_peak_meter(
    ctx: &cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    _height: f64,
    peak_level: f32,
    peak_hold: f32,
) {
    let db_value = norm_to_db(peak_level);
    let db_hold = norm_to_db(peak_hold);

    // Numeric readout: "VAL / PEAK" label
    ctx.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    ctx.set_font_size(9.0);
    ctx.set_source_rgba(0.6, 0.6, 0.6, 1.0);
    ctx.move_to(x, y + 10.0);
    ctx.show_text("VAL / PEAK").ok();

    ctx.select_font_face(
        "monospace",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    ctx.set_font_size(15.0);

    let val_text = format!("{db_value:+.1}");
    ctx.set_source_rgba_tuple(level_color(db_value));
    ctx.move_to(x, y + 28.0);
    ctx.show_text(&val_text).ok();

    let val_ext = ctx.text_extents(&val_text).ok();
    let val_advance = val_ext.map_or(0.0, |e| e.x_advance());
    let div_x = x + val_advance + 5.0;
    ctx.set_source_rgba(0.4, 0.4, 0.4, 1.0);
    ctx.move_to(div_x, y + 28.0);
    ctx.show_text("|").ok();

    let div_ext = ctx.text_extents("|").ok();
    let div_advance = div_ext.map_or(0.0, |e| e.x_advance());
    let hold_x = div_x + div_advance + 5.0;
    let hold_text = format!("{db_hold:+.1} dB");
    ctx.set_source_rgba_tuple(level_color(db_hold));
    ctx.move_to(hold_x, y + 28.0);
    ctx.show_text(&hold_text).ok();

    // Meter bar — starts after the text block.
    let hold_ext = ctx.text_extents(&hold_text).ok();
    let hold_advance = hold_ext.map_or(0.0, |e| e.x_advance());
    let text_right = hold_x + hold_advance;
    let meter_x = text_right + 20.0;
    let meter_width = (x + width - meter_x - 5.0).max(40.0);
    let bar_h = 8.0;
    let bar_y = y + 12.0;

    let bg_g = zone_gradient_horizontal(meter_x, meter_x + meter_width, 0.2);
    ctx.set_source(&bg_g).ok();
    rounded_rect(ctx, meter_x, bar_y, meter_width, bar_h, 4.0);
    ctx.fill().ok();

    let active = (f64::from(peak_level) * meter_width).clamp(0.0, meter_width);
    if active > 1.0 {
        let fg_g = zone_gradient_horizontal(meter_x, meter_x + meter_width, 1.0);
        ctx.set_source(&fg_g).ok();
        rounded_rect(ctx, meter_x, bar_y, active, bar_h, 4.0);
        ctx.fill().ok();
    }

    // Ruler: ticks + dB labels every 10 dB from −50 to −10.
    ctx.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    ctx.set_font_size(9.0);
    for db in (-50..=-10).step_by(10) {
        let ratio = (f64::from(db) - f64::from(DB_FLOOR)) / 60.0;
        let tick_x = meter_x + ratio * meter_width;
        ctx.set_source_rgba(0.0, 0.0, 0.0, 0.5);
        ctx.move_to(tick_x, bar_y);
        ctx.line_to(tick_x, bar_y + bar_h);
        ctx.stroke().ok();

        let label = db.to_string();
        if let Ok(ext) = ctx.text_extents(&label) {
            ctx.set_source_rgba(0.6, 0.6, 0.6, 0.8);
            ctx.move_to(tick_x - ext.width() / 2.0, bar_y + bar_h + 10.0);
            ctx.show_text(&label).ok();
        }
    }

    // Peak hold indicator on the bar.
    if peak_hold > 0.01 {
        let hold_x_bar = meter_x + (f64::from(peak_hold) * meter_width).clamp(0.0, meter_width);
        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        ctx.rectangle(hold_x_bar - 1.0, y + 10.0, 2.0, 12.0);
        ctx.fill().ok();
    }
}

fn draw_db_grid(ctx: &cairo::Context, x: f64, y: f64, width: f64, height: f64) {
    ctx.set_line_width(0.5);
    ctx.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    ctx.set_font_size(9.0);

    for db in [-20, -40] {
        let ratio = (f64::from(db) - f64::from(DB_FLOOR)) / 60.0;
        let line_y = y + height * (1.0 - ratio);

        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.08);
        ctx.move_to(x, line_y);
        ctx.line_to(x + width, line_y);
        ctx.stroke().ok();

        let label = db.to_string();
        if let Ok(ext) = ctx.text_extents(&label) {
            ctx.set_source_rgba(0.6, 0.6, 0.6, 0.6);
            ctx.move_to(x + width - ext.width() - 2.0, line_y - 2.0);
            ctx.show_text(&label).ok();
        }
    }
}

fn draw_frequency_labels(ctx: &cairo::Context, x: f64, y: f64, width: f64) {
    // Index/label pairs calibrated against the legacy Python widget so
    // translations stay consistent with the old screenshots.
    const MARKERS: &[(usize, &str)] = &[
        (5, "63 Hz"),
        (10, "180 Hz"),
        (15, "500 Hz"),
        (20, "1.5 kHz"),
        (25, "4 kHz"),
        (29, "9.5 kHz"),
    ];

    ctx.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    ctx.set_font_size(9.0);
    ctx.set_source_rgba(0.5, 0.5, 0.5, 0.9);

    let total_spacing = BAR_SPACING * (BAND_COUNT as f64 - 1.0);
    let bar_width = (width - total_spacing) / BAND_COUNT as f64;

    for (band_idx, label) in MARKERS {
        let band_x = x + *band_idx as f64 * (bar_width + BAR_SPACING) + bar_width / 2.0;
        if let Ok(ext) = ctx.text_extents(label) {
            ctx.move_to(band_x - ext.width() / 2.0, y + 12.0);
            ctx.show_text(label).ok();
        }
    }
}

// ── Shared helpers ───────────────────────────────────────────────────

fn zone_gradient_vertical(y_start: f64, y_end: f64, alpha_mult: f64) -> cairo::LinearGradient {
    let g = cairo::LinearGradient::new(0.0, y_start, 0.0, y_end);
    apply_zone_stops(&g, alpha_mult);
    g
}

fn zone_gradient_horizontal(x_start: f64, x_end: f64, alpha_mult: f64) -> cairo::LinearGradient {
    let g = cairo::LinearGradient::new(x_start, 0.0, x_end, 0.0);
    apply_zone_stops(&g, alpha_mult);
    g
}

/// Three colour zones mirroring pro peak meters: green → orange → red.
/// `alpha_mult` dims every stop so the same gradient doubles as a dark
/// "track" background.
fn apply_zone_stops(g: &cairo::LinearGradient, alpha_mult: f64) {
    // Green: 0 .. 0.667  (−60 .. −20 dB)
    g.add_color_stop_rgba(0.0, 0.0, 0.6 * alpha_mult, 0.0, 1.0);
    g.add_color_stop_rgba(0.667, 0.0, 0.6 * alpha_mult, 0.0, 1.0);
    // Orange: 0.667 .. 0.833  (−20 .. −10 dB)
    g.add_color_stop_rgba(0.6671, 1.0 * alpha_mult, 0.6 * alpha_mult, 0.0, 1.0);
    g.add_color_stop_rgba(0.833, 1.0 * alpha_mult, 0.6 * alpha_mult, 0.0, 1.0);
    // Red: 0.833 .. 1.0  (−10 .. 0 dB)
    g.add_color_stop_rgba(0.8331, 1.0 * alpha_mult, 0.0, 0.0, 1.0);
    g.add_color_stop_rgba(1.0, 1.0 * alpha_mult, 0.0, 0.0, 1.0);
}

fn level_color(db: f32) -> (f64, f64, f64, f64) {
    if db > -3.0 {
        (1.0, 0.2, 0.2, 1.0)
    } else if db > -10.0 {
        (1.0, 0.7, 0.0, 1.0)
    } else {
        (0.2, 0.7, 0.2, 1.0)
    }
}

fn db_to_norm(db: f32) -> f32 {
    ((db - DB_FLOOR) / 60.0).clamp(0.0, 1.0)
}

fn norm_to_db(norm: f32) -> f32 {
    (DB_FLOOR + norm * 60.0).clamp(DB_FLOOR, 0.0)
}

fn rounded_rect(ctx: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    if r < 1.0 {
        ctx.rectangle(x, y, w, h);
        return;
    }
    ctx.new_sub_path();
    let pi = std::f64::consts::PI;
    ctx.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    ctx.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    ctx.arc(x + r, y + h - r, r, pi / 2.0, pi);
    ctx.arc(x + r, y + r, r, pi, 3.0 * pi / 2.0);
    ctx.close_path();
}

// Cairo context doesn't ship a tuple-taking `set_source_rgba`, so give
// ourselves one for the level colour helper.
trait SetSourceRgbaTuple {
    fn set_source_rgba_tuple(&self, color: (f64, f64, f64, f64));
}

impl SetSourceRgbaTuple for cairo::Context {
    fn set_source_rgba_tuple(&self, (r, g, b, a): (f64, f64, f64, f64)) {
        self.set_source_rgba(r, g, b, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(level_color(-6.0), (1.0, 0.7, 0.0, 1.0)); // orange
        assert_eq!(level_color(-1.0), (1.0, 0.2, 0.2, 1.0)); // red
    }
}
