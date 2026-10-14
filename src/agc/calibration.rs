//! A calibration owns the actuator for the whole pass, not the settings lock.
//! It shares the daemon's controller/ACK path; silence never causes a blind lift.
use super::session::{FrameClock, WriteResult, actuator_lock, read_owned, same_meter, write_owned};
use super::*;
use crate::services::pipewire::SourceWatch;

const NO_AUDIO_TIMEOUT: Duration = Duration::from_secs(3);

/// Retains exclusive actuator ownership until the caller has persisted the
/// outcome. Dropping this value releases the lease; no audio worker remains.
pub struct CompletedCalibration {
    pub outcome: Outcome,
    /// Held, never read: dropping it is what releases the actuator lease.
    #[expect(dead_code, reason = "RAII lease; its only job is to be dropped")]
    owner: crate::config::storage::SettingsLock,
}

/// One calibration pass in progress: what it is listening to, and everything it
/// has learned so far. The accumulators outlive the loop because the outcome,
/// and the decision whether to restore the entry volume, are read from them
/// after the pass ends — including when it ended in an error.
struct Pass<'a> {
    watch: &'a SourceWatch,
    source: &'a Source,
    meter: &'a Source,
    analyzer: &'a AnalyzerConfig,
    controller: SourceController,
    clock: FrameClock,
    hop_s: f64,
    started: Instant,
    voiced_s: f64,
    speech_dbfs: Option<f64>,
    noise_dbfs: f64,
    tilt_sum: f64,
    tilt_n: u32,
    /// Whether the volume was ever moved. Nothing to restore if it was not.
    attempted_write: bool,
    /// The volume this pass believes it set. `None` means "unknown", which is
    /// never treated as "unchanged".
    expected: Option<f32>,
    last_audio: Instant,
    last_poll: Instant,
    last_write: Option<Instant>,
    settled_since: Option<Instant>,
}

