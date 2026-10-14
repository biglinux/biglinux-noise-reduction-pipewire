use super::*;

const DT: f64 = 0.05;
fn peak(i: usize, high: f64) -> f64 {
    high + [-6.0, -3.0, 0.0, -3.0][(i / 3) % 4]
}

fn tick(c: &mut SourceController, input_at_unity: f64) -> ControlStep {
    let step = c
        .observe(input_at_unity + c.applied_db(), DT, false, true)
        .unwrap();
    if let Some(v) = step.requested_volume {
        c.acknowledge_volume(v, v).unwrap();
    }
    step
}
fn background(c: &mut SourceController, floor_at_unity: f64) {
    for _ in 0..40 {
        tick(c, floor_at_unity);
    }
}

/// Like `tick`, but the converter cannot report above full scale — which is what
/// makes a crest a crest rather than an unbounded number.
fn tick_clipped(c: &mut SourceController, input_at_unity: f64) -> ControlStep {
    let step = c
        .observe((input_at_unity + c.applied_db()).min(0.0), DT, false, true)
        .unwrap();
    if let Some(v) = step.requested_volume {
        c.acknowledge_volume(v, v).unwrap();
    }
    step
}

#[test]
fn cubic_mapping_roundtrips_and_preserves_three_hundred_percent() {
    for db in [-40.0, -18.06, -6.0, 0.0, 6.0, 20.0, MAX_VOL_DB] {
        assert!((wpctl_to_db(db_to_wpctl(db)) - db).abs() < 1e-4);
    }
    assert!((wpctl_to_db(0.5) + 18.0618).abs() < 0.001);
    assert!((db_to_wpctl(MAX_VOL_DB) - 3.0).abs() < f32::EPSILON);
}

#[test]
fn clean_quiet_mic_reaches_target_comfort_zone() {
    let mut c = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    background(&mut c, -95.0);
    for i in 0..3000 {
        tick(&mut c, peak(i, -32.0));
    }
    let output_reference = -32.0 + c.applied_db();
    assert!(
        (-16.6..=-7.4).contains(&output_reference),
        "{output_reference}"
    );
    assert!(c.applied_volume() > 1.5);
}

#[test]
fn a_quiet_microphone_at_thirty_percent_is_raised_towards_the_target() {
    // The live reproduction: volume at 30%, target -6 dBFS, speech 45 dB below it
    // over a -69 dBFS floor. The controller used to sit at +0.0 dB forever.
    let mut c = SourceController::new(0.3, -6.0, None).unwrap();
    background(&mut c, -69.0);
    for i in 0..4000 {
        tick(&mut c, peak(i, -51.0));
    }
    assert!(c.applied_volume() > 0.3, "volume {}", c.applied_volume());
    assert!(c.applied_db() > 10.0, "gain {}", c.applied_db());
}

#[test]
fn a_calibration_pass_does_not_take_the_wheel_away() {
    // The reported failure: after calibrating, the level never moved again. The
    // pass writes the volume itself, the daemon saw a volume it had not asked
    // for, and manual ownership latches with no expiry — so every frame after a
    // calibration read `manual_hold` until the unit was restarted. A volume the
    // calibration window already licenses is not somebody taking the wheel.
    let mut c = SourceController::new(1.0, -12.0, None).unwrap();
    c.set_window(0.9, 1.6);
    assert!(
        !c.external_volume_change(1.42).unwrap(),
        "a licensed volume latched"
    );
    background(&mut c, -80.0);
    let mut tracked = 0;
    for i in 0..400 {
        if tick(&mut c, peak(i, -30.0)).decision.state == "tracking_speech" {
            tracked += 1;
        }
    }
    assert!(
        tracked > 0,
        "the controller stayed frozen after a calibration"
    );

    // Outside it there is no evidence, and whoever went there keeps the wheel.
    assert!(
        c.external_volume_change(2.8).unwrap(),
        "an unlicensed volume did not latch"
    );
    for i in 0..400 {
        assert_eq!(tick(&mut c, peak(i, -30.0)).decision.state, "manual_hold");
    }
}

#[test]
fn a_level_outside_the_range_is_brought_back_without_waiting_for_speech() {
    // Reported: switching the automatic level on, or moving the two ends, did
    // nothing at all. Writes only ever happened while tracking trusted speech,
    // so a range the person had just drawn sat there being broken until they
    // talked — indistinguishable from a switch wired to nothing.
    let mut c = SourceController::new(2.5, -12.0, None).unwrap();
    c.set_window(1.0, 1.6);
    let step = c.observe(-120.0, DT, false, true).unwrap();
    let requested = step
        .requested_volume
        .expect("silence left the level out of range");
    assert!((1.0..=1.6).contains(&requested), "asked for {requested}");

    // Once inside, silence asks for nothing more.
    c.acknowledge_volume(requested, requested).unwrap();
    for _ in 0..20 {
        assert!(
            c.observe(-120.0, DT, false, true)
                .unwrap()
                .requested_volume
                .is_none()
        );
    }
}

