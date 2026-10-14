use super::*;

const DT: f64 = 0.05;

fn background(fx: &mut FeatureExtractor, level: f64) {
    for _ in 0..40 {
        fx.observe(level, 0.0, DT, true);
    }
}

fn voice_peak(i: usize, high: f64) -> f64 {
    high + [-6.0, -3.0, 0.0, -3.0][(i / 3) % 4]
}

#[test]
fn a_voice_over_the_floor_is_speech_within_a_second() {
    // What the old extractor could not do: a webcam in a normal room. Converter
    // hiss at -37 dBFS, speech peaking around -19. It used to need half of every
    // two seconds above 18 dB of contrast and so never authorized anything.
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -37.0);
    // Rhythm has to be heard before it can be recognised: the detector needs
    // about three quarters of a second of syllables before it will authorize
    // anything, which is why the count below starts at the second half of the
    // first second rather than at the first frame. What the original complaint
    // was about — a voice that never authorized anything at all — is what the
    // second count asserts.
    let mut authorized = 0;
    for i in 0..20 {
        if fx
            .observe(voice_peak(i, -19.0), 0.0, DT, true)
            .speech_probability
            == 1.0
        {
            authorized += 1;
        }
    }
    assert!(authorized > 0, "a second of speech authorized nothing");
    let mut steady = 0;
    for i in 20..40 {
        if fx
            .observe(voice_peak(i, -19.0), 0.0, DT, true)
            .speech_probability
            == 1.0
        {
            steady += 1;
        }
    }
    assert!(
        steady > 15,
        "only {steady} of the next 20 frames counted as speech"
    );
    // One second in, the envelope is most of the way to the voice; two, and it
    // is there. That is the cost of not adopting the first loud frame it sees.
    for i in 0..20 {
        fx.observe(voice_peak(i, -19.0), 0.0, DT, true);
    }
    let level = fx.primary_dbfs().unwrap();
    assert!((level + 19.0).abs() < 2.0, "envelope {level}");
}

#[test]
fn an_empty_room_is_never_speech_however_far_it_stands_over_the_floor() {
    // The reported failure, from this workstation's own journal: with nobody
    // talking, the source read about -110 dBFS over a floor censored at -120,
    // which cleared the 6 dB margin. The daemon called it speech (`p=1.00`,
    // `snr=84`), chased the decaying envelope, and walked the capture volume to
    // its ceiling — so a few seconds of silence turned the microphone up to
    // maximum every time. Silence is not quiet speech; it is not speech.
    let mut fx = FeatureExtractor::new();
    fx.observe(-125.0, 0.0, DT, true); // censored: the floor is an upper bound, not a measurement
    let mut authorized = 0;
    for i in 0..40 {
        // Room tone wanders a little, so the flat-level test alone never caught it.
        let jitter = [0.0, -2.5, 1.5, -4.0][i % 4];
        if fx
            .observe(-100.0 + jitter, 0.0, DT, true)
            .speech_probability
            > 0.0
        {
            authorized += 1;
        }
    }
    assert_eq!(
        authorized, 0,
        "{authorized} frames of an empty room authorized a boost"
    );
}

#[test]
fn a_quiet_voice_on_a_weak_microphone_still_counts() {
    // The boundary the silence test must not cross: a poor capsule at a low
    // volume, whose own noise the analyzer cannot even see, so the floor is a
    // bound too — and the whole point of the daemon is to bring that voice up.
    let mut fx = FeatureExtractor::new();
    fx.observe(-125.0, 0.0, DT, true);
    let mut authorized = 0;
    for i in 0..40 {
        if fx
            .observe(voice_peak(i, -76.0), 0.0, DT, true)
            .speech_probability
            == 1.0
        {
            authorized += 1;
        }
    }
    assert!(
        authorized > 15,
        "only {authorized} of 40 quiet-voice frames counted"
    );
}

/// A level envelope modulated at `hz`, `depth_db` peak to peak, around `mean`.
/// This is what speech does to a level meter and what machinery never does.
fn modulated(i: usize, mean: f64, depth_db: f64, hz: f64) -> f64 {
    let t = i as f64 * DT;
    mean + 0.5 * depth_db * (t * std::f64::consts::TAU * hz).sin()
}

#[test]
fn a_steady_machine_is_not_speech_however_far_over_the_floor_it_sits() {
    // The complaint this answers: background noise moving the gain. A fan, a
    // fridge or a duct is stationary — it is loud, it clears any margin over the
    // floor, and it modulates its own level by almost nothing. Nothing about
    // this test is a property of one room: the signal is defined by its shape.
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -70.0);
    let mut authorized = 0;
    for i in 0..400 {
        // A machine 25 dB over the floor, wandering a little as machines do.
        let level = modulated(i, -45.0, 1.0, 0.2);
        if fx.observe(level, 0.0, DT, true).speech_probability > 0.0 {
            authorized += 1;
        }
    }
    assert_eq!(
        authorized, 0,
        "{authorized} frames of steady machinery counted as speech"
    );
}

