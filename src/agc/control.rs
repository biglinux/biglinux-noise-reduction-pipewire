//! Pure source-volume adapter. Kept independent of PipeWire/configuration I/O so
//! normalization, readback reconciliation and manual ownership can be unit-tested.

use crate::gain_policy::{Config, Decision, Features, GainPolicy};

use super::features::FeatureExtractor;

pub(super) const MIN_VOL_DB: f64 = -40.0;
/// Exactly 60 * log10(3), not an extra restriction on the weak-microphone range.
pub(super) const MAX_VOL_DB: f64 = 28.627_275_283_179_745;
const PEAK_CEILING_DBFS: f64 = -3.0;
const WRITE_THRESHOLD_DB: f64 = 0.4;

pub(super) fn wpctl_to_db(frac: f32) -> f64 {
    60.0 * f64::from(frac.max(1e-4)).log10()
}

pub(super) fn db_to_wpctl(db: f64) -> f32 {
    (10_f64.powf(db / 60.0) as f32).clamp(0.0, 3.0)
}

pub(super) fn volume_changed(actual: f32, expected: f32) -> bool {
    // get-volume prints two decimals. Compare readbacks in that domain, rather
    // than mistaking the coarse dB steps of a quiet mic for unrelated movement.
    (actual - expected).abs() > 0.005
}

fn valid_volume(volume: f32) -> bool {
    volume.is_finite() && (0.0..=3.0).contains(&volume)
}

fn agc_config(target_dbfs: f64, base_db: f64, noise_ceiling_dbfs: Option<f64>) -> Config {
    Config {
        target_speech_dbfs: target_dbfs,
        noise_ceiling_dbfs,
        // Match the actual actuator range: the integrator cannot wind up to +50
        // while the microphone is already at its maximum. Do not jump a quiet
        // starting volume to MIN_VOL_DB merely because the controller is enabled.
        max_boost_db: (MAX_VOL_DB - base_db).max(0.0),
        min_digital_db: (MIN_VOL_DB - base_db).min(0.0),
        rise_ceil_db_per_s: 8.0,
        fall_ceil_db_per_s: 14.0,
        // The extractor already requires 1.2 seconds of modulated evidence.
        // This short extra guard must not exclude ordinary short syllables.
        speech_hold_s: 0.1,
        ..Config::default()
    }
}

pub(super) struct ControlStep {
    pub(super) features: Features,
    pub(super) decision: Decision,
    pub(super) target_db: f64,
    pub(super) requested_volume: Option<f32>,
}

pub(super) struct SourceController {
    base_db: f64,
    applied_db: f64,
    applied_volume: f32,
    target_dbfs: f64,
    noise_ceiling_dbfs: Option<f64>,
    /// The daemon's licence: no write may leave this window. `None` is a
    /// calibration pass, where the person is speaking on purpose and the whole
    /// 0..300% range is theirs.
    window: Option<(f32, f32)>,
    extractor: FeatureExtractor,
    policy: GainPolicy,
    faulted: bool,
    last_request: Option<f32>,
}

impl SourceController {
    pub(super) fn new(
        volume: f32,
        target_dbfs: f64,
        noise_ceiling_dbfs: Option<f64>,
    ) -> Result<Self, String> {
        if !valid_volume(volume) {
            return Err(
                "source volume unavailable or outside 0..300%; leaving it untouched".into(),
            );
        }
        let base_db = wpctl_to_db(volume);
        Ok(Self {
            base_db,
            applied_db: base_db,
            applied_volume: volume,
            target_dbfs,
            noise_ceiling_dbfs,
            window: None,
            extractor: FeatureExtractor::new(),
            policy: GainPolicy::new(None, agc_config(target_dbfs, base_db, noise_ceiling_dbfs))?,
            faulted: false,
            last_request: None,
        })
    }