#[test]
fn a_frame_that_is_not_the_calibrated_voice_moves_nothing() {
    // The daemon's answer to a talker in the next room and to the room itself:
    // the calibration measured what this person's voice looks like, and a frame
    // that cannot be it is evidence about nobody. It still teaches the floor —
    // it is genuinely part of the room — it simply never counts as speech.
    let mut c = SourceController::new(1.0, -12.0, None).unwrap();
    background(&mut c, -80.0);
    for i in 0..400 {
        let step = c
            .observe(peak(i, -30.0) + c.applied_db(), DT, false, false)
            .unwrap();
        assert_eq!(step.features.speech_probability, 0.0);
        assert!(
            step.requested_volume.is_none(),
            "an inadmissible frame moved the volume"
        );
    }
    // The same signal, admitted, is the voice this controller exists for.
    let mut voiced = 0;
    for i in 0..400 {
        if tick(&mut c, peak(i, -30.0)).features.speech_probability > 0.0 {
            voiced += 1;
        }
    }
    assert!(voiced > 100, "only {voiced} of 400 admitted frames counted");
}

#[test]
fn an_occasional_crest_does_not_drag_the_level_down() {
    // A knock on the desk once a second is not the speaking level. It used to
    // become one — instant attack, frozen envelope — and the controller then
    // turned the microphone down to put the knock at the target.
    let mut c = SourceController::new(1.0, -12.0, None).unwrap();
    for _ in 0..40 {
        tick_clipped(&mut c, -60.0);
    }
    for i in 0..8000 {
        let crest = if i % 20 == 0 { 12.0 } else { 0.0 };
        tick_clipped(&mut c, peak(i, -30.0) + crest);
    }
    let speech_dbfs = -30.0 + c.applied_db();
    assert!(
        (-20.0..-6.0).contains(&speech_dbfs),
        "speech ended at {speech_dbfs} dBFS"
    );
    assert!(
        c.applied_volume() > 1.0,
        "turned it down to {}",
        c.applied_volume()
    );
}

#[test]
fn the_daemon_never_leaves_the_calibrated_window() {
    // A microphone weak enough to ask for 300%, and loud enough to ask for a deep
    // cut, still only moves between the two volumes its owner established by
    // speaking close and far. That window is the whole of the daemon's authority.
    let mut quiet = SourceController::new(1.0, -12.0, None).unwrap();
    quiet.set_window(0.8, 1.2);
    background(&mut quiet, -100.0);
    for i in 0..4000 {
        tick(&mut quiet, peak(i, -48.0));
    }
    assert!(
        quiet.applied_volume() <= 1.2,
        "climbed to {}",
        quiet.applied_volume()
    );
    assert!(
        quiet.applied_volume() > 1.0,
        "never climbed: {}",
        quiet.applied_volume()
    );

    let mut loud = SourceController::new(1.0, -12.0, None).unwrap();
    loud.set_window(0.8, 1.2);
    for _ in 0..2000 {
        tick(&mut loud, 5.0);
    }
    assert!(
        loud.applied_volume() >= 0.8,
        "fell to {}",
        loud.applied_volume()
    );
    assert!(
        loud.applied_volume() < 1.0,
        "never fell: {}",
        loud.applied_volume()
    );
}

#[test]
fn weak_mic_can_reach_exactly_three_hundred_percent() {
    let mut c = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    background(&mut c, -105.0);
    for i in 0..4000 {
        tick(&mut c, peak(i, -48.0));
    }
    assert!((c.applied_volume() - 3.0).abs() < f32::EPSILON);
    assert!(c.policy.current_gain_db() <= MAX_VOL_DB + 1e-6);
}

#[test]
fn a_distant_source_cannot_quickly_drive_gain_up() {
    let mut c = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    background(&mut c, -100.0);
    for i in 0..2000 {
        tick(&mut c, peak(i, -35.0));
    }
    let before = c.applied_db();
    // 25 dB below the learned reference: half a minute of it must not chase the
    // room up, so a distant voice or a television cannot raise the gain under a
    // speaker who has merely paused.
    for i in 0..600 {
        tick(&mut c, peak(i, -60.0));
    }
    let drifted = c.applied_db() - before;
    assert!(
        (0.0..8.0).contains(&drifted),
        "drifted {drifted} dB in 30 s"
    );
    // The owner speaking again raises the reference at once, so their own voice
    // is back inside the comfort zone around the target rather than over it.
    for i in 0..2000 {
        tick(&mut c, peak(i, -35.0));
    }
    let output = -35.0 + c.applied_db();
    assert!((-16.6..=-7.4).contains(&output), "{output}");
}

