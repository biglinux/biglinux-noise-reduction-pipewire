//! Capture level: short calibration passes, and a daemon bounded by what they found.
//!
//! Two jobs, one loop, because both do the same thing — meter the raw source that
//! is being actuated, normalize every observation before it becomes history, and
//! drive the capture volume through `wpctl` with a readback for every write.
//!
//! What they do NOT share is authority. A pass runs for a few seconds while the
//! person is deliberately speaking into their microphone, so it may move the
//! volume anywhere. The daemon runs all day next to televisions, other people and
//! open windows, so it may only move inside the window that person calibrated by
//! speaking close and far — and it exists only for devices where somebody chose
//! to draw that window.

mod calibration;
mod control;
mod features;
mod session;
pub use calibration::{CompletedCalibration, calibrate, calibrate_held};

use std::time::{Duration, Instant};

use log::{debug, info, warn};

use crate::config::{
    AppSettings, CALIBRATION_TARGET_DBFS, DeviceCalibration, VoiceEvidence, VoiceProfile,
};
#[cfg(test)]
use crate::gain_policy::GainPolicy;
use crate::gain_policy::NOISE_MARGIN_DB;
use crate::services::audio_monitor::{
    AnalyzerConfig, AudioMonitor, Event, MonitorConfig, SpectrumFrame, band_range_hz,
};
use crate::services::pipewire::{Source, set_source_volume};

use control::{SourceController, volume_changed};
#[cfg(test)]
use features::FeatureExtractor;

const TICK: Duration = Duration::from_millis(50);
const SOURCE_POLL: Duration = Duration::from_secs(1);
/// How long the loop sleeps between wake-ups while idle. Long enough to be
/// cheap, short enough that a shutdown or a settings change is not left waiting.
const IDLE_TICK: Duration = Duration::from_millis(250);
const SETTINGS_POLL: Duration = Duration::from_secs(1);
const STALE_AUDIO: Duration = Duration::from_millis(250);
/// Grace period before an idle microphone releases the capture. Short application
/// relinks (a call app switching devices, a page reload) must not cost a teardown
/// and a respawn, but an actually idle mic has to reach the point where the USB
/// device can suspend. Measured here: holding the meter open costs ~331 wakeups/s
/// and keeps the ALSA node out of `S` for as long as it lives.
const DEMAND_GRACE: Duration = Duration::from_secs(2);
// Keep 20 Hz decisions, but coalesce expensive CLI write/readback transactions.
// Pending rate-limited steps accumulate; this is not a second gain controller.
const WRITE_INTERVAL: Duration = Duration::from_millis(200);
/// A pass ends early once the volume has stopped moving for this long. Nobody
/// should stand there talking at a finished job.
const SETTLED_FOR: Duration = Duration::from_millis(1500);
/// However settled it looks, a pass listens at least this long before it may
/// end. A volume that never had to move is "settled" from the first tick, so
/// without a floor a pass could finish in about two seconds — on one syllable
/// and the room behind it, which is not a measurement of how somebody talks.
const MIN_LISTEN: Duration = Duration::from_secs(5);
/// Voiced audio a pass needs before it believes somebody spoke to it. Half a
/// second of real speech; a door closing is shorter, and a whole pass of nothing
/// never reaches it.
const HEARD_SPEECH_S: f64 = 0.5;
/// A pass that is still climbing when its time runs out gets this much longer.
/// A microphone that has to travel from 30% to 300% is doing exactly what it was
/// asked to do, and cutting it off there is how a working pass came back as
/// "I did not hear you".
const PASS_GRACE: Duration = Duration::from_secs(12);

/// What one calibration pass found. `volume` is the `wpctl` fraction it settled
/// on; `heard_speech` is false when the person never spoke, which is the one
/// outcome a caller must not save.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Outcome {
    pub device: String,
    pub description: String,
    pub volume: f32,
    pub speech_dbfs: Option<f64>,
    pub noise_dbfs: f64,
    pub heard_speech: bool,
    /// True when the pass put the microphone back where it found it, because it
    /// never heard anybody and has no business changing anything.
    pub volume_restored: bool,
    /// Spectral/level evidence for the daemon's conservative heuristic
    /// later (not speaker identification). Absent when nobody spoke, since then nothing was observed.
    #[serde(skip)]
    pub voice: Option<VoiceEvidence>,
}

