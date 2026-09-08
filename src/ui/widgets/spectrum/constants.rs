/// Number of bars rendered. The 30-band layout places the calibrated
/// frequency labels on their designated columns.
pub const BAND_COUNT: usize = 30;

/// Drawing area height in logical pixels.
pub(super) const WIDGET_HEIGHT: i32 = 120;

/// Animation rate. 30 Hz keeps the bars smooth while bounding redraw and
/// interpolation work.
pub(super) const ANIMATION_FPS: u32 = 30;
/// Catch-up rate per tick. At 30 Hz, 0.7 reaches a new target quickly while
/// retaining visible interpolation between audio frames.
pub(super) const SMOOTH_FACTOR: f32 = 0.7;

/// Per-band peak behaviour. Tick budgets at 30 Hz provide the intended
/// hold and decay timings.
pub(super) const PEAK_HOLD_TICKS: u16 = 20; // ≈ 0.7 s at 30 fps
pub(super) const PEAK_DECAY: f32 = 0.02;

/// Overall peak meter behaviour.
pub(super) const METER_HOLD_TICKS: u16 = 30; // ≈ 1 s
pub(super) const METER_PEAK_DECAY: f32 = 0.02;
pub(super) const METER_HOLD_DECAY: f32 = 0.01;

pub(super) const BAR_SPACING: f64 = 3.0;
pub(super) const CORNER_RADIUS: f64 = 2.0;

pub(super) const DB_FLOOR: f32 = -60.0;
/// Height of the displayed range, floor to 0 dBFS.
///
/// Derived rather than written out: the span was spelled `60.0` at four
/// separate sites, so retuning `DB_FLOOR` rescaled the bars while leaving
/// the peak-meter ticks and the grid labels on the old range — the labels
/// stop annotating the bars they sit next to.
pub(super) const DB_SPAN: f32 = -DB_FLOOR;