#[test]
fn a_voice_at_the_syllable_rate_is_speech_at_any_level() {
    // The same shape at three very different levels, on what could be three
    // different microphones: a level statistic that only works at one of them
    // would be a threshold tuned to somebody's room.
    for mean in [-20.0, -40.0, -60.0] {
        let mut fx = FeatureExtractor::new();
        background(&mut fx, mean - 25.0);
        let mut authorized = 0;
        for i in 0..400 {
            let level = modulated(i, mean, 12.0, 4.0);
            if fx.observe(level, 0.0, DT, true).speech_probability > 0.0 {
                authorized += 1;
            }
        }
        assert!(
            authorized > 200,
            "only {authorized} of 400 frames at {mean} dBFS counted"
        );
    }
}

#[test]
fn one_slammed_door_does_not_become_the_speaking_level() {
    // The reported failure: a pass that turned the microphone DOWN. A single
    // frame far above the voice used to be adopted as the speaking level, and
    // the envelope is frozen in silence, so it stayed there for the whole pass.
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -60.0);
    for i in 0..60 {
        fx.observe(voice_peak(i, -30.0), 0.0, DT, true);
    }
    let speaking = fx.primary_dbfs().unwrap();
    fx.observe(-2.0, 0.0, DT, true); // the door
    let after = fx.primary_dbfs().unwrap();
    assert!(
        after - speaking < 6.0,
        "one transient moved the level {} dB",
        after - speaking
    );
    for i in 0..60 {
        fx.observe(voice_peak(i, -30.0), 0.0, DT, true);
    }
    assert!(
        (fx.primary_dbfs().unwrap() - speaking).abs() < 3.0,
        "it never came back"
    );
}

#[test]
fn a_gap_between_words_does_not_end_the_speech() {
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -60.0);
    // Four seconds, not two: the first three quarters of a second is the
    // modulation detector learning the rhythm, and the envelope only starts
    // climbing once frames are being authorized. Two seconds left it still
    // 3 dB short of the voice, which is a measurement of the warm-up and not
    // of what a word gap does.
    for i in 0..80 {
        fx.observe(voice_peak(i, -25.0), 0.0, DT, true);
    }
    // 0.3 s of quiet between two words: still speaking, and the envelope stays
    // at the speaking level instead of sliding toward the floor.
    for _ in 0..6 {
        assert_eq!(fx.observe(-58.0, 0.0, DT, true).speech_probability, 1.0);
    }
    assert!(
        fx.primary_dbfs().unwrap() > -30.0,
        "{:?}",
        fx.primary_dbfs()
    );
}

#[test]
fn real_silence_freezes_the_level_instead_of_decaying_it() {
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -60.0);
    for i in 0..40 {
        fx.observe(voice_peak(i, -25.0), 0.0, DT, true);
    }
    // The half-second hold bridges word gaps; past it, nothing is speech any
    // more, and nothing may authorize a boost of the room itself.
    for _ in 0..12 {
        fx.observe(-59.0, 0.0, DT, true);
    }
    let speaking = fx.primary_dbfs().unwrap();
    for _ in 0..600 {
        assert_eq!(fx.observe(-59.0, 0.0, DT, true).speech_probability, 0.0);
    }
    assert!((fx.primary_dbfs().unwrap() - speaking).abs() < 0.01);
}

#[test]
fn steady_noise_is_absorbed_into_the_floor_rather_than_mistaken_for_a_voice() {
    // A fan that starts is louder than the room, briefly clears the margin, and
    // is then part of the room. It must not hold the pass open indefinitely.
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -80.0);
    let mut voiced_frames = 0;
    for _ in 0..2000 {
        if fx.observe(-45.0, 0.0, DT, true).speech_probability == 1.0 {
            voiced_frames += 1;
        }
    }
    assert!(
        voiced_frames < 400,
        "a fan counted as speech for {voiced_frames} frames"
    );
    assert_eq!(fx.observe(-45.0, 0.0, DT, true).speech_probability, 0.0);
}

#[test]
fn normalize_before_history_even_when_volume_changes() {
    // The same acoustic voice at two different capture volumes is one level.
    let mut quiet = FeatureExtractor::new();
    background(&mut quiet, -60.0);
    for i in 0..60 {
        quiet.observe(voice_peak(i, -30.0), 0.0, DT, true);
    }
    let mut loud = FeatureExtractor::new();
    background(&mut loud, -50.0);
    for i in 0..60 {
        loud.observe(voice_peak(i, -20.0), 10.0, DT, true);
    }
    let (a, b) = (quiet.primary_dbfs().unwrap(), loud.primary_dbfs().unwrap());
    assert!((a - b).abs() < 0.5, "{a} vs {b}");
}