/// Split of the legacy spectral-shape index. The statistic is a difference of
/// mean logarithmic band magnitudes, NOT a ratio of integrated band energies.
/// Distance, reverberation, microphone direction and EQ can all change it.
/// It cannot establish identity or reliably distinguish TV from a nearby voice.
const TILT_SPLIT_HZ: f64 = 2000.0;

/// Preserve the persisted index definition; changing to energy requires an
/// explicit calibration format migration, not silently reusing old thresholds.
fn tilt_db(frame: &SpectrumFrame, config: &AnalyzerConfig) -> Option<f64> {
    let (mut low, mut low_n, mut high, mut high_n) = (0.0_f64, 0_u32, 0.0_f64, 0_u32);
    for (index, band) in frame.bands_db.iter().enumerate() {
        let Some((start, _end)) = band_range_hz(config, index) else {
            continue;
        };
        let value = f64::from(*band);
        if !value.is_finite() {
            continue;
        }
        if start >= TILT_SPLIT_HZ {
            high += value;
            high_n += 1;
        } else {
            low += value;
            low_n += 1;
        }
    }
    (low_n > 0 && high_n > 0).then(|| high / f64::from(high_n) - low / f64::from(low_n))
}

/// The raw floor this controller measures is what the microphone's own converter
/// produces, not what the chain delivers: with noise reduction on, that hiss is
/// gone by the time anybody hears it. Vetoing a boost over it blocks every mic
/// whose converter is noisy — most built-in and webcam microphones.
fn noise_ceiling(settings: &AppSettings, metered_filtered: bool) -> Option<f64> {
    // A margin under the level we are aiming for, not a fixed dBFS.
    (!metered_filtered || settings.mic_bypass || !settings.noise_reduction.enabled)
        .then(|| f64::from(CALIBRATION_TARGET_DBFS) - NOISE_MARGIN_DB)
}

/// Analyze at the same 20 Hz cadence as the controller instead of computing
/// 93.75 spectra/s and discarding most of them. All 2400 new samples still
/// contribute to peak/RMS safety, even though the FFT window remains 2048.
fn agc_monitor_config(target: String) -> MonitorConfig {
    MonitorConfig {
        hop_size: 2400,
        hop_levels: true,
        latency_ms: 50,
        target,
        ..MonitorConfig::default()
    }
}

/// The controller settings a pass runs with: a correction bound wide enough to
/// carry a weak microphone the whole way to 300%, and a boost extreme that gets
/// it there while somebody is still talking.
#[cfg(test)]
fn pass_config(base_db: f64, noise_ceiling_dbfs: Option<f64>) -> crate::gain_policy::Config {
    crate::gain_policy::Config {
        target_speech_dbfs: f64::from(CALIBRATION_TARGET_DBFS),
        noise_ceiling_dbfs,
        max_boost_db: (control::MAX_VOL_DB - base_db).max(0.0),
        min_digital_db: (control::MIN_VOL_DB - base_db).min(0.0),
        rise_ceil_db_per_s: 8.0,
        ..crate::gain_policy::Config::default()
    }
}