impl Pass<'_> {
    /// Listen until the pass is satisfied, times out, or something under it
    /// changes. Returns Err for every reason the measurement cannot be trusted.
    fn listen(
        &mut self,
        events: &async_channel::Receiver<Event>,
        deadline: Instant,
        hard_deadline: Instant,
    ) -> Result<(), String> {
        loop {
            let tick = Instant::now();
            // Deadlines run on *all* paths, including Empty/Recovering events.
            if tick >= hard_deadline || (tick >= deadline && self.voiced_s < HEARD_SPEECH_S) {
                break;
            }
            if self.last_audio.elapsed() >= NO_AUDIO_TIMEOUT {
                return Err("microphone audio stalled for 3 seconds".into());
            }
            if !same_meter(self.watch, self.source, self.meter) {
                return Err("microphone or meter changed during calibration".into());
            }
            self.confirm_nobody_else_moved_it()?;
            match events.try_recv() {
                Ok(Event::Frame(frame)) => {
                    if self.on_frame(&frame)? {
                        break;
                    }
                }
                Ok(Event::Recovering(message)) => {
                    self.controller.interrupt();
                    self.clock.reset();
                    debug!("level: {message}");
                }
                Ok(Event::Fatal(message)) => return Err(format!("monitor failed: {message}")),
                Err(async_channel::TryRecvError::Closed) => {
                    return Err("the level monitor stopped".into());
                }
                Err(async_channel::TryRecvError::Empty) => {}
            }
            std::thread::sleep(TICK.saturating_sub(tick.elapsed()));
        }
        // Final readback too: a late external change is not a calibration result.
        let (actual, muted) = read_owned(self.source, self.watch)?;
        if muted
            || self
                .expected
                .is_none_or(|volume| volume_changed(actual, volume))
        {
            self.expected = None;
            return Err("microphone changed before calibration completed".into());
        }
        Ok(())
    }

    /// A calibration owns the actuator for its whole pass. Anything that moves
    /// the volume or the mute underneath it invalidates the measurement, so the
    /// pass reads the device back on a timer and refuses to continue.
    fn confirm_nobody_else_moved_it(&mut self) -> Result<(), String> {
        if self.last_poll.elapsed() < SOURCE_POLL {
            return Ok(());
        }
        self.last_poll = Instant::now();
        let (actual, muted) = read_owned(self.source, self.watch)?;
        if muted
            || self
                .expected
                .is_none_or(|volume| volume_changed(actual, volume))
        {
            self.expected = None;
            return Err("microphone volume or mute changed outside calibration".into());
        }
        Ok(())
    }

    /// Fold one analyzed frame into the pass. Returns true when the pass has
    /// heard enough and the level has stopped moving.
    fn on_frame(&mut self, frame: &SpectrumFrame) -> Result<bool, String> {
        let Some((dt, discontinuity)) = self.clock.take(frame, self.hop_s, Instant::now()) else {
            return Ok(false);
        };
        self.last_audio = Instant::now();
        if discontinuity {
            self.controller.interrupt();
            self.settled_since = None;
        }
        let step = self
            .controller
            .observe(f64::from(frame.peak_db), dt, false, true)?;
        self.noise_dbfs = step.features.noise_dbfs;
        let trusted = step.features.speech_probability >= 0.8
            && step.features.snr_db >= 10.0
            && matches!(step.decision.state, "tracking_speech" | "peak_safety");
        if trusted {
            // Never count missing audio as additional observed speech.
            self.voiced_s += self.hop_s;
            self.speech_dbfs = Some(step.features.speech_dbfs);
            if let Some(tilt) = tilt_db(frame, self.analyzer) {
                self.tilt_sum += tilt;
                self.tilt_n += 1;
            }
        }
        let wrote = self.apply(step.requested_volume)?;
        if wrote || !trusted || step.requested_volume.is_some() {
            self.settled_since = None;
        } else {
            self.settled_since.get_or_insert_with(Instant::now);
        }
        Ok(self.voiced_s >= HEARD_SPEECH_S
            && self.started.elapsed() >= MIN_LISTEN
            && self
                .settled_since
                .is_some_and(|at| at.elapsed() >= SETTLED_FOR))
    }

    /// Move the volume, if the controller asked and the write interval allows.
    /// Returns whether a write actually happened.
    fn apply(&mut self, requested: Option<f32>) -> Result<bool, String> {
        let Some(requested) = requested else {
            return Ok(false);
        };
        if self
            .last_write
            .is_some_and(|at| at.elapsed() < WRITE_INTERVAL)
        {
            return Ok(false);
        }
        // An error/unknown ACK makes restoration unsafe; never guess that a
        // command which timed out did not execute.
        self.attempted_write = true;
        self.expected = None;
        let status = write_owned(
            self.source,
            self.meter,
            self.watch,
            &mut self.controller,
            requested,
        )?;
        if status != WriteResult::Applied {
            return Err("external volume/mute change interrupted calibration".into());
        }
        self.expected = Some(self.controller.applied_volume());
        self.last_write = Some(Instant::now());
        self.clock.after_write();
        Ok(true)
    }
}

/// Compatibility API for callers that only need a measurement.
pub fn calibrate(seconds: f64) -> Result<Outcome, String> {
    calibrate_held(seconds).map(|completed| completed.outcome)
}

