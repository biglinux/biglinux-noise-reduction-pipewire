// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Display-free routing for standard MPRIS playback commands.

/// Player operations used by standard MPRIS commands.
pub struct BigMprisPlaybackControls<'a> {
    /// Toggle play/pause.
    pub toggle_pause: &'a dyn Fn(),
    /// Start playback.
    pub play: &'a dyn Fn(),
    /// Pause playback.
    pub pause: &'a dyn Fn(),
    /// Stop playback.
    pub stop: &'a dyn Fn(),
    /// Set playback volume.
    pub set_volume: &'a dyn Fn(f64),
}

/// Optional desktop feedback emitted after standard MPRIS commands.
pub struct BigMprisPlaybackFeedback<'a> {
    /// Read whether playback is currently paused.
    pub is_paused: &'a dyn Fn() -> bool,
    /// Read current playback position in seconds.
    pub position: &'a dyn Fn() -> f64,
    /// Read current media duration in seconds.
    pub duration: &'a dyn Fn() -> f64,
    /// Emit MPRIS playback-status changes.
    pub emit_playback_status: &'a dyn Fn(bool),
    /// Publish taskbar progress.
    pub update_taskbar_progress: &'a dyn Fn(f64, f64),
    /// Hide taskbar progress.
    pub hide_taskbar_progress: &'a dyn Fn(),
}