    pub(super) fn for_calibration(
        volume: f32,
        target: f64,
        noise: Option<f64>,
    ) -> Result<Self, String> {
        let mut controller = Self::new(volume, target, noise)?;
        controller.extractor = FeatureExtractor::for_calibration();
        Ok(controller)
    }

    pub(super) fn applied_volume(&self) -> f32 {
        self.applied_volume
    }
    pub(super) fn applied_db(&self) -> f64 {
        self.applied_db
    }
    pub(super) fn primary_dbfs(&self) -> Option<f64> {
        self.extractor.primary_dbfs()
    }
    pub(super) fn noise_is_upper_bound(&self) -> bool {
        self.extractor.noise_is_upper_bound()
    }

    /// How much the metered level is moving, in dB. For the daemon's log, while
    /// the threshold that will use it is still being measured.
    pub(super) fn modulation_db(&self) -> f64 {
        self.extractor.modulation_db()
    }

    /// Confine every future write to the window a calibration drew.
    ///
    /// This is the daemon's whole licence. Speaking close and far established how
    /// loud this microphone gets in this room; outside those two volumes there is
    /// no evidence, and a controller acting without evidence is what turned a
    /// neighbour's television into a volume change.
    pub(super) fn set_window(&mut self, low: f32, high: f32) -> bool {
        // wpctl readback/setter use hundredths. Round both bounds inward;
        // no automatic write may turn a positive microphone into zero/mute.
        if !valid_volume(low) || !valid_volume(high) || low > high {
            self.actuator_fault();
            return false;
        }
        let low = ((f64::from(low) * 100.0 - 1e-4).ceil().max(1.0) / 100.0) as f32;
        let high = ((f64::from(high) * 100.0 + 1e-4).floor() / 100.0) as f32;
        if !valid_volume(low) || !valid_volume(high) || low > high || high == 0.0 {
            self.actuator_fault();
            return false;
        }
        let changed = self.window.is_some_and(|old| old != (low, high));
        self.window = Some((low, high));
        if self
            .policy
            .set_external_gain_bounds(
                wpctl_to_db(low) - self.base_db,
                wpctl_to_db(high) - self.base_db,
            )
            .is_err()
        {
            self.actuator_fault();
            return false;
        }
        if changed && !self.faulted {
            // Redrawing the range is explicit consent, unlike a timer/reconnect.
            self.policy.rearm();
            self.last_request = None;
        }
        true
    }

    /// A denoiser downstream means the measured raw floor never reaches a
    /// listener, so it must not veto a boost. Live, because the user can toggle
    /// noise reduction at any time.
    pub(super) fn set_noise_ceiling(&mut self, noise_ceiling_dbfs: Option<f64>) {
        self.noise_ceiling_dbfs = noise_ceiling_dbfs;
        self.policy.set_noise_ceiling_dbfs(noise_ceiling_dbfs);
    }

    pub(super) fn interrupt(&mut self) {
        self.extractor.interrupt();
        self.policy.interrupt_tracking();
        // Cancel a correction accumulated against audio that no longer exists.
        let _ = self
            .policy
            .synchronize_external_gain(self.applied_db - self.base_db);
        self.last_request = None;
    }

    /// Re-baseline AND zero the old correction. A user's slider is not permission
    /// to reapply the previous gain on top of their new value.
    ///
    /// Whether it also takes the wheel away depends on where the new volume
    /// landed. Inside the calibrated window it does not: that window is the
    /// daemon's whole licence, a calibration pass writes a volume into it by
    /// design, and latching there left the controller dead — measured here, one
    /// pass put the level at 19.1 dB and the daemon then sat in `manual_hold`
    /// for every frame that followed, so the gain never came down again until
    /// the unit was restarted. Outside the window there is no evidence at all,
    /// and somebody who went there deliberately keeps the wheel.
    pub(super) fn external_volume_change(&mut self, volume: f32) -> Result<bool, String> {
        if !valid_volume(volume) {
            self.actuator_fault();
            return Err("external source volume outside controllable range".into());
        }
        let new_base = wpctl_to_db(volume);
        self.extractor.rebase(new_base - self.base_db);
        self.base_db = new_base;
        self.applied_db = new_base;
        self.applied_volume = volume;
        self.policy = GainPolicy::new(
            None,
            agc_config(self.target_dbfs, new_base, self.noise_ceiling_dbfs),
        )?;
        if let Some((low, high)) = self.window {
            self.policy.set_external_gain_bounds(
                wpctl_to_db(low) - new_base,
                wpctl_to_db(high) - new_base,
            )?;
        }
        let latched = !self.volume_is_licensed(volume);
        if latched {
            self.policy.user_changed_gain();
        }
        self.last_request = None;
        Ok(latched)
    }