/// The bounded daemon: no graph client when nobody opted into a range.
pub fn run() -> ! {
    info!("level: daemon started; acting only inside a calibrated window");
    let mut reported = String::new();
    // One connection for the daemon's life, replaced only when it dies: see
    // `SourceWatch::connect` for what reconnecting on every retry did.
    let mut watch: Option<crate::services::pipewire::SourceWatch> = None;
    loop {
        let enabled = AppSettings::load_strict().is_ok_and(|settings| {
            settings
                .mic_calibration
                .devices
                .values()
                .any(|d| d.window().is_some())
        });
        if !enabled {
            std::thread::sleep(SETTINGS_POLL);
            continue;
        }
        if !watch
            .as_ref()
            .is_some_and(crate::services::pipewire::SourceWatch::is_alive)
        {
            watch = None;
            match crate::services::pipewire::SourceWatch::connect() {
                Ok(connected) => watch = Some(connected),
                Err(error) => {
                    if error != reported {
                        warn!("level: session ended: {error}; retrying");
                        reported = error;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            }
        }
        let Some(current) = watch.as_ref() else {
            continue;
        };
        match drive(current) {
            Ok(()) => {
                reported.clear();
                std::thread::sleep(SETTINGS_POLL);
            }
            Err(error) => {
                if error != reported {
                    warn!("level: session ended: {error}; retrying");
                    reported = error;
                }
                // A metering error is never consent to change an ALSA card profile.
                // Explicit recovery in the UI remains available.
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn calibration_for(settings: &AppSettings, device: &str) -> Option<DeviceCalibration> {
    settings.mic_calibration.devices.get(device).copied()
}

/// The running daemon: what it is metering, and everything it must remember
/// between frames. One loop drives it; each concern below is one step of that
/// loop, so the order they run in is visible in `run` rather than buried.
struct Daemon<'a> {
    watch: &'a crate::services::pipewire::SourceWatch,
    source: &'a Source,
    meter: &'a Source,
    analyzer: AnalyzerConfig,
    config: MonitorConfig,
    monitor: AudioMonitor,
    events: async_channel::Receiver<Event>,
    controller: SourceController,
    clock: session::FrameClock,
    hop_s: f64,
    metering_filtered: bool,
    voice_profile: Option<VoiceProfile>,
    muted: bool,
    metering: bool,
    idle_since: Option<Instant>,
    last_settings: Instant,
    last_source: Instant,
    last_audio: Instant,
    last_log: Instant,
    last_write: Option<Instant>,
    restart_at: Option<Instant>,
    readback_ok: bool,
    paused_for_calibration: bool,
    /// Enforce an explicit range once, not once per idle poll: later manual
    /// changes must not be repeatedly clamped back by a silent microphone.
    enforce_range: bool,
}

impl Daemon<'_> {
    fn run(&mut self, mutex: &session::ActuatorMutex) -> Result<(), String> {
        loop {
            let tick = Instant::now();
            let Some(owner) = mutex.try_acquire().map_err(|e| e.to_string())? else {
                self.yield_to_calibration();
                std::thread::sleep(IDLE_TICK);
                continue;
            };
            if !session::same_meter(self.watch, self.source, self.meter) {
                return Ok(());
            }
            if !self.reload_settings()? {
                return Ok(()); // the range was withdrawn; there is nothing to drive
            }
            self.resume_from_calibration()?;
            self.follow_demand();
            self.enforce_range()?;
            if !self.metering {
                drop(owner);
                std::thread::sleep(IDLE_TICK.saturating_sub(tick.elapsed()));
                continue;
            }
            self.poll_source()?;
            if !self.restart_monitor_if_due() {
                drop(owner);
                std::thread::sleep(TICK);
                continue;
            }
            self.handle_event()?;
            if self.last_audio.elapsed() >= Duration::from_secs(3) && self.restart_at.is_none() {
                self.schedule_restart();
            }
            drop(owner);
            std::thread::sleep(TICK.saturating_sub(tick.elapsed()));
        }
    }

    /// A calibration pass owns the actuator. Stand down completely while it
    /// does, and do not re-enforce the range on the way back in.
    fn yield_to_calibration(&mut self) {
        if self.paused_for_calibration {
            return;
        }
        self.monitor.set_active(false);
        self.controller.interrupt();
        self.clock.reset();
        self.metering = false;
        self.paused_for_calibration = true;
        self.enforce_range = false;
    }

    /// Re-read the settings on a timer, and immediately after a calibration.
    /// Returns false when the device no longer has a usable range, which is the
    /// daemon's cue to stop rather than an error.
    fn reload_settings(&mut self) -> Result<bool, String> {
        if self.last_settings.elapsed() < SETTINGS_POLL && !self.paused_for_calibration {
            return Ok(true);
        }
        self.last_settings = Instant::now();
        let settings = AppSettings::load_strict().map_err(|e| e.to_string())?;
        let Some((low, high)) =
            calibration_for(&settings, &self.source.node_name).and_then(|d| d.window())
        else {
            return Ok(false);
        };
        let old = self.controller.window();
        if !self.controller.set_window(low, high) {
            return Err("invalid live gain window".into());
        }
        self.enforce_range |= self.controller.window() != old;
        self.voice_profile = calibration_for(&settings, &self.source.node_name)
            .and_then(|d| d.voice_profile(self.metering_filtered));
        self.controller
            .set_noise_ceiling(noise_ceiling(&settings, self.metering_filtered));
        Ok(true)
    }

    /// Whatever the calibration left behind is the new truth: adopt the volume
    /// it set instead of assuming the one this loop last wrote.
    fn resume_from_calibration(&mut self) -> Result<(), String> {
        if !self.paused_for_calibration {
            return Ok(());
        }
        let (actual, is_muted) = session::read_owned(self.source, self.watch)?;
        self.controller.external_volume_change(actual)?;
        self.muted = is_muted;
        self.paused_for_calibration = false;
        self.readback_ok = true;
        self.clock.reset();
        self.last_source = Instant::now();
        self.last_audio = Instant::now();
        Ok(())
    }

    /// Meter only while somebody is actually listening to the microphone, and
    /// stop a grace period after the last consumer goes away.
    fn follow_demand(&mut self) {
        if self.watch.has_consumer() {
            self.idle_since = None;
            if !self.metering {
                self.metering = true;
                self.monitor.set_active(true);
                self.controller.interrupt();
                self.clock.reset();
                self.last_audio = Instant::now();
                // Resuming after idle requires a fresh volume/mute reading.
                self.readback_ok = false;
                // Backdate the poll clock so the source is re-read on this very
                // iteration. Within the first SOURCE_POLL of the monotonic clock
                // there is nothing to subtract from, and `now` is already older
                // than any poll that happened.
                self.last_source = Instant::now()
                    .checked_sub(SOURCE_POLL)
                    .unwrap_or_else(Instant::now);
            }
        } else if self.idle_since.get_or_insert_with(Instant::now).elapsed() >= DEMAND_GRACE
            && self.metering
        {
            self.metering = false;
            self.monitor.set_active(false);
            self.controller.interrupt();
            self.clock.reset();
        }
    }

    /// Pull a volume left outside the calibrated range back into it, once per
    /// range change. A failure here is the actuator's, not the policy's.
    fn enforce_range(&mut self) -> Result<(), String> {
        if !self.enforce_range {
            return Ok(());
        }
        self.enforce_range = false;
        let Some((low, high)) = self.controller.window() else {
            return Ok(());
        };
        let (actual, is_muted) = session::read_owned(self.source, self.watch)?;
        if is_muted
            || actual <= 0.0
            || volume_changed(actual, self.controller.applied_volume())
            || (low..=high).contains(&actual)
        {
            return Ok(());
        }
        match session::write_owned(
            self.source,
            self.meter,
            self.watch,
            &mut self.controller,
            actual.clamp(low, high),
        ) {
            Ok(_) => {
                self.clock.after_write();
                self.last_write = Some(Instant::now());
            }
            Err(error) => {
                self.controller.actuator_fault();
                warn!("level: range enforcement failed: {error}");
            }
        }
        Ok(())
    }

    /// Read volume and mute back on a timer: anything that moved them behind
    /// this loop's back invalidates the evidence it has accumulated.
    fn poll_source(&mut self) -> Result<(), String> {
        if self.last_source.elapsed() < SOURCE_POLL {
            return Ok(());
        }
        self.last_source = Instant::now();
        match session::read_owned(self.source, self.watch) {
            Ok((actual, is_muted)) => {
                self.readback_ok = true;
                if self.muted != is_muted {
                    self.controller.interrupt();
                    self.clock.reset();
                }
                self.muted = is_muted;
                if volume_changed(actual, self.controller.applied_volume()) {
                    self.controller.external_volume_change(actual)?;
                    self.clock.reset();
                }
            }
            Err(error) => {
                self.readback_ok = false;
                self.controller.interrupt();
                self.clock.reset();
                debug!("level: {error}");
            }
        }
        Ok(())
    }

    /// Returns false while a scheduled monitor restart is still pending, which
    /// means this iteration has nothing else to do.
    fn restart_monitor_if_due(&mut self) -> bool {
        let Some(at) = self.restart_at else {
            return true;
        };
        if Instant::now() < at {
            return false;
        }
        let old = std::mem::replace(&mut self.monitor, AudioMonitor::start(self.config.clone()));
        old.shutdown();
        self.events = self.monitor.events();
        self.monitor.set_active(true);
        self.restart_at = None;
        self.last_audio = Instant::now();
        self.clock.reset();
        true
    }

    fn schedule_restart(&mut self) {
        self.controller.interrupt();
        self.clock.reset();
        self.monitor.set_active(false);
        self.restart_at = Some(Instant::now() + Duration::from_secs(2));
    }

    fn handle_event(&mut self) -> Result<(), String> {
        match self.events.try_recv() {
            Ok(Event::Frame(frame)) if self.readback_ok => self.on_frame(&frame)?,
            Ok(Event::Frame(_)) => {}
            Ok(Event::Recovering(message)) => {
                self.controller.interrupt();
                self.clock.reset();
                debug!("level: {message}");
            }
            Ok(Event::Fatal(message)) => {
                self.schedule_restart();
                warn!("level: monitor failed: {message}");
            }
            Err(async_channel::TryRecvError::Closed) => self.schedule_restart(),
            Err(async_channel::TryRecvError::Empty) => {
                if self.clock.stale() {
                    self.controller.interrupt();
                    self.clock.reset();
                }
            }
        }
        Ok(())
    }

    fn on_frame(&mut self, frame: &SpectrumFrame) -> Result<(), String> {
        let Some((dt, discontinuity)) = self.clock.take(frame, self.hop_s, Instant::now()) else {
            return Ok(());
        };
        self.last_audio = Instant::now();
        if discontinuity {
            self.controller.interrupt();
        }
        let tilt = tilt_db(frame, &self.analyzer);
        let level_at_unity = f64::from(frame.peak_db) - self.controller.applied_db();
        // A missing spectrum cannot satisfy an existing profile. This remains a
        // heuristic, NOT speaker identification.
        let admissible = self.voice_profile.is_none_or(|profile| {
            tilt.is_some_and(|tilt| profile.admits(level_at_unity as f32, tilt as f32))
        });
        let step = self
            .controller
            .observe(f64::from(frame.peak_db), dt, self.muted, admissible)?;
        self.log_step(frame, &step, admissible);
        self.apply(step.requested_volume);
        Ok(())
    }

    fn log_step(&mut self, frame: &SpectrumFrame, step: &control::ControlStep, admissible: bool) {
        if self.last_log.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_log = Instant::now();
        debug!(
            "level: peak={:.1} speech={:.1} snr={:.1} p={:.2} volume={:.2} target={:.1} state={} admit={} modulation={:.1} primary={:?} floor_bound={}",
            frame.peak_db,
            step.features.speech_dbfs,
            step.features.snr_db,
            step.features.speech_probability,
            self.controller.applied_volume(),
            step.target_db,
            step.decision.state,
            admissible,
            self.controller.modulation_db(),
            self.controller.primary_dbfs(),
            self.controller.noise_is_upper_bound()
        );
    }

    /// Write the requested volume, no more often than the write interval. An
    /// actuator failure faults the controller rather than propagating: the
    /// daemon keeps metering and stops trying to move anything.
    fn apply(&mut self, requested: Option<f32>) {
        let Some(requested) = requested else { return };
        if self
            .last_write
            .is_some_and(|at| at.elapsed() < WRITE_INTERVAL)
        {
            return;
        }
        self.last_write = Some(Instant::now());
        match session::write_owned(
            self.source,
            self.meter,
            self.watch,
            &mut self.controller,
            requested,
        ) {
            Ok(session::WriteResult::Applied) => self.clock.after_write(),
            Ok(_) => {
                self.controller.interrupt();
                self.clock.reset();
                self.readback_ok = false;
            }
            Err(error) => {
                self.controller.actuator_fault();
                self.clock.reset();
                warn!("level: actuator fault: {error}");
            }
        }
    }
}

fn drive(watch: &crate::services::pipewire::SourceWatch) -> Result<(), String> {
    let source = watch
        .default_source()
        .ok_or("no unambiguous real default microphone")?;
    let settings = AppSettings::load_strict().map_err(|e| e.to_string())?;
    let Some((low, high)) = calibration_for(&settings, &source.node_name).and_then(|d| d.window())
    else {
        return Ok(());
    };
    let meter = watch
        .meter_source(&source)
        .ok_or("no unambiguous microphone meter")?;
    let metering_filtered = meter.node_name != source.node_name;
    let (volume, muted) = session::read_owned(&source, watch)?;
    let mut controller = SourceController::new(
        volume,
        f64::from(CALIBRATION_TARGET_DBFS),
        noise_ceiling(&settings, metering_filtered),
    )?;
    if !controller.set_window(low, high) {
        return Err("range contains no writable nonzero volume".into());
    }
    let config = agc_monitor_config(
        meter
            .object_serial
            .clone()
            .ok_or("meter serial unavailable")?,
    );
    let analyzer = config.analyzer.clone();
    let hop_s = config.hop_size as f64 / f64::from(config.analyzer.sample_rate);
    let voice_profile = calibration_for(&settings, &source.node_name)
        .and_then(|d| d.voice_profile(metering_filtered));
    let monitor = AudioMonitor::start(config.clone());
    let events = monitor.events();
    let mutex = session::ActuatorMutex::open().map_err(|e| e.to_string())?;
    let now = Instant::now();
    let mut daemon = Daemon {
        watch,
        source: &source,
        meter: &meter,
        analyzer,
        config,
        monitor,
        events,
        controller,
        clock: session::FrameClock::default(),
        hop_s,
        metering_filtered,
        voice_profile,
        muted,
        metering: false,
        idle_since: Some(now),
        last_settings: now,
        last_source: now,
        last_audio: now,
        last_log: now,
        last_write: None,
        restart_at: None,
        readback_ok: true,
        paused_for_calibration: false,
        enforce_range: true,
    };
    let result = daemon.run(&mutex);
    daemon.monitor.shutdown();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The journey a pass exists for, and the one the reported failure never
    /// finished: a webcam left at 30% whose voice arrives at -45 dBFS when the
    /// volume is at 100%. Nothing short of the top is enough for it.
    #[test]
    fn a_pass_carries_a_weak_microphone_to_the_top_while_somebody_talks() {
        const DT: f64 = 0.05;
        let base_db = control::wpctl_to_db(0.3);
        let mut policy = GainPolicy::new(None, pass_config(base_db, None)).unwrap();
        let mut extractor = FeatureExtractor::new();
        let mut applied_db = base_db;
        for _ in 0..40 {
            extractor.observe(-95.0 + applied_db, applied_db - base_db, DT, true);
        }
        for i in 0..300 {
            // 15 s of ordinary speech, as the converter reports it at whatever
            // volume is currently set. A pass allows 8 s plus grace while it is
            // still climbing, which is what this journey needs.
            let voice = -45.0 + [-6.0, -3.0, 0.0, -3.0][(i / 3) % 4];
            let gain = applied_db - base_db;
            let features = extractor.observe(voice + applied_db, gain, DT, true);
            let decision = policy.update(&features, DT).unwrap();
            applied_db = (base_db + decision.digital_gain_db)
                .clamp(control::MIN_VOL_DB, control::MAX_VOL_DB);
        }
        let volume = control::db_to_wpctl(applied_db);
        assert!(volume > 2.9, "15 s of talking only reached {volume}");
    }
}