/// Route a common MPRIS playback command.
///
/// Returns `true` when the command was consumed. Window/playlist-specific
/// commands such as `raise`, `next`, `previous`, and seeking stay in the app.
pub fn route_standard_playback_command(
    command: &str,
    argument: Option<i64>,
    controls: &BigMprisPlaybackControls<'_>,
    feedback: Option<&BigMprisPlaybackFeedback<'_>>,
) -> bool {
    match command {
        "play-pause" => {
            (controls.toggle_pause)();
            if let Some(feedback) = feedback {
                let is_paused = (feedback.is_paused)();
                (feedback.emit_playback_status)(is_paused);
                if is_paused {
                    (feedback.hide_taskbar_progress)();
                } else {
                    (feedback.update_taskbar_progress)(
                        (feedback.position)(),
                        (feedback.duration)(),
                    );
                }
            }
            true
        }
        "play" => {
            (controls.play)();
            if let Some(feedback) = feedback {
                (feedback.emit_playback_status)(false);
                (feedback.update_taskbar_progress)((feedback.position)(), (feedback.duration)());
            }
            true
        }
        "pause" => {
            (controls.pause)();
            if let Some(feedback) = feedback {
                (feedback.emit_playback_status)(true);
                (feedback.hide_taskbar_progress)();
            }
            true
        }
        "stop" => {
            (controls.stop)();
            if let Some(feedback) = feedback {
                (feedback.hide_taskbar_progress)();
            }
            true
        }
        "set-volume" => {
            if let Some(volume) = argument {
                (controls.set_volume)(volume as f64);
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::{
        BigMprisPlaybackControls, BigMprisPlaybackFeedback, route_standard_playback_command,
    };

    #[derive(Debug, PartialEq)]
    enum PlaybackCall {
        TogglePause,
        Play,
        Pause,
        Stop,
        SetVolume(f64),
        Status(bool),
        Progress(f64, f64),
        HideProgress,
    }

    struct RecordedPlaybackControls {
        toggle_pause: Box<dyn Fn()>,
        play: Box<dyn Fn()>,
        pause: Box<dyn Fn()>,
        stop: Box<dyn Fn()>,
        set_volume: Box<dyn Fn(f64)>,
    }

    impl RecordedPlaybackControls {
        fn new(playback_call_log: Rc<RefCell<Vec<PlaybackCall>>>) -> Self {
            let toggle_pause_log = Rc::clone(&playback_call_log);
            let play_log = Rc::clone(&playback_call_log);
            let pause_log = Rc::clone(&playback_call_log);
            let stop_log = Rc::clone(&playback_call_log);
            let set_volume_log = Rc::clone(&playback_call_log);

            Self {
                toggle_pause: Box::new(move || {
                    toggle_pause_log
                        .borrow_mut()
                        .push(PlaybackCall::TogglePause);
                }),
                play: Box::new(move || {
                    play_log.borrow_mut().push(PlaybackCall::Play);
                }),
                pause: Box::new(move || {
                    pause_log.borrow_mut().push(PlaybackCall::Pause);
                }),
                stop: Box::new(move || {
                    stop_log.borrow_mut().push(PlaybackCall::Stop);
                }),
                set_volume: Box::new(move |volume| {
                    set_volume_log
                        .borrow_mut()
                        .push(PlaybackCall::SetVolume(volume));
                }),
            }
        }

        fn controls(&self) -> BigMprisPlaybackControls<'_> {
            BigMprisPlaybackControls {
                toggle_pause: self.toggle_pause.as_ref(),
                play: self.play.as_ref(),
                pause: self.pause.as_ref(),
                stop: self.stop.as_ref(),
                set_volume: self.set_volume.as_ref(),
            }
        }
    }

    struct RecordedPlaybackFeedback {
        is_paused: Box<dyn Fn() -> bool>,
        position: Box<dyn Fn() -> f64>,
        duration: Box<dyn Fn() -> f64>,
        emit_playback_status: Box<dyn Fn(bool)>,
        update_taskbar_progress: Box<dyn Fn(f64, f64)>,
        hide_taskbar_progress: Box<dyn Fn()>,
    }

    impl RecordedPlaybackFeedback {
        fn new(
            playback_call_log: Rc<RefCell<Vec<PlaybackCall>>>,
            is_paused: bool,
            position_seconds: f64,
            duration_seconds: f64,
        ) -> Self {
            let status_log = Rc::clone(&playback_call_log);
            let progress_log = Rc::clone(&playback_call_log);
            let hide_progress_log = Rc::clone(&playback_call_log);

            Self {
                is_paused: Box::new(move || is_paused),
                position: Box::new(move || position_seconds),
                duration: Box::new(move || duration_seconds),
                emit_playback_status: Box::new(move |is_paused| {
                    status_log
                        .borrow_mut()
                        .push(PlaybackCall::Status(is_paused));
                }),
                update_taskbar_progress: Box::new(move |position, duration| {
                    progress_log
                        .borrow_mut()
                        .push(PlaybackCall::Progress(position, duration));
                }),
                hide_taskbar_progress: Box::new(move || {
                    hide_progress_log
                        .borrow_mut()
                        .push(PlaybackCall::HideProgress);
                }),
            }
        }

        fn feedback(&self) -> BigMprisPlaybackFeedback<'_> {
            BigMprisPlaybackFeedback {
                is_paused: self.is_paused.as_ref(),
                position: self.position.as_ref(),
                duration: self.duration.as_ref(),
                emit_playback_status: self.emit_playback_status.as_ref(),
                update_taskbar_progress: self.update_taskbar_progress.as_ref(),
                hide_taskbar_progress: self.hide_taskbar_progress.as_ref(),
            }
        }
    }

    fn assert_playback_calls(
        playback_call_log: &Rc<RefCell<Vec<PlaybackCall>>>,
        expected_calls: &[PlaybackCall],
    ) {
        let recorded_calls = playback_call_log.borrow();
        assert_eq!(recorded_calls.as_slice(), expected_calls);
    }

    #[test]
    fn play_pause_reports_paused_state_and_hides_progress() {
        let playback_call_log = Rc::new(RefCell::new(Vec::new()));
        let recorded_controls = RecordedPlaybackControls::new(Rc::clone(&playback_call_log));
        let recorded_feedback =
            RecordedPlaybackFeedback::new(Rc::clone(&playback_call_log), true, 9.0, 90.0);
        let controls = recorded_controls.controls();
        let feedback = recorded_feedback.feedback();

        assert!(route_standard_playback_command(
            "play-pause",
            None,
            &controls,
            Some(&feedback),
        ));

        assert_playback_calls(
            &playback_call_log,
            &[
                PlaybackCall::TogglePause,
                PlaybackCall::Status(true),
                PlaybackCall::HideProgress,
            ],
        );
    }

    #[test]
    fn play_pause_reports_playing_state_and_updates_progress() {
        let playback_call_log = Rc::new(RefCell::new(Vec::new()));
        let recorded_controls = RecordedPlaybackControls::new(Rc::clone(&playback_call_log));
        let recorded_feedback =
            RecordedPlaybackFeedback::new(Rc::clone(&playback_call_log), false, 12.5, 125.0);
        let controls = recorded_controls.controls();
        let feedback = recorded_feedback.feedback();

        assert!(route_standard_playback_command(
            "play-pause",
            None,
            &controls,
            Some(&feedback),
        ));

        assert_playback_calls(
            &playback_call_log,
            &[
                PlaybackCall::TogglePause,
                PlaybackCall::Status(false),
                PlaybackCall::Progress(12.5, 125.0),
            ],
        );
    }

    #[test]
    fn play_reports_playing_state_and_updates_progress() {
        let playback_call_log = Rc::new(RefCell::new(Vec::new()));
        let recorded_controls = RecordedPlaybackControls::new(Rc::clone(&playback_call_log));
        let recorded_feedback =
            RecordedPlaybackFeedback::new(Rc::clone(&playback_call_log), true, 3.0, 30.0);
        let controls = recorded_controls.controls();
        let feedback = recorded_feedback.feedback();

        assert!(route_standard_playback_command(
            "play",
            None,
            &controls,
            Some(&feedback),
        ));

        assert_playback_calls(
            &playback_call_log,
            &[
                PlaybackCall::Play,
                PlaybackCall::Status(false),
                PlaybackCall::Progress(3.0, 30.0),
            ],
        );
    }

    #[test]
    fn pause_reports_paused_state_and_hides_progress() {
        let playback_call_log = Rc::new(RefCell::new(Vec::new()));
        let recorded_controls = RecordedPlaybackControls::new(Rc::clone(&playback_call_log));
        let recorded_feedback =
            RecordedPlaybackFeedback::new(Rc::clone(&playback_call_log), false, 1.0, 2.0);
        let controls = recorded_controls.controls();
        let feedback = recorded_feedback.feedback();

        assert!(route_standard_playback_command(
            "pause",
            None,
            &controls,
            Some(&feedback),
        ));

        assert_playback_calls(
            &playback_call_log,
            &[
                PlaybackCall::Pause,
                PlaybackCall::Status(true),
                PlaybackCall::HideProgress,
            ],
        );
    }

    #[test]
    fn stop_hides_progress_without_status_emit() {
        let playback_call_log = Rc::new(RefCell::new(Vec::new()));
        let recorded_controls = RecordedPlaybackControls::new(Rc::clone(&playback_call_log));
        let recorded_feedback =
            RecordedPlaybackFeedback::new(Rc::clone(&playback_call_log), false, 1.0, 2.0);
        let controls = recorded_controls.controls();
        let feedback = recorded_feedback.feedback();

        assert!(route_standard_playback_command(
            "stop",
            None,
            &controls,
            Some(&feedback),
        ));

        assert_playback_calls(
            &playback_call_log,
            &[PlaybackCall::Stop, PlaybackCall::HideProgress],
        );
    }

    #[test]
    fn set_volume_consumes_argument() {
        let playback_call_log = Rc::new(RefCell::new(Vec::new()));
        let recorded_controls = RecordedPlaybackControls::new(Rc::clone(&playback_call_log));
        let controls = recorded_controls.controls();

        assert!(route_standard_playback_command(
            "set-volume",
            Some(42),
            &controls,
            None,
        ));
        assert_playback_calls(&playback_call_log, &[PlaybackCall::SetVolume(42.0)]);
    }

    #[test]
    fn set_volume_without_argument_is_consumed_without_setting_volume() {
        let playback_call_log = Rc::new(RefCell::new(Vec::new()));
        let recorded_controls = RecordedPlaybackControls::new(Rc::clone(&playback_call_log));
        let controls = recorded_controls.controls();

        assert!(route_standard_playback_command(
            "set-volume",
            None,
            &controls,
            None,
        ));
        assert_playback_calls(&playback_call_log, &[]);
    }

    #[test]
    fn unknown_command_is_not_consumed() {
        let playback_call_log = Rc::new(RefCell::new(Vec::new()));
        let recorded_controls = RecordedPlaybackControls::new(Rc::clone(&playback_call_log));
        let controls = recorded_controls.controls();

        assert!(!route_standard_playback_command(
            "next", None, &controls, None,
        ));
        assert_playback_calls(&playback_call_log, &[]);
    }
}