#[test]
fn no_startup_jump_on_noise_low_volume_zero_or_mute() {
    for volume in [0.0, 0.1, 0.3, 1.0, 3.0] {
        let mut c = SourceController::new(volume, -12.0, Some(-50.0)).unwrap();
        for _ in 0..400 {
            let step = c.observe(-70.0, DT, false, true).unwrap();
            assert!(step.requested_volume.is_none());
        }
        assert_eq!(c.applied_volume(), volume);
    }
    let mut c = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    for i in 0..400 {
        let step = c.observe(peak(i, 10.0), DT, true, true).unwrap();
        assert_eq!(step.decision.state, "muted");
        assert!(step.requested_volume.is_none());
    }
}

#[test]
fn over_high_start_reduces_without_waiting_for_vad() {
    let mut c = SourceController::new(3.0, -12.0, Some(-50.0)).unwrap();
    let before = c.applied_db();
    for _ in 0..40 {
        tick(&mut c, 5.0);
    }
    assert!(before - c.applied_db() > 20.0);
    // Peak safety acted on its own. A level that loud, and that steady, is not
    // somebody speaking, so nothing here may authorize a boost either.
    assert_eq!(
        c.observe(5.0, DT, false, true)
            .unwrap()
            .features
            .speech_probability,
        0.0
    );
}

#[test]
fn manual_change_zeros_old_correction_and_remains_held() {
    let mut c = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    background(&mut c, -100.0);
    for i in 0..500 {
        tick(&mut c, peak(i, -40.0));
    }
    assert!(c.policy.current_gain_db() > 5.0);
    c.external_volume_change(0.7).unwrap();
    assert_eq!(c.policy.current_gain_db(), 0.0);
    assert_eq!(c.applied_volume(), 0.7);
    c.interrupt();
    for i in 0..1200 {
        let step = c.observe(peak(i, 20.0), DT, false, true).unwrap();
        assert_eq!(step.decision.state, "manual_hold");
        assert!(step.requested_volume.is_none());
    }
}

#[test]
fn source_restart_has_independent_calibration() {
    let mut first = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    background(&mut first, -90.0);
    for i in 0..300 {
        tick(&mut first, peak(i, -30.0));
    }
    assert!(first.primary_dbfs().is_some());
    let second = SourceController::new(0.5, -12.0, Some(-50.0)).unwrap();
    assert!(second.primary_dbfs().is_none());
    assert_eq!(second.policy.current_gain_db(), 0.0);
}

#[test]
fn readback_fault_never_retries_using_an_assumed_applied_gain() {
    let mut c = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    c.actuator_fault();
    for i in 0..400 {
        let step = c.observe(peak(i, 30.0), DT, false, true).unwrap();
        assert_eq!(step.decision.state, "actuator_fault");
        assert!(step.requested_volume.is_none());
    }
    assert_eq!(c.applied_volume(), 1.0);
}

#[test]
fn coarse_readback_steps_do_not_stall_all_boosting() {
    let mut c = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    background(&mut c, -100.0);
    for i in 0..4000 {
        let step = c
            .observe(peak(i, -48.0) + c.applied_db(), DT, false, true)
            .unwrap();
        if let Some(requested) = step.requested_volume {
            let coarse = ((requested * 10.0).round() / 10.0).clamp(0.1, 3.0);
            c.acknowledge_volume(requested, coarse).unwrap();
        }
    }
    assert!((c.applied_volume() - 3.0).abs() < f32::EPSILON);
}

#[test]
fn unchanged_maximum_request_is_not_repeated_forever() {
    let mut c = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    background(&mut c, -100.0);
    let mut late_requests = 0;
    for i in 0..4000 {
        let step = c
            .observe(peak(i, -48.0) + c.applied_db(), DT, false, true)
            .unwrap();
        if let Some(requested) = step.requested_volume {
            if i > 3000 {
                late_requests += 1;
            }
            c.acknowledge_volume(requested, requested.min(1.5)).unwrap();
        }
    }
    assert_eq!(late_requests, 0);
    assert_eq!(c.applied_volume(), 1.5);
}

