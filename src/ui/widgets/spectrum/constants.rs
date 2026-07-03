/// Number of bars rendered. Kept at 30 to match the Python widget so the
/// hard-coded frequency labels land on their expected columns.
pub const BAND_COUNT: usize = 30;

/// Drawing area height in logical pixels.
pub(super) const WIDGET_HEIGHT: i32 = 200;

/// Animation hz. 30 Hz keeps the bars smooth to the eye while halving
/// the redraw + interpolation cost compared to the previous 60 Hz timer.
pub(super) const ANIMATION_FPS: u32 = 30;
/// Catch-up rate per tick. Doubled from the 60 Hz version (0.45) so the
/// bars still reach a new target in roughly the same wall-clock time
/// at half the tick rate.
pub(super) const SMOOTH_FACTOR: f32 = 0.7;

/// Per-band peak behaviour. Tick budgets are scaled to 30 Hz so the
/// hold/decay timings stay close to the original feel.
pub(super) const PEAK_HOLD_TICKS: u16 = 20; // ≈ 0.7 s at 30 fps
pub(super) const PEAK_DECAY: f32 = 0.02;

/// Overall peak meter behaviour.
pub(super) const METER_HOLD_TICKS: u16 = 30; // ≈ 1 s
pub(super) const METER_PEAK_DECAY: f32 = 0.02;
pub(super) const METER_HOLD_DECAY: f32 = 0.01;

pub(super) const BAR_SPACING: f64 = 3.0;
pub(super) const CORNER_RADIUS: f64 = 2.0;
pub(super) const BG_RADIUS: f64 = 12.0;

pub(super) const DB_FLOOR: f32 = -60.0;
pub(super) const PEAK_METER_TICK_VALUES: [i32; 5] = [-50, -40, -30, -20, -10];
