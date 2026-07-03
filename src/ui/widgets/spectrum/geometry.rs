use super::constants::{BAND_COUNT, BAR_SPACING, DB_FLOOR, PEAK_METER_TICK_VALUES};
use super::state::SpectrumState;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RectGeometry {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SpectrumLayout {
    pub(super) background: RectGeometry,
    pub(super) peak_meter: RectGeometry,
    pub(super) bars: RectGeometry,
    pub(super) frequency_labels_y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BarGeometry {
    pub(super) track: RectGeometry,
    pub(super) active_bar: Option<RectGeometry>,
    pub(super) peak_y: Option<f64>,
    pub(super) segment_y_positions: [f64; 5],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DecibelGridLine {
    pub(super) label: &'static str,
    pub(super) y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FrequencyLabel {
    pub(super) text: &'static str,
    pub(super) center_x: f64,
    pub(super) baseline_y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GradientStop {
    pub(super) offset: f64,
    pub(super) red: f64,
    pub(super) green: f64,
    pub(super) blue: f64,
    pub(super) alpha: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TextPosition {
    pub(super) x: f64,
    pub(super) baseline_y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PeakMeterTextMetrics {
    pub(super) value_advance: f64,
    pub(super) divider_advance: f64,
    pub(super) hold_advance: f64,
    pub(super) tick_label_widths: [f64; PEAK_METER_TICK_VALUES.len()],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PeakMeterTick {
    pub(super) db: i32,
    pub(super) x: f64,
    pub(super) label_position: TextPosition,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PeakMeterGeometry {
    pub(super) value_position: TextPosition,
    pub(super) divider_position: TextPosition,
    pub(super) hold_position: TextPosition,
    pub(super) track: RectGeometry,
    pub(super) active: Option<RectGeometry>,
    pub(super) ticks: [PeakMeterTick; PEAK_METER_TICK_VALUES.len()],
    pub(super) hold_indicator: Option<RectGeometry>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ArcGeometry {
    pub(super) center_x: f64,
    pub(super) center_y: f64,
    pub(super) radius: f64,
    pub(super) start_angle: f64,
    pub(super) end_angle: f64,
}

pub(super) fn spectrum_layout(width: i32, height: i32) -> SpectrumLayout {
    // Layout: peak meter on top, bars in the middle, frequency labels below.
    let padding = 12.0;
    let meter_height = 42.0;
    let frequency_labels_height = 18.0;
    let bars_y = padding + meter_height + 8.0;
    let content_width = (f64::from(width) - padding * 2.0).max(1.0);
    let bars_height = (f64::from(height) - bars_y - frequency_labels_height - padding).max(1.0);

    SpectrumLayout {
        background: RectGeometry {
            x: 0.0,
            y: 0.0,
            width: f64::from(width).max(1.0),
            height: f64::from(height).max(1.0),
        },
        peak_meter: RectGeometry {
            x: padding,
            y: padding,
            width: content_width,
            height: meter_height,
        },
        bars: RectGeometry {
            x: padding,
            y: bars_y,
            width: content_width,
            height: bars_height,
        },
        frequency_labels_y: bars_y + bars_height + 4.0,
    }
}

pub(super) fn resampled_band_targets(input_bands_db: &[f32]) -> [f32; BAND_COUNT] {
    let mut targets = [0.0; BAND_COUNT];
    if input_bands_db.is_empty() {
        return targets;
    }

    for (band_index, target) in targets.iter_mut().enumerate() {
        let source_index = (band_index * input_bands_db.len()) / BAND_COUNT;
        *target = db_to_norm(input_bands_db[source_index]);
    }
    targets
}

pub(super) fn bar_width(area_width: f64) -> f64 {
    let total_spacing = BAR_SPACING * (BAND_COUNT as f64 - 1.0);
    ((area_width - total_spacing) / BAND_COUNT as f64).max(2.0)
}

pub(super) fn bar_geometries(
    bars_area: RectGeometry,
    state: &SpectrumState,
) -> [BarGeometry; BAND_COUNT] {
    let single_bar_width = bar_width(bars_area.width);
    std::array::from_fn(|band_index| {
        let x = bars_area.x + band_index as f64 * (single_bar_width + BAR_SPACING);
        let level = state.bands[band_index];
        let peak = state.peaks[band_index];

        BarGeometry {
            track: RectGeometry {
                x,
                y: bars_area.y,
                width: single_bar_width,
                height: bars_area.height,
            },
            active_bar: active_bar_geometry(x, single_bar_width, bars_area, level),
            peak_y: peak_tick_y(bars_area, peak),
            segment_y_positions: segment_y_positions(bars_area),
        }
    })
}

pub(super) fn active_bar_geometry(
    x: f64,
    width: f64,
    bars_area: RectGeometry,
    level: f32,
) -> Option<RectGeometry> {
    if level <= 0.003 {
        return None;
    }
    let height = (bars_area.height * f64::from(level)).max(2.0);
    Some(RectGeometry {
        x,
        y: bars_area.y + bars_area.height - height,
        width,
        height,
    })
}

pub(super) fn peak_tick_y(bars_area: RectGeometry, peak: f32) -> Option<f64> {
    (peak > 0.02).then(|| bars_area.y + bars_area.height - (bars_area.height * f64::from(peak)))
}

pub(super) fn segment_y_positions(bars_area: RectGeometry) -> [f64; 5] {
    std::array::from_fn(|segment_index| {
        let step = segment_index + 1;
        let ratio = step as f64 / 6.0;
        bars_area.y + bars_area.height * (1.0 - ratio)
    })
}

pub(super) fn decibel_grid_lines(bars_area: RectGeometry) -> [DecibelGridLine; 2] {
    [
        DecibelGridLine {
            label: "-20",
            y: decibel_y(bars_area, -20.0),
        },
        DecibelGridLine {
            label: "-40",
            y: decibel_y(bars_area, -40.0),
        },
    ]
}

pub(super) fn decibel_y(bars_area: RectGeometry, db: f32) -> f64 {
    let ratio = (f64::from(db) - f64::from(DB_FLOOR)) / 60.0;
    bars_area.y + bars_area.height * (1.0 - ratio)
}

pub(super) fn frequency_labels(layout: SpectrumLayout) -> [FrequencyLabel; 6] {
    // Index/label pairs calibrated against the legacy Python widget so
    // translations stay consistent with the old screenshots.
    const MARKERS: [(usize, &str); 6] = [
        (5, "63 Hz"),
        (10, "180 Hz"),
        (15, "500 Hz"),
        (20, "1.5 kHz"),
        (25, "4 kHz"),
        (29, "9.5 kHz"),
    ];

    let single_bar_width = bar_width(layout.bars.width);
    MARKERS.map(|(band_index, text)| FrequencyLabel {
        text,
        center_x: layout.bars.x
            + band_index as f64 * (single_bar_width + BAR_SPACING)
            + single_bar_width / 2.0,
        baseline_y: layout.frequency_labels_y + 12.0,
    })
}

pub(super) fn peak_meter_geometry(
    meter_area: RectGeometry,
    peak_level: f32,
    peak_hold: f32,
    text_metrics: PeakMeterTextMetrics,
) -> PeakMeterGeometry {
    let value_position = TextPosition {
        x: meter_area.x,
        baseline_y: meter_area.y + 28.0,
    };
    let divider_position = TextPosition {
        x: value_position.x + text_metrics.value_advance + 5.0,
        baseline_y: value_position.baseline_y,
    };
    let hold_position = TextPosition {
        x: divider_position.x + text_metrics.divider_advance + 5.0,
        baseline_y: value_position.baseline_y,
    };
    let text_right = hold_position.x + text_metrics.hold_advance;
    let track_x = text_right + 20.0;
    let track = RectGeometry {
        x: track_x,
        y: meter_area.y + 12.0,
        width: (meter_area.x + meter_area.width - track_x - 5.0).max(40.0),
        height: 8.0,
    };
    let active_width = (f64::from(peak_level) * track.width).clamp(0.0, track.width);
    let active = (active_width > 1.0).then_some(RectGeometry {
        x: track.x,
        y: track.y,
        width: active_width,
        height: track.height,
    });
    let ticks = std::array::from_fn(|tick_index| {
        let db = PEAK_METER_TICK_VALUES[tick_index];
        let ratio = (f64::from(db) - f64::from(DB_FLOOR)) / 60.0;
        let x = track.x + ratio * track.width;
        PeakMeterTick {
            db,
            x,
            label_position: TextPosition {
                x: x - text_metrics.tick_label_widths[tick_index] / 2.0,
                baseline_y: track.y + track.height + 10.0,
            },
        }
    });
    let hold_indicator = (peak_hold > 0.01).then_some(RectGeometry {
        x: track.x + (f64::from(peak_hold) * track.width).clamp(0.0, track.width) - 1.0,
        y: meter_area.y + 10.0,
        width: 2.0,
        height: 12.0,
    });

    PeakMeterGeometry {
        value_position,
        divider_position,
        hold_position,
        track,
        active,
        ticks,
        hold_indicator,
    }
}

pub(super) fn gradient_stops(alpha_mult: f64) -> [GradientStop; 6] {
    [
        // Green: 0 .. 0.667  (-60 .. -20 dB)
        GradientStop {
            offset: 0.0,
            red: 0.0,
            green: 0.6 * alpha_mult,
            blue: 0.0,
            alpha: 1.0,
        },
        GradientStop {
            offset: 0.667,
            red: 0.0,
            green: 0.6 * alpha_mult,
            blue: 0.0,
            alpha: 1.0,
        },
        // Orange: 0.667 .. 0.833  (-20 .. -10 dB)
        GradientStop {
            offset: 0.6671,
            red: alpha_mult,
            green: 0.6 * alpha_mult,
            blue: 0.0,
            alpha: 1.0,
        },
        GradientStop {
            offset: 0.833,
            red: alpha_mult,
            green: 0.6 * alpha_mult,
            blue: 0.0,
            alpha: 1.0,
        },
        // Red: 0.833 .. 1.0  (-10 .. 0 dB)
        GradientStop {
            offset: 0.8331,
            red: alpha_mult,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
        },
        GradientStop {
            offset: 1.0,
            red: alpha_mult,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
        },
    ]
}

pub(super) fn clamped_corner_radius(width: f64, height: f64, radius: f64) -> f64 {
    radius.min(width / 2.0).min(height / 2.0)
}

pub(super) fn level_color(db: f32) -> (f64, f64, f64, f64) {
    if db > -3.0 {
        (1.0, 0.2, 0.2, 1.0)
    } else if db > -10.0 {
        (1.0, 0.7, 0.0, 1.0)
    } else {
        (0.2, 0.7, 0.2, 1.0)
    }
}

pub(super) fn db_to_norm(db: f32) -> f32 {
    ((db - DB_FLOOR) / 60.0).clamp(0.0, 1.0)
}

pub(super) fn norm_to_db(norm: f32) -> f32 {
    (DB_FLOOR + norm * 60.0).clamp(DB_FLOOR, 0.0)
}

pub(super) fn rounded_corner_arcs(rect: RectGeometry, radius: f64) -> Option<[ArcGeometry; 4]> {
    let radius = clamped_corner_radius(rect.width, rect.height, radius);
    if radius < 1.0 {
        return None;
    }

    let pi = std::f64::consts::PI;
    Some([
        ArcGeometry {
            center_x: rect.x + rect.width - radius,
            center_y: rect.y + radius,
            radius,
            start_angle: -pi / 2.0,
            end_angle: 0.0,
        },
        ArcGeometry {
            center_x: rect.x + rect.width - radius,
            center_y: rect.y + rect.height - radius,
            radius,
            start_angle: 0.0,
            end_angle: pi / 2.0,
        },
        ArcGeometry {
            center_x: rect.x + radius,
            center_y: rect.y + rect.height - radius,
            radius,
            start_angle: pi / 2.0,
            end_angle: pi,
        },
        ArcGeometry {
            center_x: rect.x + radius,
            center_y: rect.y + radius,
            radius,
            start_angle: pi,
            end_angle: 3.0 * pi / 2.0,
        },
    ])
}