#[test]
fn pending_steps_survive_five_hz_write_coalescing() {
    let mut c = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    background(&mut c, -100.0);
    for i in 0..4000 {
        let step = c
            .observe(peak(i, -48.0) + c.applied_db(), DT, false, true)
            .unwrap();
        if i % 4 == 0
            && let Some(v) = step.requested_volume
        {
            c.acknowledge_volume(v, v).unwrap();
        }
    }
    assert_eq!(c.applied_volume(), 3.0);
}

#[test]
fn invalid_volume_is_rejected_and_readback_comparison_is_quantized() {
    for bad in [f32::NAN, f32::INFINITY, -0.1, 3.1] {
        assert!(SourceController::new(bad, -12.0, Some(-50.0)).is_err());
    }
    assert!(!volume_changed(0.21, 0.21));
    assert!(volume_changed(0.22, 0.21));
}

#[test]
fn quiet_start_with_censored_noise_calibrates_without_a_volume_jump() {
    let mut c = SourceController::new(0.3, -12.0, Some(-50.0)).unwrap();
    background(&mut c, -95.0); // Measured noise < -120 dBFS at the initial volume.
    assert_eq!(c.applied_volume(), 0.3);
    for i in 0..4000 {
        // Natural word gaps expose the changing raw noise floor as gain rises.
        tick(&mut c, if i % 80 > 65 { -95.0 } else { peak(i, -32.0) });
    }
    assert!((-16.6..=-7.4).contains(&(-32.0 + c.applied_db())));
    assert!(c.applied_volume() > 1.5);
}

#[test]
fn a_raw_hiss_only_vetoes_the_boost_when_it_reaches_a_listener() {
    // Measured on this project's own hardware: a webcam converter hisses at
    // -39 dBFS raw while the chain delivers digital silence. Judging the boost
    // on the raw floor blocks the level on every microphone with a noisy
    // converter, so the veto has to follow the noise somebody can hear.
    let speak = |c: &mut SourceController| {
        background(c, -60.0);
        for i in 0..4000 {
            tick(c, peak(i, -30.0));
        }
    };
    let mut vetoed = SourceController::new(1.0, -12.0, Some(-50.0)).unwrap();
    speak(&mut vetoed);
    let mut delivered_clean = SourceController::new(1.0, -12.0, None).unwrap();
    speak(&mut delivered_clean);
    assert!(vetoed.applied_db() < 8.0, "vetoed {}", vetoed.applied_db());
    assert!(
        delivered_clean.applied_db() > 12.0,
        "clean {}",
        delivered_clean.applied_db()
    );
}

#[test]
fn rejected_speech_after_tracking_cannot_keep_boosting() {
    let mut c = SourceController::new(1.0, -12.0, None).unwrap();
    background(&mut c, -90.0);
    for i in 0..80 {
        tick(&mut c, peak(i, -40.0));
    }
    for _ in 0..30 {
        let step = c.observe(-60.0, DT, false, false).unwrap();
        assert_eq!(step.features.speech_probability, 0.0);
        assert!(step.requested_volume.is_none());
    }
}

#[test]
fn narrow_window_does_not_leave_gain_wound_up_above_the_actuator() {
    let mut c = SourceController::new(1.0, -12.0, None).unwrap();
    assert!(c.set_window(0.8, 1.2));
    background(&mut c, -100.0);
    for i in 0..2000 {
        tick(&mut c, peak(i, -45.0));
    }
    assert!(c.policy.current_gain_db() <= wpctl_to_db(1.2) + 1e-6);
}

#[test]
fn opposite_direction_ack_is_not_our_write() {
    let mut c = SourceController::new(1.0, -12.0, None).unwrap();
    assert!(c.acknowledge_volume(1.1, 0.9).is_err());
    assert!(c.faulted);
}

#[test]
fn integer_percent_windows_do_not_shrink_from_binary_float_roundoff() {
    for percent in 1..=300 {
        let volume = percent as f32 / 100.0;
        let mut controller = SourceController::new(volume, -12.0, None).unwrap();
        assert!(
            controller.set_window(volume, volume),
            "{percent}% became empty"
        );
        assert_eq!(controller.window(), Some((volume, volume)));
    }
}

#[test]
fn loud_audio_cannot_turn_an_external_below_window_change_into_an_error() {
    let mut controller = SourceController::new(1.5, -12.0, None).unwrap();
    assert!(controller.set_window(1.0, 2.0));
    assert!(controller.external_volume_change(0.5).unwrap());
    for _ in 0..100 {
        let step = controller.observe(0.0, 0.05, false, true).unwrap();
        assert!(step.requested_volume.is_none());
    }
}
