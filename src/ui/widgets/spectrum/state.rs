use crate::services::audio_monitor::SpectrumFrame;

use super::constants::{
    BAND_COUNT, METER_HOLD_DECAY, METER_HOLD_TICKS, METER_PEAK_DECAY, PEAK_DECAY, PEAK_HOLD_TICKS,
    SMOOTH_FACTOR,
};
use super::geometry::{db_to_norm, resampled_band_targets};

#[derive(Default)]
pub(super) struct SpectrumState {
    pub(super) bands: [f32; BAND_COUNT],
    pub(super) target_bands: [f32; BAND_COUNT],
    pub(super) peaks: [f32; BAND_COUNT],
    pub(super) band_peak_ticks: [u16; BAND_COUNT],

    /// Overall peak meter, normalised 0..=1.
    pub(super) peak_level: f32,
    pub(super) target_peak: f32,
    pub(super) peak_hold: f32,
    pub(super) meter_hold_ticks: u16,
}

impl SpectrumState {
    pub(super) fn update_targets(&mut self, frame: &SpectrumFrame) {
        if !frame.bands_db.is_empty() {
            self.target_bands = resampled_band_targets(&frame.bands_db);
        }
        self.target_peak = db_to_norm(frame.peak_db);
    }

    pub(super) fn advance_animation(&mut self, is_visible: bool) -> bool {
        if !is_visible {
            return false;
        }

        let mut changed = false;
        changed |= self.advance_peak_meter();
        changed |= self.advance_bands();

        changed || self.peaks.iter().any(|peak| *peak > 0.01)
    }

    pub(super) fn advance_peak_meter(&mut self) -> bool {
        let mut changed = false;
        if self.target_peak > self.peak_level {
            self.peak_level = self.target_peak;
            changed = true;
        } else {
            self.peak_level = (self.peak_level - METER_PEAK_DECAY).max(0.0);
            changed |= self.peak_level > 0.01;
        }

        if self.target_peak > self.peak_hold {
            self.peak_hold = self.target_peak;
            self.meter_hold_ticks = METER_HOLD_TICKS;
            changed = true;
        } else if self.meter_hold_ticks > 0 {
            self.meter_hold_ticks -= 1;
            changed = true;
        } else {
            self.peak_hold = (self.peak_hold - METER_HOLD_DECAY).max(0.0);
            changed |= self.peak_hold > 0.01;
        }
        changed
    }

    pub(super) fn advance_bands(&mut self) -> bool {
        let mut changed = false;
        for band_index in 0..BAND_COUNT {
            let difference = self.target_bands[band_index] - self.bands[band_index];
            if difference.abs() > 0.001 {
                self.bands[band_index] += difference * SMOOTH_FACTOR;
                changed = true;
            }

            if self.bands[band_index] > self.peaks[band_index] {
                self.peaks[band_index] = self.bands[band_index];
                self.band_peak_ticks[band_index] = PEAK_HOLD_TICKS;
                changed = true;
            } else if self.band_peak_ticks[band_index] > 0 {
                self.band_peak_ticks[band_index] -= 1;
                changed = true;
            } else {
                self.peaks[band_index] = (self.peaks[band_index] - PEAK_DECAY).max(0.0);
            }
        }
        changed
    }
}