    /// Is this volume one the calibration already vouched for?
    fn volume_is_licensed(&self, volume: f32) -> bool {
        self.window
            .is_some_and(|(low, high)| (low..=high).contains(&volume))
    }

    /// The range this device is licensed to move inside, if it has one.
    pub(super) fn window(&self) -> Option<(f32, f32)> {
        self.window
    }

    /// Is the level currently outside the range it is licensed to be in?
    fn volume_outside_window(&self) -> bool {
        self.window.is_some() && !self.volume_is_licensed(self.applied_volume)
    }

    pub(super) fn actuator_fault(&mut self) {
        self.faulted = true;
        self.policy.user_changed_gain();
        self.extractor.interrupt();
    }

    /// Called only after a successful command AND a real volume/mute readback.
    /// A failed/unknown write is not an ACK. No synchronization on sub-threshold
    /// ticks: otherwise the small rate-limited steps could never accumulate.
    pub(super) fn acknowledge_volume(&mut self, requested: f32, volume: f32) -> Result<(), String> {
        if !valid_volume(requested) || !valid_volume(volume) || volume == 0.0 {
            self.actuator_fault();
            return Err("invalid or zero actuator readback".into());
        }
        if self.window.is_some() && !self.volume_is_licensed(volume) {
            self.actuator_fault();
            return Err("actuator readback outside calibrated window".into());
        }
        let requested_delta = requested - self.applied_volume;
        let actual_delta = volume - self.applied_volume;
        if volume_changed(volume, self.applied_volume) && requested_delta * actual_delta < 0.0 {
            self.actuator_fault();
            return Err("actuator moved opposite to the requested direction".into());
        }
        let db = wpctl_to_db(volume);
        // An unchanged readback can mean a coarse hardware step. Keep accumulating
        // the request until a different knob value crosses that step; do not reset
        // to zero after every ineffectual sub-step. The configured bounds still
        // cap the integrator, and an identical acknowledged request is not retried.
        if volume_changed(volume, self.applied_volume)
            && let Err(error) = self.policy.synchronize_external_gain(db - self.base_db)
        {
            self.actuator_fault();
            return Err(error);
        }
        self.last_request = Some(requested);
        self.applied_db = db;
        self.applied_volume = volume;
        Ok(())
    }

    /// The gain window in decibels: the calibrated range when there is one, and
    /// the daemon's own bounds when there is not.
    fn window_bounds(&self) -> (f64, f64) {
        self.window
            .map_or((MIN_VOL_DB.min(self.base_db), MAX_VOL_DB), |(low, high)| {
                (wpctl_to_db(low), wpctl_to_db(high))
            })
    }

    /// Match the existing setter's decimal quantization BEFORE requesting a
    /// write, then keep the result inside the calibrated window, or nowhere.
    /// The lower bound rounds inward so readback stays inside the policy.
    fn quantize_into_window(&self, target_db: f64, lower: f64) -> f32 {
        let minimum_knob = self
            .window
            .map_or((db_to_wpctl(lower) * 100.0).ceil() / 100.0, |(low, _)| low);
        let quantized = ((db_to_wpctl(target_db) * 100.0).round() / 100.0).clamp(minimum_knob, 3.0);
        self.window
            .map_or(quantized, |(low, high)| quantized.clamp(low, high))
    }

