//! Shared control-plane safety. No audio callback acquires these locks.
use super::{
    STALE_AUDIO,
    control::{SourceController, volume_changed},
};
use crate::{
    config,
    services::{
        audio_monitor::SpectrumFrame,
        pipewire::{Source, SourceWatch},
    },
};
use std::time::{Duration, Instant};

pub(super) fn actuator_lock(
    timeout: Duration,
) -> Result<config::storage::SettingsLock, std::io::Error> {
    // Separate inode from the settings transaction: never hold settings.lock
    // while listening. All cooperating calibrations/AGC writes serialize here.
    config::storage::SettingsLock::at(&config::settings_file().with_extension("agc.lock"), timeout)
}

pub(super) fn same_meter(watch: &SourceWatch, source: &Source, meter: &Source) -> bool {
    watch
        .meter_source(source)
        .is_some_and(|now| meter.same_generation(&now))
}

#[derive(Default)]
pub(super) struct FrameClock {
    last: Option<(u64, u64, Instant)>,
    discard_before: Option<Instant>,
}
impl FrameClock {
    pub(super) fn reset(&mut self) {
        self.last = None;
        self.discard_before = Some(Instant::now());
    }
    /// Discard a frame already queued before an ACK. Delivery time does NOT
    /// measure ADC/filter latency: step-response tests must assess that delay.
    pub(super) fn after_write(&mut self) {
        self.discard_before = Some(Instant::now());
    }
    pub(super) fn take(
        &mut self,
        frame: &SpectrumFrame,
        hop_s: f64,
        now: Instant,
    ) -> Option<(f64, bool)> {
        if frame.captured_at > now
            || now.duration_since(frame.captured_at) >= STALE_AUDIO
            || self
                .discard_before
                .is_some_and(|at| frame.captured_at <= at)
        {
            return None;
        }
        let mut discontinuity = true;
        let mut dt = hop_s;
        if let Some((epoch, seq, at)) = self.last {
            if frame.capture_epoch == epoch && frame.seq <= seq {
                return None;
            }
            if frame.capture_epoch == epoch && frame.captured_at > at {
                let audio_dt = frame.seq.saturating_sub(seq) as f64 * hop_s;
                if audio_dt <= STALE_AUDIO.as_secs_f64()
                    && frame.captured_at.duration_since(at) < STALE_AUDIO
                {
                    discontinuity = false;
                    dt = audio_dt.min(0.1);
                }
            }
        }
        self.last = Some((frame.capture_epoch, frame.seq, frame.captured_at));
        Some((dt.min(0.1), discontinuity))
    }
    pub(super) fn stale(&self) -> bool {
        self.last
            .is_some_and(|(_, _, at)| at.elapsed() >= STALE_AUDIO)
    }
}

/// The source must still be ours on both sides of each CLI transaction. This
/// does not turn wpctl's numeric-ID write into an atomic generation-aware CAS.
pub(super) fn read_owned(source: &Source, watch: &SourceWatch) -> Result<(f32, bool), String> {
    if !watch.is_current(source) {
        return Err("default microphone generation changed".into());
    }
    let state = source
        .volume_and_mute()
        .ok_or("volume/mute readback failed")?;
    if !watch.is_current(source) {
        return Err("microphone changed during readback".into());
    }
    Ok(state)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WriteResult {
    Applied,
    External,
    Muted,
}

pub(super) fn write_owned(
    source: &Source,
    meter: &Source,
    watch: &SourceWatch,
    controller: &mut SourceController,
    requested: f32,
) -> Result<WriteResult, String> {
    let (actual, muted) = read_owned(source, watch)?;
    if muted || actual == 0.0 {
        controller.interrupt();
        return Ok(WriteResult::Muted);
    }
    if volume_changed(actual, controller.applied_volume()) {
        controller.external_volume_change(actual)?;
        return Ok(WriteResult::External);
    }
    if !same_meter(watch, source, meter) {
        return Err("meter generation changed before write".into());
    }
    crate::services::pipewire::set_source_volume(source.node_id, requested)
        .map_err(|e| e.to_string())?;
    let (actual, muted) = read_owned(source, watch)?;
    if muted || !same_meter(watch, source, meter) {
        return Err("source muted or meter changed during write".into());
    }
    controller.acknowledge_volume(requested, actual)?;
    Ok(WriteResult::Applied)
}

/// Keep one descriptor for the daemon. Only the nonblocking flock/unlock pair
/// runs per control tick; do not create directories/open files twenty times/s.
pub(super) struct ActuatorMutex(std::fs::File);
pub(super) struct ActuatorGuard<'a>(&'a std::fs::File);
impl ActuatorMutex {
    pub(super) fn open() -> std::io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let path = config::settings_file().with_extension("agc.lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::other("AGC lock is not a regular file"));
        }
        Ok(Self(file))
    }
    pub(super) fn try_acquire(&self) -> std::io::Result<Option<ActuatorGuard<'_>>> {
        match self.0.try_lock() {
            Ok(()) => Ok(Some(ActuatorGuard(&self.0))),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }
}
impl Drop for ActuatorGuard<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.0.unlock() {
            log::error!("AGC lock release failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::audio_monitor::{Analyzer, AnalyzerConfig};
    #[test]
    fn queued_frames_and_capture_restarts_cannot_reuse_evidence() {
        let mut analyzer = Analyzer::new(AnalyzerConfig::default());
        let mut frame = analyzer.analyze_samples(&vec![0.0; 2048]);
        frame.capture_epoch = 1;
        let mut clock = FrameClock::default();
        let now = Instant::now();
        frame.captured_at = now;
        assert_eq!(clock.take(&frame, 0.05, now), Some((0.05, true)));
        assert_eq!(clock.take(&frame, 0.05, now), None);
        frame.capture_epoch = 2;
        frame.seq = 0;
        frame.captured_at = now + Duration::from_millis(50);
        assert_eq!(
            clock.take(&frame, 0.05, frame.captured_at),
            Some((0.05, true))
        );
        assert_eq!(
            clock.take(&frame, 0.05, frame.captured_at + STALE_AUDIO),
            None
        );
        frame.captured_at = now;
        clock.after_write();
        assert_eq!(clock.take(&frame, 0.05, Instant::now()), None);
    }
    #[test]
    fn two_calibrations_cannot_own_the_same_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agc.lock");
        let first = config::storage::SettingsLock::at(&path, Duration::ZERO).unwrap();
        assert_eq!(
            config::storage::SettingsLock::at(&path, Duration::ZERO)
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(first);
        assert!(config::storage::SettingsLock::at(&path, Duration::ZERO).is_ok());
    }
}
