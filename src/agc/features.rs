//! Level-only speech evidence for a short, deliberate calibration pass.
//!
//! This is a peak follower and an energy gate, nothing more. It cannot tell who
//! is speaking, and it no longer tries: the controller that used it ran all day
//! and therefore had to defend itself against a television in the next room,
//! which it did by demanding sustained modulated evidence far above the noise —
//! evidence an ordinary webcam in an ordinary room never produces, so it learned
//! nothing and the level never moved.
//!
//! What replaced it asks for a few seconds in which the person is deliberately
//! talking into their own microphone. Inside that window "somebody is speaking
//! now" is the only question worth asking, and a peak standing clear of the floor
//! answers it. Restored from the version that measurably converged in seconds.

use std::collections::VecDeque;

use crate::gain_policy::Features;

pub(super) struct FeatureExtractor {
    noise_floor: Option<f64>,
    // Deliberate calibration may inspect any non-censored signal. The daemon
    // retains its conservative existing audibility floor until hardware VAD
    // validation establishes a safe replacement.
    audibility_floor: f64,
    noise_is_bound: bool,
    /// Peak-follower envelope of the speaking level, in the baseline's dB frame.
    speech_env: Option<f64>,
    /// Seconds of "still speaking" left, bridging the gaps between words.
    hold_s: f64,
    /// A level that has not changed, and for how long. A voice never holds one
    /// level for seconds; a fan, a fridge or a tone does nothing else.
    flat_level: Option<f64>,
    flat_s: f64,
    /// The two low-pass envelopes whose difference is the syllabic band, and the
    /// running average of that band's magnitude, in dB.
    band_fast: Option<f64>,
    band_slow: Option<f64>,
    modulation_db: f64,
    /// Which side of its centre the band-passed envelope was on, and how often
    /// it has been changing sides, in Hz.
    band_sign: Option<bool>,
    /// Observed audio time, and when the recent crossings happened on it.
    clock_s: f64,
    crossings: VecDeque<f64>,
}