    /// Ask the policy what this frame is worth, after reconciling peak safety
    /// with the volume that is actually applied.
    ///
    /// Peak safety is about clipping, not about the loudness somebody chose.
    /// The ceiling used to be `target + deadband`, but the target is a p80-of-
    /// peaks speech statistic and this is an instantaneous sample peak, so every
    /// ordinary vocal crest sat above it: the guard fired constantly and threw
    /// the accumulated boost back to what was already applied. Measured on a
    /// webcam that needs 300% to be usable — its reference sat 12 dB under the
    /// target and the volume still hovered, because the boost was discarded
    /// faster than the rate law could accumulate it.
    fn decide(
        &mut self,
        features: &Features,
        peak_dbfs: f64,
        applied_gain: f64,
        dt: f64,
    ) -> Result<Decision, String> {
        // Peak safety refers to the actual volume, not an as-yet-unrealized
        // boost waiting for a coarse hardware step. Preserve pending downward
        // steps.
        if !features.muted
            && !features.discontinuity
            && !self.faulted
            && !self.volume_outside_window()
            && peak_dbfs > PEAK_CEILING_DBFS
            && self.policy.current_gain_db() > applied_gain
        {
            self.policy.synchronize_external_gain(applied_gain)?;
        }
        let mut decision = if peak_dbfs.is_finite() {
            self.policy.update_with_peak_guard(
                features,
                dt,
                peak_dbfs - applied_gain,
                PEAK_CEILING_DBFS,
            )?
        } else {
            self.policy.update(features, dt)?
        };
        if self.faulted && !decision.mute {
            decision.state = "actuator_fault";
        }
        Ok(decision)
    }

    pub(super) fn observe(
        &mut self,
        peak_dbfs: f64,
        dt: f64,
        muted: bool,
        admissible: bool,
    ) -> Result<ControlStep, String> {
        let applied_gain = self.applied_db - self.base_db;
        if !admissible {
            self.policy.interrupt_tracking();
        }
        let features = if muted || self.applied_volume == 0.0 {
            self.extractor.source_muted()
        } else {
            self.extractor
                .observe(peak_dbfs, applied_gain, dt, admissible)
        };
        let mut decision = self.decide(&features, peak_dbfs, applied_gain, dt)?;
        let (lower, upper) = self.window_bounds();
        let target_db = (self.base_db + decision.digital_gain_db).clamp(lower, upper);
        let at_limit = (target_db - lower).abs() < 1e-8 || (target_db - upper).abs() < 1e-8;
        let tracking = matches!(decision.state, "tracking_speech" | "peak_safety")
            && ((target_db - self.applied_db).abs() >= WRITE_THRESHOLD_DB || at_limit);
        // A volume outside the window is not a gain decision, it is a licence
        // being broken, and fixing it needs no evidence at all: the person drew
        // a range and the level is not in it. Waiting for trusted speech meant a
        // range set by hand did nothing until somebody talked — which reads, and
        // was reported, as a switch that does not switch anything on.
        let may_write = !self.faulted
            && (tracking
                || (self.volume_outside_window()
                    && !matches!(decision.state, "manual_hold" | "muted")));
        let quantized = self.quantize_into_window(target_db, lower);
        let changed_request = self
            .last_request
            .is_none_or(|last| volume_changed(quantized, last));
        let requested_volume =
            (may_write && volume_changed(quantized, self.applied_volume) && changed_request)
                .then_some(quantized);
        if at_limit
            && !changed_request
            && volume_changed(quantized, self.applied_volume)
            && decision.state == "tracking_speech"
        {
            decision.state = "actuator_limited";
        }
        Ok(ControlStep {
            features,
            decision,
            target_db,
            requested_volume,
        })
    }
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