#[test]
fn rebase_moves_the_coordinates_not_the_speaker() {
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -70.0);
    for i in 0..60 {
        fx.observe(voice_peak(i, -35.0), 0.0, DT, true);
    }
    let before = fx.primary_dbfs().unwrap();
    fx.rebase(8.0);
    assert!((fx.primary_dbfs().unwrap() - (before + 8.0)).abs() < 1e-9);
    fx.rebase(f64::NAN);
    assert_eq!(
        fx.primary_dbfs(),
        None,
        "a meaningless baseline keeps no level"
    );
}

#[test]
fn censored_noise_is_a_finite_bound_and_zero_never_authorizes_boost() {
    let mut fx = FeatureExtractor::new();
    // A mic so quiet the analyzer reports its floor as censored: that is an upper
    // bound on the noise at the baseline, not silence and not a mute.
    for _ in 0..40 {
        let feature = fx.observe(-130.0, 12.0, DT, true);
        assert_eq!(feature.speech_probability, 0.0);
        assert!(feature.noise_dbfs.is_finite());
        assert!(!feature.muted);
    }
    assert!(fx.noise_is_upper_bound());
    assert!((fx.observe(-40.0, 12.0, DT, true).noise_dbfs + 132.0).abs() < 1.0);
}

#[test]
fn invalid_input_is_fail_closed() {
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -60.0);
    for (peak, gain, dt) in [
        (f64::NAN, 0.0, DT),
        (-30.0, f64::INFINITY, DT),
        (-30.0, 0.0, 0.0),
        (-30.0, 0.0, 0.5),
        (-30.0, 0.0, f64::NAN),
    ] {
        let feature = fx.observe(peak, gain, dt, true);
        assert!(feature.discontinuity);
        assert_eq!(feature.speech_probability, 0.0);
    }
}

#[test]
fn a_muted_source_supplies_no_calibration_evidence() {
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -60.0);
    let feature = fx.source_muted();
    assert!(feature.muted);
    assert_eq!(feature.speech_probability, 0.0);
}

#[test]
#[ignore = "probe: crossing rate per shape"]
fn probe_rate_of_each_shape() {
    type Shape = (&'static str, Box<dyn Fn(usize) -> f64>);
    let shapes: [Shape; 4] = [
        (
            "steady machine",
            Box::new(|i| modulated(i, -45.0, 1.0, 0.2)),
        ),
        ("project voice fixture", Box::new(|i| voice_peak(i, -30.0))),
        (
            "speech 4 Hz/12 dB",
            Box::new(|i| modulated(i, -40.0, 12.0, 4.0)),
        ),
        (
            "speech 2 Hz/30 dB",
            Box::new(|i| modulated(i, -40.0, 30.0, 2.0)),
        ),
    ];
    for (name, shape) in shapes {
        let mut fx = FeatureExtractor::new();
        background(&mut fx, -70.0);
        for i in 0..400 {
            fx.observe(shape(i), 0.0, DT, true);
        }
        println!(
            "{name}: depth {:.3} dB, rate {:.2} Hz",
            fx.modulation_db(),
            fx.cross_hz()
        );
    }
}

#[test]
fn rebase_moves_every_level_history_without_synthetic_modulation() {
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -60.0);
    let before = fx.modulation_db();
    fx.rebase(18.0);
    let result = fx.observe(-42.0, 0.0, DT, true);
    assert!((fx.modulation_db() - before).abs() < 1e-9);
    assert_eq!(result.speech_probability, 0.0);
}

#[test]
fn a_rejected_speaker_cannot_inherit_the_previous_speech_hold() {
    let mut fx = FeatureExtractor::new();
    background(&mut fx, -80.0);
    for i in 0..100 {
        fx.observe(voice_peak(i, -30.0), 0.0, DT, true);
    }
    let reference = fx.primary_dbfs();
    let result = fx.observe(-5.0, 0.0, DT, false);
    assert_eq!(result.speech_probability, 0.0);
    assert_eq!(fx.primary_dbfs(), reference);
}

#[test]
fn deliberate_calibration_can_hear_below_the_daemons_absolute_floor() {
    let mut fx = FeatureExtractor::for_calibration();
    fx.observe(-120.0, 0.0, DT, true);
    let mut voiced = 0;
    for i in 0..100 {
        if fx
            .observe(voice_peak(i, -102.0), 0.0, DT, true)
            .speech_probability
            > 0.0
        {
            voiced += 1;
        }
    }
    assert!(voiced > 20);
}