impl FeatureExtractor {
    /// A peak this far over the floor is somebody talking.
    ///
    /// Measured on the webcam this was reported on, with its owner speaking at
    /// their normal distance: peaks at -33 dBFS over a -40 dBFS converter floor.
    /// Seven decibels. A 12 dB margin does not merely miss that voice — it
    /// destroys the evidence, because a peak that is not speech lets the floor
    /// leak upward, and the floor then climbs into the voice until nothing can
    /// ever clear the margin again. What separates a voice from a fan at 6 dB is
    /// not loudness anyway: it is that the fan holds one level and FLAT_S below
    /// hands it to the floor.
    const SPEECH_MARGIN_DB: f64 = 6.0;
    /// The floor drops toward a new quietest level quickly and leaks upward only
    /// in silence, so speech never drags the floor up underneath itself.
    const FLOOR_FALL: f64 = 0.10;
    const FLOOR_RISE: f64 = 0.01;
    /// Fast attack, slow release: the envelope tracks the sustained speaking peak
    /// rather than each syllable.
    ///
    /// The attack is fast but NOT instant, and that distinction is the whole
    /// difference between a pass that works and one that makes things worse. A
    /// door slamming, a cough or a knock on the desk is one frame 20 dB over the
    /// voice; with an instant attack it becomes "the speaking level", and since
    /// the envelope is frozen in silence it stays there for the rest of the pass
    /// — so the microphone gets turned DOWN to put a door slam at the target.
    /// At 0.3 s a single 50 ms transient moves the envelope about 3 dB and decays
    /// away, while an actual voice reaches its level inside a second.
    const ATTACK_TAU_S: f64 = 0.3;
    const RELEASE_TAU_S: f64 = 5.0;
    const HOLD_S: f64 = 0.5;
    /// What the converter has to actually deliver before anything is a voice.
    ///
    /// This is tested on the measured peak, at whatever volume is set right now,
    /// and not in the baseline frame — a microphone at 30% volume presents a
    /// real voice around -76 dBFS, so a threshold in baseline coordinates either
    /// rejects that voice or lets silence through, depending on where the volume
    /// happened to start.
    ///
    /// Measured on this workstation. An empty room at 157% capture reads -98 to
    /// -112 dBFS; the same room with somebody talking reads -5 to -24. The
    /// analyzer censors at -120, so the floor it records in a quiet room is a
    /// bound rather than a measurement, and room tone stood 10-22 dB over that
    /// bound — enough to clear `SPEECH_MARGIN_DB` and be authorized as speech.
    /// The journal shows what followed: `p=1.00`, an envelope decaying toward
    /// silence, and the controller chasing it to the ceiling, so a few seconds
    /// of not talking turned the capture up to maximum. Thirty decibels over the
    /// censor, fourteen under the weakest real voice this daemon has measured.
    const SILENCE_DBFS: f64 = -90.0;
    /// The syllabic band, in hertz, and the decay of the running average that
    /// measures how strongly the level moves inside it.
    ///
    /// This is a heuristic for speech-like modulation, not speaker identity.
    /// Television, music, and some mechanical sounds may also satisfy it. Speech is amplitude-modulated by the
    /// syllable rate, which sits near 4 Hz in every language that has been
    /// measured and which every intelligibility standard built on the modulation
    /// transfer function (STI, and the 0.63-12.5 Hz octave bands it integrates)
    /// takes as given. Machinery does not do this: a fan, a fridge, a
    /// transformer hum or a ventilation duct is stationary, and its level moves
    /// by fractions of a decibel from one tenth of a second to the next.
    ///
    /// The statistic is in decibels, which are a ratio, so it says the same
    /// thing about a whisper and a shout, about a laptop capsule and a studio
    /// condenser, and in a bedroom or a call centre. Nothing here is tuned to a
    /// room, and nothing here may be: this runs on machines nobody has heard.
    /// The band is wider than the 3-5 Hz where the syllable rate sits, because
    /// careful speech, a held word and a language with long vowels all run
    /// slower, and an excited talker runs faster. Nothing outside 1.5-10 Hz is
    /// speech rhythm, and nothing stationary is inside it.
    const SYLLABIC_LOW_HZ: f64 = 1.5;
    const SYLLABIC_HIGH_HZ: f64 = 10.0;
    const MODULATION_AVERAGE_S: f64 = 0.15;
    /// Depth is not enough on its own: switching a machine on is a step, a step
    /// is broadband, and it lands in this band once — measured here, the onset
    /// of a steady fan read as speech for two and a half seconds, long enough
    /// for a controller to move. What a step does not have is rhythm. Counting
    /// how often the band-passed envelope crosses its own centre measures that
    /// directly: a step crosses once, and syllables cross twice per syllable.
    /// The bar is under the slowest speech rhythm and far over a single event.
    const MODULATION_RATE_HZ: f64 = 1.5;
    /// Crossings are counted over this window rather than smoothed into a rate:
    /// an exponential average cannot tell one crossing from a slow rhythm
    /// without either letting a single step through or taking a second to
    /// believe a voice — and a voice has to be heard inside a second, because
    /// the pass that measures it only lasts five.
    const RATE_WINDOW_S: f64 = 1.0;
    /// Modulation this deep is evidence compatible with speech, not proof.
    ///
    /// The bar sits between two families of signal, not between two rooms.
    /// Measured on envelopes defined by their shape alone, through this same
    /// filter pair: a stationary machine wandering 1 dB at 0.2 Hz reads 0.02 dB;
    /// speech-shaped modulation reads 1.5 dB at 6 Hz / 8 dB deep, 2.2 dB at
    /// 4 Hz / 12 dB, and 4.2 dB at 2 Hz / 30 dB. The gentlest speech-like
    /// envelope this project's own fixtures use — 6 dB peak to peak at 1.7 Hz,
    /// which is far flatter than connected speech — reads 0.87 dB.
    ///
    /// At 0.35 dB the bar is fifteen times the machine and less than half the
    /// flattest thing anybody has called speech here. It is a ratio, so it says
    /// the same about a whisper and a shout, a laptop capsule and a condenser.
    const MODULATION_MIN_DB: f64 = 0.35;
    /// Once talking, the bar drops: a sustained vowel or a held note modulates
    /// less than the sentence around it, and dropping the speaker mid-word is
    /// how a controller starts hunting.
    const MODULATION_KEEP_DB: f64 = 0.2;
    /// After this long at one level, whatever it is becomes part of the room.
    /// Without it a fan that starts loud enough is "speech" for as long as it
    /// runs, because the floor only ever leaks upward while nothing is speaking.
    const FLAT_S: f64 = 2.0;
    const FLAT_TOLERANCE_DB: f64 = 1.0;

    pub(super) fn new() -> Self {
        Self {
            noise_floor: None,
            audibility_floor: Self::SILENCE_DBFS,
            speech_env: None,
            hold_s: 0.0,
            noise_is_bound: false,
            flat_level: None,
            flat_s: 0.0,
            band_fast: None,
            band_slow: None,
            modulation_db: 0.0,
            band_sign: None,
            clock_s: 0.0,
            crossings: VecDeque::new(),
        }
    }

    pub(super) fn for_calibration() -> Self {
        Self {
            audibility_floor: -120.0,
            ..Self::new()
        }
    }

