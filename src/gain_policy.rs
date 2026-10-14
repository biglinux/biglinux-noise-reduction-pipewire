//! Microphone auto-gain control **policy**, ported from the external review's
//! Python reference (`docs/biglinux-audio-review-20260913/prototypes/`).
//!
//! This is policy only. It decides a digital gain in dB and *proposes* hardware
//! writes from pre-gain dBFS features. It does NOT process PCM and does NOT touch
//! hardware: a separate audio adapter must apply per-sample gain plus a final
//! limiter, and a non-real-time owner must validate and ACK hardware readback.
//! Not wired into the pipeline — off by default. Constants are engineering
//! starting points, not acoustically validated.
//!
//! All dBFS features are measured BEFORE this policy's digital gain.

/// How far below the speech target an amplified noise floor has to stay.
///
/// The ceiling used to be written as an absolute -50 dBFS, which silently meant
/// "27 dB below the default -23 dBFS target". Once the target became a slider the
/// two drifted apart: at a -6 dBFS target that same absolute ceiling demands 44 dB
/// of SNR, which no consumer microphone delivers, so the policy commanded a cut on
/// every microphone in the world. The rule is a margin, so state it as one.
pub const NOISE_MARGIN_DB: f64 = 27.0;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub target_speech_dbfs: f64,
    pub max_boost_db: f64,
    pub min_digital_db: f64,
    /// Cap on the noise level this policy is willing to amplify to, or `None`
    /// when nothing downstream will deliver that noise to a listener. The
    /// measured floor belongs to the raw microphone; whether it survives to the
    /// other end is knowledge only the caller has.
    pub noise_ceiling_dbfs: Option<f64>,
    /// Comfort-zone half-width. While the (integrated) speech level is within this
    /// many dB of target the gain is frozen, so natural vocal dynamics and passing
    /// intonation do not move it — a broadcast leveler's "window".
    pub deadband_db: f64,
    /// Gentle rates at the edge of the comfort zone (dB/s). A level just outside the
    /// window creeps; it does not lurch. `rise` = boost (too quiet), `fall` = cut.
    pub rise_db_per_s: f64,
    pub fall_db_per_s: f64,
    /// Fast rates at the far extreme (dB/s), reached when the level is `rate_span_db`
    /// beyond the comfort zone (a whisper or a shout). Cut is faster than boost to
    /// protect against clipping and avoid pumping noise up in pauses.
    pub rise_ceil_db_per_s: f64,
    pub fall_ceil_db_per_s: f64,
    /// Error beyond the comfort zone at which the rate saturates to the ceiling.
    pub rate_span_db: f64,
    pub speech_hold_s: f64,
    /// How long tracking continues after the last frame of trusted speech.
    ///
    /// Without it the gain moves only on trusted frames, and measured on a real
    /// conversation that is 3% of them: a correction of 8.6 dB that the rate law
    /// says should take ten seconds took minutes, so moving the target slider
    /// looked like it did nothing at all. Nothing here reacts to a pause — the
    /// reference and the target are unchanged, so the gain simply keeps walking
    /// toward the value it was already walking toward. Silence with no reference
    /// ever learned still freezes, as does a pause longer than this.
    pub track_release_s: f64,
    pub hardware_interval_s: f64,
    pub hardware_ack_timeout_s: f64,
    pub manual_hold_requires_rearm: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target_speech_dbfs: -23.0,
            max_boost_db: 12.0,
            min_digital_db: -24.0,
            noise_ceiling_dbfs: Some(-23.0 - NOISE_MARGIN_DB),
            // Half the old 4 dB: the whole target slider spans 12 dB, so a 4 dB
            // comfort zone made most of its positions indistinguishable.
            deadband_db: 2.0,
            rise_db_per_s: 0.4,
            fall_db_per_s: 0.6,
            rise_ceil_db_per_s: 6.0,
            fall_ceil_db_per_s: 10.0,
            rate_span_db: 15.0,
            speech_hold_s: 0.5,
            track_release_s: 1.5,
            hardware_interval_s: 2.0,
            hardware_ack_timeout_s: 1.0,
            manual_hold_requires_rearm: true,
        }
    }
}