/// Listen for a bounded duration and keep ownership through the caller's save.
pub fn calibrate_held(seconds: f64) -> Result<CompletedCalibration, String> {
    if !seconds.is_finite() || seconds <= 0.0 || seconds > 60.0 {
        return Err("calibration duration must be finite and in (0, 60] seconds".into());
    }
    let owner = actuator_lock(Duration::from_secs(4)).map_err(|e| e.to_string())?;
    let watch = SourceWatch::start()?;
    let source = watch
        .default_source()
        .ok_or("no unambiguous real default microphone")?;
    let meter = watch
        .meter_source(&source)
        .ok_or("no unambiguous microphone meter")?;
    let (entry_volume, muted) = read_owned(&source, &watch)?;
    if muted || entry_volume == 0.0 {
        return Err("the microphone is muted or at zero volume".into());
    }
    let settings = AppSettings::load_strict().map_err(|e| e.to_string())?;
    let metered_filtered = meter.node_name != source.node_name;
    let controller = SourceController::for_calibration(
        entry_volume,
        f64::from(CALIBRATION_TARGET_DBFS),
        noise_ceiling(&settings, metered_filtered),
    )?;
    let base_db = controller.applied_db();
    let config = agc_monitor_config(
        meter
            .object_serial
            .clone()
            .ok_or("meter serial unavailable")?,
    );
    let analyzer = config.analyzer.clone();
    let hop_s = config.hop_size as f64 / f64::from(config.analyzer.sample_rate);
    let monitor = AudioMonitor::start(config);
    let events = monitor.events();
    let started = Instant::now();
    let deadline = started + Duration::from_secs_f64(seconds);
    let hard_deadline = deadline + PASS_GRACE;
    let mut pass = Pass {
        watch: &watch,
        source: &source,
        meter: &meter,
        analyzer: &analyzer,
        controller,
        clock: FrameClock::default(),
        hop_s,
        started,
        voiced_s: 0.0,
        speech_dbfs: None,
        noise_dbfs: -120.0,
        tilt_sum: 0.0,
        tilt_n: 0,
        attempted_write: false,
        expected: Some(entry_volume),
        last_audio: started,
        last_poll: started,
        last_write: None,
        settled_since: None,
    };
    monitor.set_active(true);
    let result = pass.listen(&events, deadline, hard_deadline);
    // Synchronous teardown on success AND every error path.
    monitor.shutdown();
    let heard_speech = pass.voiced_s >= HEARD_SPEECH_S;
    let needs_restore = result.is_err() || !heard_speech;
    let restored = if needs_restore && pass.attempted_write {
        restore(&source, &watch, entry_volume, pass.expected)
    } else {
        Ok(false)
    };
    if let Err(error) = result {
        return Err(match restored {
            Ok(true) => format!("{error}; initial volume restored and verified"),
            Ok(false) => error,
            Err(restore_error) => format!("{error}; initial volume NOT restored: {restore_error}"),
        });
    }
    let volume_restored = restored?;
    let voice = (heard_speech && pass.tilt_n > 0)
        .then(|| {
            pass.speech_dbfs.map(|level| VoiceEvidence {
                dbfs: (level - base_db) as f32,
                tilt_db: (pass.tilt_sum / f64::from(pass.tilt_n)) as f32,
                metered_filtered,
            })
        })
        .flatten();
    let volume = if heard_speech {
        pass.controller.applied_volume()
    } else {
        entry_volume
    };
    let (speech_dbfs, noise_dbfs) = (pass.speech_dbfs, pass.noise_dbfs);
    drop(pass); // releases the borrow of `source`, whose strings the outcome owns
    Ok(CompletedCalibration {
        outcome: Outcome {
            device: source.node_name,
            description: source.description,
            volume,
            speech_dbfs,
            noise_dbfs,
            heard_speech,
            volume_restored,
            voice,
        },
        owner,
    })
}

fn restore(
    source: &Source,
    watch: &SourceWatch,
    entry: f32,
    expected: Option<f32>,
) -> Result<bool, String> {
    let expected =
        expected.ok_or("last applied volume is unknown; preserving external ownership")?;
    let (actual, muted) = read_owned(source, watch)?;
    if muted || volume_changed(actual, expected) {
        return Err("volume/mute changed outside calibration".into());
    }
    if !volume_changed(actual, entry) {
        return Ok(false);
    }
    set_source_volume(source.node_id, entry).map_err(|e| e.to_string())?;
    let (actual, muted) = read_owned(source, watch)?;
    if muted || volume_changed(actual, entry) {
        return Err("restoration readback did not confirm initial volume".into());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_duration_is_rejected_before_any_io() {
        for seconds in [f64::NAN, f64::INFINITY, 0.0, -1.0, 61.0] {
            assert!(super::calibrate(seconds).unwrap_err().contains("duration"));
        }
    }
}