    /// How strongly the level is being modulated in the syllabic band, in dB.
    pub(super) fn modulation_db(&self) -> f64 {
        self.modulation_db
    }

    /// How often that modulation changes direction, in Hz — the rhythm.
    pub(super) fn cross_hz(&self) -> f64 {
        self.crossings.len() as f64 / Self::RATE_WINDOW_S
    }

    /// Band-pass the level envelope over the syllabic band: a low-pass at the
    /// top of the band minus a low-pass at the bottom of it. Two exponential
    /// averages, because a filter bank here would buy resolution nothing
    /// downstream can use. Also counts how often the result crosses its own
    /// centre, which is what separates a rhythm from a single step.
    fn track_syllabic_band(&mut self, peak: f64, dt: f64) {
        let alpha = |hz: f64| 1.0 - (-dt * std::f64::consts::TAU * hz).exp();
        let fast = self.band_fast.map_or(peak, |prev| {
            prev + (peak - prev) * alpha(Self::SYLLABIC_HIGH_HZ)
        });
        let slow = self.band_slow.map_or(peak, |prev| {
            prev + (peak - prev) * alpha(Self::SYLLABIC_LOW_HZ)
        });
        self.band_fast = Some(fast);
        self.band_slow = Some(slow);
        self.modulation_db += ((fast - slow).abs() - self.modulation_db)
            * (1.0 - (-dt / Self::MODULATION_AVERAGE_S).exp());
        self.clock_s += dt;
        let above = fast >= slow;
        if self.band_sign.is_some_and(|was| was != above) {
            self.crossings.push_back(self.clock_s);
        }
        self.band_sign = Some(above);
        while self
            .crossings
            .front()
            .is_some_and(|at| self.clock_s - at > Self::RATE_WINDOW_S)
        {
            self.crossings.pop_front();
        }
    }

    /// How long the level has sat still, within a tolerance. A voice never
    /// holds one level; a fan, a fridge or a tone does nothing else.
    fn track_flatness(&mut self, peak: f64, dt: f64) {
        if self
            .flat_level
            .is_some_and(|v| (peak - v).abs() <= Self::FLAT_TOLERANCE_DB)
        {
            self.flat_s += dt;
        } else {
            self.flat_level = Some(peak);
            self.flat_s = dt;
        }
    }

    /// Somebody talking modulates their own level; a machine does not. The bar
    /// is lower once talking has started, so a sustained vowel or a held note
    /// does not drop the speaker mid-word. Must be asked before the hold is
    /// advanced for this frame: "already talking" means the previous frame's
    /// hold, not this one's.
    fn sounds_modulated(&self) -> bool {
        let talking = self.hold_s > 0.0;
        let bar = if talking {
            Self::MODULATION_KEEP_DB
        } else {
            Self::MODULATION_MIN_DB
        };
        self.modulation_db >= bar && (talking || self.cross_hz() >= Self::MODULATION_RATE_HZ)
    }

    /// Instant attack to each loud frame, slow release across a bridged gap,
    /// and frozen in real silence — a pause must never decay the measured level
    /// into the noise and invite a boost of nothing.
    fn advance_envelope(&mut self, env: f64, peak: f64, dt: f64, active: bool) -> f64 {
        let tau = if peak > env {
            Self::ATTACK_TAU_S
        } else {
            Self::RELEASE_TAU_S
        };
        let env = if active {
            env + (peak - env) * (1.0 - (-dt / tau).exp())
        } else {
            env
        };
        self.speech_env = Some(env);
        env
    }

    /// The floor follows the quietest level down quickly, and climbs only
    /// toward something that has held still. "Not speech" is not permission to
    /// climb: a voice the margin happened to miss would be absorbed, and then
    /// it could never clear the margin again.
    fn advance_floor(&mut self, floor: f64, peak: f64, dt: f64) -> f64 {
        let floor = if peak < floor {
            self.noise_is_bound = false;
            floor + (peak - floor) * (1.0 - (1.0 - Self::FLOOR_FALL).powf(dt / 0.05))
        } else if self.flat_s >= Self::FLAT_S {
            floor + (peak - floor) * (1.0 - (1.0 - Self::FLOOR_RISE).powf(dt / 0.05))
        } else {
            floor
        };
        self.noise_floor = Some(floor);
        floor
    }

    pub(super) fn primary_dbfs(&self) -> Option<f64> {
        self.speech_env
    }

    pub(super) fn noise_is_upper_bound(&self) -> bool {
        self.noise_is_bound
    }

    pub(super) fn source_muted(&mut self) -> Features {
        self.interrupt();
        self.untrusted(true, false)
    }