/// Hardware capability. Every field must be established by the backend, never
/// inferred from a volume percentage.
#[derive(Clone, Debug)]
pub struct Hardware {
    pub device_epoch: String,
    pub gain_db: f64,
    pub min_db: f64,
    pub max_db: f64,
    pub step_db: f64,
    pub verified_before_adc: bool,
    pub writable_and_readable: bool,
    pub raw_clip_meter_verified: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Features {
    pub speech_dbfs: f64,
    pub noise_dbfs: f64,
    pub raw_clip_fraction: f64,
    pub speech_probability: f64,
    pub snr_db: f64,
    pub muted: bool,
    pub far_end_only: bool,
    pub unreliable_double_talk: bool,
    pub discontinuity: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HardwareRequest {
    pub request_id: u64,
    pub device_epoch: String,
    pub expected_db: f64,
    pub requested_db: f64,
    pub reason: &'static str,
}

#[derive(Clone, Debug)]
pub struct Decision {
    pub digital_gain_db: f64,
    pub mute: bool,
    pub state: &'static str,
    pub hardware_request: Option<HardwareRequest>,
}

pub struct GainPolicy {
    config: Config,
    hardware: Option<Hardware>,
    // Reachability uses physical headroom, not a subsequently narrowed licence.
    evidence_max_boost_db: f64,
    gain_db: f64,
    time: f64,
    speech_time: f64,
    /// Seconds since the last frame of trusted speech; infinite until the first.
    since_trusted: f64,
    clip_time: f64,
    low_time: f64,
    last_hardware_time: f64,
    manual_hold: bool,
    hardware_failed: bool,
    pending: Option<HardwareRequest>,
    pending_time: f64,
    next_id: u64,
    settle_until: f64,
    last_muted: bool,
}

/// How far under the reachable level a voice may still sit before the
/// controller stops treating it as one. See `reachable` in `update`.
///
/// The two journeys this number sits between, both measured: a microphone at
/// 30% volume, whose speech lands 16 dB beyond what its 60 dB of available
/// boost can fix and which this controller must still raise; and a quiet room
/// at a calibrated volume, whose tone lands 31 dB beyond the 17 dB left in the
/// actuator and which must never be raised at all. Anything from 17 to 46
/// separates them; 25 leaves both ends room.
const UNREACHABLE_SLACK_DB: f64 = 25.0;

impl GainPolicy {
    pub fn new(hardware: Option<Hardware>, config: Config) -> Result<Self, String> {
        let numeric = [
            config.target_speech_dbfs,
            config.max_boost_db,
            config.min_digital_db,
            config.deadband_db,
            config.rise_db_per_s,
            config.fall_db_per_s,
            config.rise_ceil_db_per_s,
            config.fall_ceil_db_per_s,
            config.rate_span_db,
            config.speech_hold_s,
            config.track_release_s,
            config.hardware_interval_s,
            config.hardware_ack_timeout_s,
        ];
        if !numeric
            .iter()
            .chain(config.noise_ceiling_dbfs.iter())
            .all(|x| x.is_finite())
        {
            return Err("non-finite configuration".into());
        }
        if !(config.min_digital_db <= 0.0
            && 0.0 <= config.max_boost_db
            && config.deadband_db >= 0.0
            && config.rise_db_per_s > 0.0
            && config.fall_db_per_s > 0.0
            && config.rise_ceil_db_per_s >= config.rise_db_per_s
            && config.fall_ceil_db_per_s >= config.fall_db_per_s
            && config.rate_span_db > 0.0
            && config.speech_hold_s > 0.0
            && config.track_release_s >= 0.0
            && config.hardware_interval_s >= 2.0
            && config.hardware_ack_timeout_s > 0.0)
        {
            return Err("invalid configuration bounds".into());
        }
        // Manual ownership never expires on its own.
        if !config.manual_hold_requires_rearm {
            return Err("manual ownership requires explicit rearm".into());
        }
        if let Some(hw) = &hardware {
            let vals = [hw.gain_db, hw.min_db, hw.max_db, hw.step_db];
            let valid = vals.iter().all(|x| x.is_finite())
                && !hw.device_epoch.is_empty()
                && (hw.min_db..=hw.max_db).contains(&hw.gain_db)
                && hw.step_db > 0.0
                && hw.step_db <= 3.0;
            if !valid {
                return Err("invalid or excessively coarse hardware range".into());
            }
        }
        Ok(Self {
            evidence_max_boost_db: config.max_boost_db,
            config,
            hardware,
            gain_db: 0.0,
            time: 0.0,
            speech_time: 0.0,
            since_trusted: f64::INFINITY,
            clip_time: 0.0,
            low_time: 0.0,
            last_hardware_time: 0.0,
            manual_hold: false,
            hardware_failed: false,
            pending: None,
            pending_time: 0.0,
            next_id: 1,
            settle_until: 0.0,
            last_muted: false,
        })
    }

    /// The native backend must distinguish our ACKs from unrelated/manual changes.
    pub fn user_changed_gain(&mut self) {
        self.manual_hold = true;
        self.pending = None;
        self.speech_time = 0.0;
        self.since_trusted = f64::INFINITY;
        self.clip_time = 0.0;
        self.low_time = 0.0;
    }

    /// Invalidate evidence without granting permission to override a manual
    /// setting. A capture discontinuity or a rejected speaker is not a word gap.
    pub fn interrupt_tracking(&mut self) {
        self.pending = None;
        self.speech_time = 0.0;
        self.since_trusted = f64::INFINITY;
        self.clip_time = 0.0;
        self.low_time = 0.0;
    }

    /// Bind the software integrator to the actual external actuator window.
    /// Clipping only the final wpctl command leaves an integrator wound up
    /// outside that window and delays the next reversal.
    pub fn set_external_gain_bounds(&mut self, min_db: f64, max_db: f64) -> Result<(), String> {
        if self.hardware.is_some() || !min_db.is_finite() || !max_db.is_finite() || min_db > max_db
        {
            return Err("invalid external actuator bounds".into());
        }
        self.config.min_digital_db = min_db;
        self.config.max_boost_db = max_db;
        self.gain_db = self.gain_db.clamp(min_db, max_db);
        Ok(())
    }

    /// Update the target speech level (dBFS) live — e.g. when the user moves the
    /// target slider. Ignored if not finite.
    pub fn set_target_speech_dbfs(&mut self, dbfs: f64) {
        if dbfs.is_finite() {
            self.config.target_speech_dbfs = dbfs;
        }
    }

    /// Update the noise ceiling live — e.g. when the user turns noise reduction
    /// on or off. `None` removes the ceiling entirely.
    pub fn set_noise_ceiling_dbfs(&mut self, dbfs: Option<f64>) {
        if dbfs.is_none_or(f64::is_finite) {
            self.config.noise_ceiling_dbfs = dbfs;
        }
    }

    /// Explicit user action, not silence, a timer, or an application reconnect.
    pub fn rearm(&mut self) {
        self.manual_hold = false;
        self.speech_time = 0.0;
        self.since_trusted = f64::INFINITY;
        self.clip_time = 0.0;
        self.low_time = 0.0;
        self.last_hardware_time = self.time;
    }

    pub fn acknowledge(
        &mut self,
        request_id: u64,
        device_epoch: &str,
        readback_db: f64,
        success: bool,
    ) -> bool {
        let Some(request) = self.pending.clone() else {
            return false;
        };
        if request_id != request.request_id || device_epoch != request.device_epoch {
            return false;
        }
        let hw = self.hardware.clone();
        self.pending = None;
        // Readback must match the request within measurement tolerance, not a whole
        // hardware step: otherwise an external user's simultaneous change could win.
        let bad = match &hw {
            None => true,
            Some(hw) => {
                !(success
                    && readback_db.is_finite()
                    && self.time - self.pending_time <= self.config.hardware_ack_timeout_s
                    && (hw.min_db..=hw.max_db).contains(&readback_db)
                    && (readback_db - request.requested_db).abs() <= 0.1)
            }
        };
        if bad {
            self.hardware_failed = true;
            return false;
        }
        let hw = hw.expect("hw is Some when !bad");
        let delta = readback_db - hw.gain_db;
        self.hardware = Some(Hardware {
            gain_db: readback_db,
            ..hw
        });
        // A clipping-reduction request must NOT immediately boost digital gain to
        // undo the safety action. Normal upstream gain increases are compensated.
        if request.reason != "persistent_raw_clipping" {
            self.gain_db =
                (self.gain_db - delta).clamp(self.config.min_digital_db, self.config.max_boost_db);
        }
        self.settle_until = self.time + 0.5;
        self.speech_time = 0.0;
        self.clip_time = 0.0;
        self.low_time = 0.0;
        true
    }

    fn proposal(&mut self, steps: f64, reason: &'static str) -> Option<HardwareRequest> {
        let hw = self.hardware.clone()?;
        if !hw.verified_before_adc
            || !hw.writable_and_readable
            || self.hardware_failed
            || self.pending.is_some()
            || self.last_muted
            || self.time - self.last_hardware_time < self.config.hardware_interval_s
        {
            return None;
        }
        let target = (hw.gain_db + steps * hw.step_db).clamp(hw.min_db, hw.max_db);
        // Do not write a partial step at a range edge: backend must expose exact grid.
        if (target - hw.gain_db).abs() < hw.step_db - 1e-9 {
            return None;
        }
        let req = HardwareRequest {
            request_id: self.next_id,
            device_epoch: hw.device_epoch.clone(),
            expected_db: hw.gain_db,
            requested_db: target,
            reason,
        };
        self.next_id += 1;
        self.pending = Some(req.clone());
        self.pending_time = self.time;
        self.last_hardware_time = self.time;
        Some(req)
    }

    pub fn current_gain_db(&self) -> f64 {
        self.gain_db
    }

    /// Reconcile this software policy with a successful external actuator readback.
    /// Do not use for hardware proposals: their epoch/request ACK protocol remains
    /// authoritative. This neither rearms manual ownership nor creates speech.
    pub fn synchronize_external_gain(&mut self, applied_db: f64) -> Result<(), String> {
        if self.hardware.is_some() || !applied_db.is_finite() {
            return Err("external synchronization requires a finite software-only gain".into());
        }
        if applied_db < self.config.min_digital_db - 0.01
            || applied_db > self.config.max_boost_db + 0.01
        {
            return Err("external gain is outside the configured actuator range".into());
        }
        self.gain_db = applied_db.clamp(self.config.min_digital_db, self.config.max_boost_db);
        Ok(())
    }

    pub fn update(&mut self, features: &Features, dt: f64) -> Result<Decision, String> {
        self.update_impl(features, dt, None)
    }

    /// Optional, downward-only post-volume sample-peak protection. Neither this
    /// slow controller nor its sample peak replaces the audio chain's limiter.
    /// `peak_before_gain` uses the SAME baseline as Features; no double gain.
    pub fn update_with_peak_guard(
        &mut self,
        features: &Features,
        dt: f64,
        peak_before_gain: f64,
        ceiling_dbfs: f64,
    ) -> Result<Decision, String> {
        if !peak_before_gain.is_finite() || !ceiling_dbfs.is_finite() {
            return Err("non-finite peak guard".into());
        }
        self.update_impl(features, dt, Some((peak_before_gain, ceiling_dbfs)))
    }

    /// A decision that changes nothing, under a name that says why.
    fn steady(&self, state: &'static str) -> Decision {
        Decision {
            digital_gain_db: self.gain_db,
            mute: false,
            state,
            hardware_request: None,
        }
    }

    /// Reject a frame the adapter should never have produced. A NaN fails
    /// `> 0.0` and an infinity fails `<= 0.1`, so `dt` needs no separate
    /// `is_finite` check.
    fn validate(features: &Features, dt: f64) -> Result<(), String> {
        if !(dt > 0.0 && dt <= 0.1) {
            return Err("dt must be a finite frame duration in (0, 0.1] seconds".into());
        }
        let vals = [
            features.speech_dbfs,
            features.noise_dbfs,
            features.raw_clip_fraction,
            features.speech_probability,
            features.snr_db,
        ];
        if !vals.iter().all(|x| x.is_finite())
            || !(0.0..=1.0).contains(&features.raw_clip_fraction)
            || !(0.0..=1.0).contains(&features.speech_probability)
        {
            return Err("invalid feature vector; the audio adapter must fail safely".into());
        }
        Ok(())
    }

    /// Every reason this frame decides nothing: the source is muted or just
    /// restarted, somebody is holding the volume by hand, or a previous request
    /// has not been acknowledged yet.
    fn halted(&mut self, features: &Features) -> Option<Decision> {
        if features.muted || features.discontinuity {
            // Cancels only the local request; the worker must also recheck mute
            // and epoch immediately before every actual write and cancel stale
            // requests.
            self.pending = None;
            self.speech_time = 0.0;
            self.since_trusted = f64::INFINITY;
            self.clip_time = 0.0;
            self.low_time = 0.0;
            self.settle_until = self.time + 0.5;
            return Some(Decision {
                digital_gain_db: self.gain_db,
                mute: features.muted,
                state: if features.muted { "muted" } else { "settling" },
                hardware_request: None,
            });
        }
        if self.manual_hold {
            return Some(self.steady("manual_hold"));
        }
        if self.time < self.settle_until || self.pending.is_some() {
            return Some(self.steady(if self.pending.is_none() {
                "settling"
            } else {
                "awaiting_ack"
            }));
        }
        None
    }

    /// Reduction of VERIFIED pre-ADC clipping does not require a VAD decision.
    fn clipping_safety(&mut self) -> Option<Decision> {
        let verified = self
            .hardware
            .as_ref()
            .is_some_and(|hw| hw.raw_clip_meter_verified);
        if self.clip_time < 0.1 || !verified {
            return None;
        }
        let request = self.proposal(-1.0, "persistent_raw_clipping")?;
        Some(Decision {
            digital_gain_db: self.gain_db,
            mute: false,
            state: "hardware_safety",
            hardware_request: Some(request),
        })
    }

    /// A large peak can require a cut before enough evidence exists to permit a
    /// boost. Mute, discontinuity, manual ownership and hardware ACKs are all
    /// honoured before this runs. This meter never authorizes an unverified
    /// ADC-gain request.
    fn peak_safety(&mut self, peak_guard: Option<(f64, f64)>, dt: f64) -> Option<Decision> {
        let (peak, ceiling) = peak_guard?;
        let desired = (ceiling - peak).clamp(self.config.min_digital_db, self.config.max_boost_db);
        let excess = self.gain_db - desired;
        if excess <= 0.0 {
            return None;
        }
        let c = self.config;
        let u = (excess / c.rate_span_db).clamp(0.0, 1.0);
        let rate = c.fall_db_per_s + (c.fall_ceil_db_per_s - c.fall_db_per_s) * u * u;
        self.gain_db -= (rate * dt).min(excess);
        self.low_time = 0.0;
        Some(self.steady("peak_safety"))
    }

    /// Walk the gain toward the level the target asks for, at a rate that grows
    /// with how far outside the comfort zone the error sits.
    fn track(&mut self, features: &Features, dt: f64) -> Decision {
        let c = self.config;
        // The adapter defines its speech-level statistic and matching target. Do
        // not add another averaging stage here. The source AGC supplies a robust
        // primary peak reference; the optional current-peak guard handles
        // outliers.
        let mut requested = c.target_speech_dbfs - features.speech_dbfs;
        // Only refuse to amplify noise somebody will actually hear. With a
        // denoiser downstream the raw floor is not what leaves the chain, and
        // judging it there blocks every microphone whose own converter hisses.
        if let Some(ceiling) = c.noise_ceiling_dbfs {
            requested = requested.min(ceiling - features.noise_dbfs);
        }
        let desired = requested.clamp(c.min_digital_db, c.max_boost_db);
        let delta = desired - self.gain_db;
        // Non-linear rate law: the further the level sits outside the comfort
        // zone, the faster we move — gentle near the edge, fast at the extremes
        // (whisper / shout) — so corrections are quick when they matter without
        // hunting on normal dynamics. `shape = u²` keeps the response flat near
        // the zone. The comfort zone belongs to the level error, not the
        // physical stop. Otherwise a weak mic needing MORE than the available
        // gain would always stop deadband_db short of its actual 300% ceiling.
        let excess = (requested - self.gain_db).abs() - c.deadband_db;
        if excess > 0.0 && delta != 0.0 {
            let (floor, ceil) = if delta > 0.0 {
                (c.rise_db_per_s, c.rise_ceil_db_per_s)
            } else {
                (c.fall_db_per_s, c.fall_ceil_db_per_s)
            };
            let u = (excess / c.rate_span_db).clamp(0.0, 1.0);
            let rate = floor + (ceil - floor) * u * u;
            let step = (rate * dt).min(delta.abs());
            self.gain_db += step.copysign(delta);
        }
        self.low_time = if features.speech_dbfs < c.target_speech_dbfs - 12.0
            && features.snr_db >= 18.0
            && features.raw_clip_fraction == 0.0
        {
            self.low_time + dt
        } else {
            0.0
        };
        Decision {
            digital_gain_db: self.gain_db,
            mute: false,
            state: "tracking_speech",
            hardware_request: (self.low_time >= 3.0)
                .then(|| self.proposal(1.0, "sustained_quiet_clean_speech"))
                .flatten(),
        }
    }

    fn update_impl(
        &mut self,
        features: &Features,
        dt: f64,
        peak_guard: Option<(f64, f64)>,
    ) -> Result<Decision, String> {
        Self::validate(features, dt)?;
        self.time += dt;
        self.last_muted = features.muted;
        if self.pending.is_some()
            && self.time - self.pending_time > self.config.hardware_ack_timeout_s
        {
            self.pending = None;
            self.hardware_failed = true;
        }
        if let Some(decision) = self.halted(features) {
            return Ok(decision);
        }
        // A level no reachable volume can bring near the target is not a voice
        // this controller should chase. Measured on this workstation: seconds
        // after the daemon rebound, ordinary room tone (a chair, a keyboard)
        // read 60-75 dB under the target, was authorized as speech, and asked
        // for a boost far past the actuator's ceiling — so the capture pinned at
        // the top of the calibrated window and stayed there until somebody
        // spoke. `max_boost_db` is what the actuator can still give from this
        // baseline, and the slack keeps the supported journey of a
        // weak capsule that needs nearly all of it.
        let reachable = features.speech_dbfs
            >= self.config.target_speech_dbfs - (self.evidence_max_boost_db + UNREACHABLE_SLACK_DB);
        if features.far_end_only || features.unreliable_double_talk {
            self.interrupt_tracking();
        }
        let trusted_speech = reachable
            && features.speech_probability >= 0.8
            && features.snr_db >= 10.0
            && !features.far_end_only
            && !features.unreliable_double_talk;
        self.speech_time = if trusted_speech {
            self.speech_time + dt
        } else {
            0.0
        };
        self.clip_time = if features.raw_clip_fraction >= 0.002 {
            self.clip_time + dt
        } else {
            0.0
        };
        if let Some(decision) = self.clipping_safety() {
            return Ok(decision);
        }
        if let Some(decision) = self.peak_safety(peak_guard, dt) {
            return Ok(decision);
        }
        if trusted_speech && self.speech_time >= self.config.speech_hold_s {
            self.since_trusted = 0.0;
        } else {
            self.since_trusted += dt;
        }
        // Keep walking toward the same target through the gaps between
        // syllables: a conversation supplies trusted frames in bursts, and
        // stopping dead between them is what made the whole loop feel inert.
        if self.since_trusted > self.config.track_release_s {
            self.low_time = 0.0;
            return Ok(self.steady("frozen_no_trusted_speech"));
        }
        Ok(self.track(features, dt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reported failure: a few seconds without talking and the capture was
    /// back at maximum. Room tone at a calibrated volume is 60-75 dB under the
    /// target, which no remaining boost can reach — asking for it pins the
    /// actuator at its ceiling until somebody speaks again.
    #[test]
    fn room_tone_no_boost_could_ever_reach_is_not_speech() {
        let config = Config {
            target_speech_dbfs: -12.0,
            // What is left in the actuator once a calibration has already put
            // the volume where this person's voice lands.
            max_boost_db: 16.8,
            ..Config::default()
        };
        let mut policy = GainPolicy::new(None, config).unwrap();
        let room = Features {
            speech_dbfs: -75.0,
            noise_dbfs: -104.0,
            snr_db: 29.0,
            speech_probability: 1.0,
            ..voice()
        };
        let mut state = "";
        for _ in 0..200 {
            state = policy.update(&room, 0.05).unwrap().state;
        }
        assert_eq!(state, "frozen_no_trusted_speech");
        assert!(policy.current_gain_db().abs() < 0.01, "it boosted the room");
    }

    fn hw() -> Hardware {
        Hardware {
            device_epoch: "usb-test:epoch-1".into(),
            gain_db: 12.0,
            min_db: 0.0,
            max_db: 30.0,
            step_db: 1.5,
            verified_before_adc: true,
            writable_and_readable: true,
            raw_clip_meter_verified: true,
        }
    }
    fn voice() -> Features {
        Features {
            speech_dbfs: -35.0,
            noise_dbfs: -70.0,
            raw_clip_fraction: 0.0,
            speech_probability: 0.95,
            snr_db: 25.0,
            muted: false,
            far_end_only: false,
            unreliable_double_talk: false,
            discontinuity: false,
        }
    }
    fn policy(with_hw: bool) -> GainPolicy {
        GainPolicy::new(with_hw.then(hw), Config::default()).unwrap()
    }
    fn run(p: &mut GainPolicy, f: &Features, n: usize) -> Vec<Decision> {
        (0..n).map(|_| p.update(f, 0.01).unwrap()).collect()
    }
    fn quiet_request() -> (GainPolicy, HardwareRequest) {
        let mut p = policy(true);
        let f = Features {
            speech_dbfs: -42.0,
            ..voice()
        };
        for _ in 0..500 {
            if let Some(req) = p.update(&f, 0.01).unwrap().hardware_request {
                return (p, req);
            }
        }
        panic!("no quiet-speech request");
    }

    #[test]
    fn a_conversation_keeps_tracking_through_the_gaps_between_syllables() {
        // Measured on this project's own hardware, someone speaking normally into
        // the microphone: 6 of 195 frames cleared the trusted-speech bar. A gain
        // that may only move on those frames crawls — 8.6 dB of correction took
        // minutes, so moving the target slider looked like it did nothing at all.
        let cfg = Config {
            speech_hold_s: 0.1,
            max_boost_db: 28.0,
            ..Config::default()
        };
        let clean = Features {
            noise_dbfs: -90.0,
            ..voice()
        }; // no noise-ceiling clamp
        let speech = Features {
            speech_dbfs: -35.0,
            ..clean
        };
        let gap = Features {
            speech_probability: 0.0,
            snr_db: 3.0,
            ..speech
        };
        let mut p = GainPolicy::new(None, cfg).unwrap();
        // `run` steps 10 ms per frame: 0.12 s of trusted speech — barely past
        // speech_hold_s — then 0.88 s that is not, 36 times.
        for _ in 0..36 {
            run(&mut p, &speech, 12);
            run(&mut p, &gap, 88);
        }
        assert!(
            p.gain_db > 9.0,
            "36 s of conversation only reached {}",
            p.gain_db
        );
        // A real pause still stops it — the release window is not a licence to
        // keep moving through silence.
        let held = p.gain_db;
        run(&mut p, &gap, 600);
        assert!(
            (p.gain_db - held).abs() < 1.5,
            "kept moving in silence: {held} → {}",
            p.gain_db
        );
    }

    #[test]
    fn silence_never_pumps() {
        let mut p = policy(true);
        run(
            &mut p,
            &Features {
                speech_probability: 0.0,
                ..voice()
            },
            3000,
        );
        assert_eq!(p.gain_db, 0.0);
        assert!(p.pending.is_none());
    }

    #[test]
    fn far_end_and_uncertain_double_talk_freeze() {
        for f in [
            Features {
                far_end_only: true,
                ..voice()
            },
            Features {
                unreliable_double_talk: true,
                ..voice()
            },
        ] {
            let mut p = policy(true);
            run(&mut p, &f, 1000);
            assert_eq!(p.gain_db, 0.0);
            assert!(p.pending.is_none());
        }
    }

    #[test]
    fn manual_lock_requires_rearm() {
        let mut p = policy(true);
        p.user_changed_gain();
        run(&mut p, &voice(), 10000);
        assert_eq!(p.gain_db, 0.0);
        assert!(p.manual_hold);
        p.rearm();
        run(&mut p, &voice(), 100);
        assert!(p.gain_db > 0.0);
    }

    #[test]
    fn mute_never_changes_hardware_or_unmutes() {
        let mut p = policy(true);
        let ds = run(
            &mut p,
            &Features {
                muted: true,
                raw_clip_fraction: 0.1,
                ..voice()
            },
            3000,
        );
        assert!(ds.iter().all(|d| d.mute && d.hardware_request.is_none()));
        assert_eq!(p.gain_db, 0.0);
        assert_eq!(p.hardware.as_ref().unwrap().gain_db, 12.0);
    }

    #[test]
    fn digital_gain_slew_and_cap() {
        // voice() is -35 dBFS (12 dB below the -23 target): the level sits 10 dB
        // outside the 2 dB comfort zone, so the rate stays well under the ceiling
        // and never exceeds `rise_ceil_db_per_s * dt`. Gain is capped at max_boost.
        let c = Config::default();
        let per_frame_max = c.rise_ceil_db_per_s * 0.01 + 1e-6;
        let mut p = policy(false);
        let ds = run(&mut p, &voice(), 10000);
        let mut last = 0.0;
        for d in ds {
            assert!(d.digital_gain_db - last <= per_frame_max);
            assert!(d.digital_gain_db <= 12.0);
            last = d.digital_gain_db;
        }
    }

    #[test]
    fn level_inside_comfort_zone_is_left_alone() {
        // Speech 1 dB below target sits inside the 2 dB comfort zone: the gain must
        // not chase it, so natural vocal dynamics and intonation pass untouched.
        let mut p = policy(false);
        run(
            &mut p,
            &Features {
                speech_dbfs: -24.0,
                ..voice()
            },
            3000,
        );
        assert!(
            p.gain_db.abs() < 0.5,
            "in-zone level moved the gain: {}",
            p.gain_db
        );
    }

    #[test]
    fn rate_grows_with_distance_from_target() {
        // A whisper (far outside the zone) must be corrected much faster than a level
        // only just outside it — the non-linear rate law, not a fixed slew.
        let frames = 120; // 1.2 s: past speech_hold, still climbing
        let cfg = Config {
            max_boost_db: 28.0,
            ..Config::default()
        };
        let clean = Features {
            noise_dbfs: -90.0,
            ..voice()
        }; // no noise-ceiling clamp
        let mut whisper = GainPolicy::new(None, cfg).unwrap();
        run(
            &mut whisper,
            &Features {
                speech_dbfs: -60.0,
                ..clean
            },
            frames,
        );
        let mut mild = GainPolicy::new(None, cfg).unwrap();
        run(
            &mut mild,
            &Features {
                speech_dbfs: -30.0,
                ..clean
            },
            frames,
        );
        assert!(
            whisper.gain_db > mild.gain_db + 2.0,
            "whisper {} should climb faster than mild {}",
            whisper.gain_db,
            mild.gain_db
        );
    }

    #[test]
    fn noise_ceiling() {
        let mut p = policy(false);
        run(
            &mut p,
            &Features {
                noise_dbfs: -40.0,
                ..voice()
            },
            1000,
        );
        // Noise at -40 caps desired at noise_ceiling - noise = -10 dB; the gain cuts
        // toward it and settles at the comfort-zone edge (~ -10 + deadband = -6),
        // i.e. it reduces gain rather than boosting the noise floor.
        assert!(p.gain_db <= -5.0);
    }

    #[test]
    fn single_cough_does_not_touch_hardware() {
        let mut p = policy(true);
        run(
            &mut p,
            &Features {
                speech_probability: 0.0,
                ..voice()
            },
            300,
        );
        p.update(
            &Features {
                raw_clip_fraction: 0.1,
                ..voice()
            },
            0.01,
        )
        .unwrap();
        run(
            &mut p,
            &Features {
                speech_probability: 0.0,
                ..voice()
            },
            300,
        );
        assert!(p.pending.is_none());
        assert_eq!(p.hardware.as_ref().unwrap().gain_db, 12.0);
    }

    #[test]
    fn persistent_clip_reduces_hardware() {
        let mut p = policy(true);
        let ds = run(
            &mut p,
            &Features {
                raw_clip_fraction: 0.1,
                speech_probability: 0.0,
                ..voice()
            },
            220,
        );
        let req = ds.into_iter().find_map(|d| d.hardware_request).unwrap();
        assert_eq!(req.requested_db, 10.5);
        let old = p.gain_db;
        assert!(p.acknowledge(req.request_id, &req.device_epoch, 10.5, true));
        assert_eq!(p.gain_db, old); // do not undo a protective reduction
    }

    #[test]
    fn software_volume_is_never_treated_as_analog() {
        let mut p = GainPolicy::new(
            Some(Hardware {
                verified_before_adc: false,
                ..hw()
            }),
            Config::default(),
        )
        .unwrap();
        run(
            &mut p,
            &Features {
                raw_clip_fraction: 0.1,
                ..voice()
            },
            1000,
        );
        assert!(p.pending.is_none());
    }

    #[test]
    fn gain_up_ack_compensates_actual_delta() {
        let (mut p, req) = quiet_request();
        let previous = p.gain_db;
        assert!(p.acknowledge(req.request_id, &req.device_epoch, req.requested_db, true));
        assert!((p.gain_db - (previous - 1.5)).abs() < 1e-6);
    }

    #[test]
    fn stale_epoch_or_nonce_rejected() {
        let (mut p, req) = quiet_request();
        assert!(!p.acknowledge(req.request_id, "new-device", req.requested_db, true));
        assert!(!p.acknowledge(
            req.request_id + 1,
            &req.device_epoch,
            req.requested_db,
            true
        ));
        assert!(p.pending.is_some());
    }

    #[test]
    fn wrong_readback_disables_hardware() {
        let (mut p, req) = quiet_request();
        assert!(!p.acknowledge(req.request_id, &req.device_epoch, 15.0, true));
        assert!(p.hardware_failed);
    }

    #[test]
    fn timeout_disables_hardware() {
        let (mut p, req) = quiet_request();
        run(&mut p, &voice(), 200);
        assert!(p.hardware_failed);
        assert!(!p.acknowledge(req.request_id, &req.device_epoch, req.requested_db, true));
    }

    #[test]
    fn discontinuity_cancels_pending() {
        let (mut p, req) = quiet_request();
        p.update(
            &Features {
                discontinuity: true,
                ..voice()
            },
            0.01,
        )
        .unwrap();
        assert!(p.pending.is_none());
        assert!(!p.acknowledge(req.request_id, &req.device_epoch, req.requested_db, true));
    }

    #[test]
    fn no_hardware_request_more_often_than_two_seconds() {
        let mut p = policy(true);
        let f = Features {
            speech_dbfs: -42.0,
            ..voice()
        };
        let mut times = Vec::new();
        for _ in 0..2000 {
            if let Some(r) = p.update(&f, 0.01).unwrap().hardware_request {
                times.push(p.time);
                p.acknowledge(r.request_id, &r.device_epoch, r.requested_db, true);
            }
        }
        assert!(times.len() >= 2);
        assert!(times.windows(2).all(|w| w[1] - w[0] >= 2.0));
    }

    #[test]
    fn invalid_features_and_dt() {
        for val in [f64::NAN, f64::INFINITY, -1.0, 0.0, 0.2] {
            assert!(policy(false).update(&voice(), val).is_err());
        }
        for f in [
            Features {
                speech_dbfs: f64::NAN,
                ..voice()
            },
            Features {
                raw_clip_fraction: 1.1,
                ..voice()
            },
            Features {
                speech_probability: -0.1,
                ..voice()
            },
        ] {
            assert!(policy(false).update(&f, 0.01).is_err());
        }
    }

    #[test]
    fn invalid_hardware_and_configuration() {
        for h in [
            Hardware {
                step_db: 10.0,
                ..hw()
            },
            Hardware {
                gain_db: 100.0,
                ..hw()
            },
            Hardware {
                gain_db: f64::NAN,
                ..hw()
            },
        ] {
            assert!(GainPolicy::new(Some(h), Config::default()).is_err());
        }
        assert!(
            GainPolicy::new(
                None,
                Config {
                    rise_db_per_s: -1.0,
                    ..Config::default()
                }
            )
            .is_err()
        );
        assert!(
            GainPolicy::new(
                None,
                Config {
                    manual_hold_requires_rearm: false,
                    ..Config::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn reset_device_requires_new_instance() {
        let (_old, req) = quiet_request();
        let mut new = GainPolicy::new(
            Some(Hardware {
                device_epoch: "epoch-2".into(),
                ..hw()
            }),
            Config::default(),
        )
        .unwrap();
        assert!(!new.acknowledge(req.request_id, &req.device_epoch, req.requested_db, true));
        assert_eq!(new.gain_db, 0.0);
    }
    #[test]
    fn negative_tracking_release_is_rejected() {
        assert!(
            GainPolicy::new(
                None,
                Config {
                    track_release_s: -1.0,
                    ..Config::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn discontinuity_drops_release_without_rearming_manual_ownership() {
        let mut p = policy(false);
        run(&mut p, &voice(), 200);
        p.interrupt_tracking();
        let silent = Features {
            speech_probability: 0.0,
            ..voice()
        };
        assert_eq!(
            p.update(&silent, 0.05).unwrap().state,
            "frozen_no_trusted_speech"
        );
        p.user_changed_gain();
        p.interrupt_tracking();
        assert_eq!(p.update(&voice(), 0.05).unwrap().state, "manual_hold");
    }

    #[test]
    fn external_window_bounds_the_integrator_itself() {
        let mut p = policy(false);
        p.set_external_gain_bounds(-3.0, 2.0).unwrap();
        run(&mut p, &voice(), 2000);
        assert!(p.current_gain_db() <= 2.0);
        p.set_external_gain_bounds(-2.0, 1.0).unwrap();
        assert!(p.current_gain_db() <= 1.0);
    }
}

// The legacy safety/ACK tests above remain unchanged.
#[cfg(test)]
#[path = "agc/policy_tests.rs"]
mod source_agc_tests;