    /// Missing audio is not silence. Drop what a stopped capture cannot vouch
    /// for, and keep the level this microphone was already at.
    pub(super) fn interrupt(&mut self) {
        self.noise_floor = None;
        self.noise_is_bound = false;
        self.hold_s = 0.0;
        self.flat_level = None;
        self.flat_s = 0.0;
        self.band_fast = None;
        self.band_slow = None;
        self.modulation_db = 0.0;
        self.band_sign = None;
        self.crossings.clear();
    }

    /// Changing the normalization baseline changes coordinates, not the voice.
    pub(super) fn rebase(&mut self, delta_db: f64) {
        if !delta_db.is_finite() {
            self.interrupt();
            self.speech_env = None;
            return;
        }
        self.noise_floor = self.noise_floor.map(|v| v + delta_db);
        self.speech_env = self.speech_env.map(|v| v + delta_db);
        self.flat_level = self.flat_level.map(|v| v + delta_db);
        self.band_fast = self.band_fast.map(|v| v + delta_db);
        self.band_slow = self.band_slow.map(|v| v + delta_db);
    }

    fn untrusted(&self, muted: bool, discontinuity: bool) -> Features {
        // Finite placeholders that carry no evidence and cannot authorize a boost.
        let floor = self.noise_floor.unwrap_or(-120.0);
        Features {
            speech_dbfs: self.speech_env.unwrap_or(floor),
            noise_dbfs: floor,
            raw_clip_fraction: 0.0,
            speech_probability: 0.0,
            snr_db: 0.0,
            muted,
            far_end_only: false,
            unreliable_double_talk: false,
            discontinuity,
        }
    }

    /// Input is the selected meter's post-volume sample peak. A denoiser is
    /// nonlinear: normalization is an approximation, not a raw-ADC measurement.
    /// Normalize BEFORE updating any history. `dt` is observed audio time.
    ///
    /// `admissible` is the caller's judgement that this frame could be the voice
    /// the calibration measured — level and spectral tilt inside what was
    /// observed of it. An inadmissible frame still teaches the floor, because it
    /// is genuinely part of the room; it simply never counts as somebody
    /// speaking, so it cannot move the gain.
    pub(super) fn observe(
        &mut self,
        measured_peak: f64,
        applied_gain: f64,
        dt: f64,
        admissible: bool,
    ) -> Features {
        // NaN must be rejected, so `dt` is range-checked only after `is_finite`.
        if !measured_peak.is_finite()
            || !applied_gain.is_finite()
            || !dt.is_finite()
            || dt <= 0.0
            || dt > 0.1
        {
            self.interrupt();
            return self.untrusted(false, true);
        }
        // Test the analyzer floor BEFORE subtracting gain: -110 measured with +28
        // of gain is a valid -138 at baseline. On a raw source, <= -120 means
        // censored data — an UPPER BOUND on the noise, not silence and not a mute.
        if measured_peak <= -120.0 {
            let bound = -120.0 - applied_gain;
            self.noise_floor = Some(self.noise_floor.map_or(bound, |old| old.max(bound)));
            self.noise_is_bound = true;
            self.hold_s = 0.0;
            self.flat_level = None;
            self.flat_s = 0.0;
            return self.untrusted(false, false);
        }
        let peak = measured_peak - applied_gain;
        if !peak.is_finite() {
            self.interrupt();
            return self.untrusted(false, true);
        }
        let floor = *self.noise_floor.get_or_insert(peak);
        let env = *self.speech_env.get_or_insert(peak);

        self.track_syllabic_band(peak, dt);
        self.track_flatness(peak, dt);

        let audible = measured_peak > self.audibility_floor;
        let voiced_now = admissible
            && audible
            && self.sounds_modulated()
            && peak - floor >= Self::SPEECH_MARGIN_DB
            && self.flat_s < Self::FLAT_S;
        // The hold bridges the gaps between words; it must not bridge a room.
        // Silence ends the hold outright, so a single tap on the desk buys half
        // a second of tracking and not a conversation's worth.
        self.hold_s = if voiced_now {
            Self::HOLD_S
        } else if audible && admissible {
            (self.hold_s - dt).max(0.0)
        } else {
            0.0
        };
        let active = voiced_now || self.hold_s > 0.0;

        let env = self.advance_envelope(env, peak, dt, active);
        let floor = self.advance_floor(floor, peak, dt);

        Features {
            speech_dbfs: env,
            noise_dbfs: floor,
            // Post-volume peaks cannot prove clipping at the ADC.
            raw_clip_fraction: 0.0,
            // Authorization, not a calibrated P(speech).
            speech_probability: if active { 1.0 } else { 0.0 },
            snr_db: if active { env - floor } else { peak - floor },
            muted: false,
            far_end_only: false,
            unreliable_double_talk: false,
            discontinuity: false,
        }
    }
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
